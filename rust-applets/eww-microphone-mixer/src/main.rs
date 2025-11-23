use clap::{Parser, Subcommand};
use libpulse_binding as pulse;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::Proplist;
use libpulse_binding::volume::Volume;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-microphone-mixer.sock";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,

    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket: String,
}

#[derive(Subcommand, Serialize, Deserialize, Debug, Clone, PartialEq)]
enum CliCommand {
    Daemon,
    Listen,
    Ping,
    GetState,
    SetSourceVolume { source_index: u32, volume: u8 },
    SetSourceOutputVolume { index: u32, volume: u8 },
    SetDefaultSource { source_name: String },
    MuteSource { source_index: u32, mute: bool },
    ToggleMuteSource { source_index: u32 },
    ToggleMuteDefault,
    VolumeUp,
    VolumeDown,
    MuteSourceOutput { index: u32, mute: bool },
    ToggleMuteSourceOutput { index: u32 },
    Kill,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SourceInfo {
    index: u32,
    name: String,
    description: String,
    volume: u8,
    muted: bool,
    is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SourceOutputInfo {
    index: u32,
    name: String,
    volume: u8,
    muted: bool,
    source_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct MicMixerState {
    sources: Vec<SourceInfo>,
    source_outputs: Vec<SourceOutputInfo>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum DaemonResponse {
    Success,
    Error(String),
    State(MicMixerState),
    Pong,
}

trait AudioBackend: Send + Sync {
    fn get_state(&self) -> Result<MicMixerState, String>;
    fn set_source_volume(&self, index: u32, percent: u8);
    fn set_output_volume(&self, index: u32, percent: u8);
    fn set_source_mute(&self, index: u32, mute: bool);
    fn toggle_source_mute(&self, index: u32);
    fn set_output_mute(&self, index: u32, mute: bool);
    fn toggle_output_mute(&self, index: u32);
    fn set_default_source(&self, name: &str);
}

struct PulseAudioClient {
    mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
}

unsafe impl Send for PulseAudioClient {}
unsafe impl Sync for PulseAudioClient {}

impl Drop for PulseAudioClient {
    fn drop(&mut self) {
        if let Ok(mut ctx) = self.context.try_borrow_mut() {
            ctx.disconnect();
        }
        if let Ok(mut ml) = self.mainloop.try_borrow_mut() {
            ml.stop();
        }
    }
}

impl PulseAudioClient {
    fn new() -> Result<Self, String> {
        let mut proplist = Proplist::new().unwrap();
        proplist
            .set_str(
                pulse::proplist::properties::APPLICATION_NAME,
                "EWW Microphone Mixer",
            )
            .unwrap();

        let mainloop = Rc::new(RefCell::new(
            Mainloop::new().ok_or("Failed to create mainloop")?,
        ));
        let context = Rc::new(RefCell::new(
            Context::new_with_proplist(
                &*mainloop.borrow(),
                "EWW Microphone Mixer Context",
                &proplist,
            )
            .ok_or("Failed to create context")?,
        ));

        context
            .borrow_mut()
            .connect(None, ContextFlagSet::NOFLAGS, None)
            .map_err(|e| format!("Failed to connect: {:?}", e))?;

        mainloop
            .borrow_mut()
            .start()
            .map_err(|_| "Failed to start mainloop")?;

        let start = std::time::Instant::now();
        loop {
            match context.borrow().get_state() {
                pulse::context::State::Ready => break,
                pulse::context::State::Failed | pulse::context::State::Terminated => {
                    return Err("Context failed".into());
                }
                _ => {
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err("Timeout waiting for PA context".into());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }

        Ok(Self { mainloop, context })
    }

    fn get_default_source_name(&self) -> String {
        let result = Arc::new(Mutex::new(String::new()));
        let result_clone = result.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        let introspector = self.context.borrow().introspect();
        introspector.get_server_info(move |info| {
            if let Some(name) = &info.default_source_name {
                *result_clone.lock().unwrap() = name.to_string();
            }
            *done_clone.lock().unwrap() = true;
        });

        for _ in 0..50 {
            if *done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        result.lock().unwrap().clone()
    }
}

impl AudioBackend for PulseAudioClient {
    fn get_state(&self) -> Result<MicMixerState, String> {
        let default_source = self.get_default_source_name();
        let sources = Arc::new(Mutex::new(Vec::new()));
        let source_outputs = Arc::new(Mutex::new(Vec::new()));

        let sources_clone = sources.clone();
        let default_clone = default_source.clone();
        let sources_done = Arc::new(Mutex::new(false));
        let sources_done_clone = sources_done.clone();

        self.context
            .borrow()
            .introspect()
            .get_source_info_list(move |res| {
                if let ListResult::Item(item) = res {
                    let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                    sources_clone.lock().unwrap().push(SourceInfo {
                        index: item.index,
                        name: item
                            .name
                            .as_ref()
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        description: item
                            .description
                            .as_ref()
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                        volume: vol,
                        muted: item.mute,
                        is_default: item.name.as_ref().map(|s| s.to_string())
                            == Some(default_clone.clone()),
                    });
                } else if let ListResult::End = res {
                    *sources_done_clone.lock().unwrap() = true;
                }
            });

        let outputs_clone = source_outputs.clone();
        let outputs_done = Arc::new(Mutex::new(false));
        let outputs_done_clone = outputs_done.clone();

        self.context
            .borrow()
            .introspect()
            .get_source_output_info_list(move |res| {
                if let ListResult::Item(item) = res {
                    let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                    let name = item
                        .proplist
                        .get_str(pulse::proplist::properties::APPLICATION_NAME)
                        .unwrap_or_else(|| "Unknown".to_string());
                    outputs_clone.lock().unwrap().push(SourceOutputInfo {
                        index: item.index,
                        name,
                        volume: vol,
                        muted: item.mute,
                        source_index: item.source,
                    });
                } else if let ListResult::End = res {
                    *outputs_done_clone.lock().unwrap() = true;
                }
            });

        for _ in 0..100 {
            if *sources_done.lock().unwrap() && *outputs_done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let mut sources_vec = sources.lock().unwrap().clone();
        let outputs_vec = source_outputs.lock().unwrap().clone();

        // Sort sources: default/active first, then others
        sources_vec.sort_by(|a, b| b.is_default.cmp(&a.is_default));

        Ok(MicMixerState {
            sources: sources_vec,
            source_outputs: outputs_vec,
        })
    }

    fn set_source_volume(&self, index: u32, percent: u8) {
        let vol_val = (Volume::NORMAL.0 as f64 * (percent.min(150) as f64 / 100.0)) as u32;
        let volume = Volume(vol_val);
        let channels = Arc::new(Mutex::new(None));
        let channels_clone = channels.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_source_info_by_index(index, move |res| {
                if let ListResult::Item(item) = res {
                    *channels_clone.lock().unwrap() = Some(item.volume);
                }
                *done_clone.lock().unwrap() = true;
            });

        for _ in 0..20 {
            if *done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let vol_opt = channels.lock().unwrap().clone();
        if let Some(mut cv) = vol_opt {
            cv.scale(volume);
            self.context
                .borrow()
                .introspect()
                .set_source_volume_by_index(index, &cv, None);
        }
    }

    fn set_output_volume(&self, index: u32, percent: u8) {
        let vol_val = (Volume::NORMAL.0 as f64 * (percent.min(150) as f64 / 100.0)) as u32;
        let volume = Volume(vol_val);
        let channels = Arc::new(Mutex::new(None));
        let channels_clone = channels.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_source_output_info(index, move |res| {
                if let ListResult::Item(item) = res {
                    *channels_clone.lock().unwrap() = Some(item.volume);
                }
                *done_clone.lock().unwrap() = true;
            });

        for _ in 0..20 {
            if *done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let vol_opt = channels.lock().unwrap().clone();
        if let Some(mut cv) = vol_opt {
            cv.scale(volume);
            self.context
                .borrow()
                .introspect()
                .set_source_output_volume(index, &cv, None);
        }
    }

    fn set_source_mute(&self, index: u32, mute: bool) {
        self.context
            .borrow()
            .introspect()
            .set_source_mute_by_index(index, mute, None);
    }

    fn toggle_source_mute(&self, index: u32) {
        let muted = Arc::new(Mutex::new(None));
        let muted_clone = muted.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_source_info_by_index(index, move |res| {
                if let ListResult::Item(item) = res {
                    *muted_clone.lock().unwrap() = Some(item.mute);
                }
                *done_clone.lock().unwrap() = true;
            });

        for _ in 0..20 {
            if *done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let is_muted_opt = *muted.lock().unwrap();
        if let Some(is_muted) = is_muted_opt {
            self.context
                .borrow()
                .introspect()
                .set_source_mute_by_index(index, !is_muted, None);
        }
    }

    fn set_output_mute(&self, index: u32, mute: bool) {
        self.context
            .borrow()
            .introspect()
            .set_source_output_mute(index, mute, None);
    }

    fn toggle_output_mute(&self, index: u32) {
        let muted = Arc::new(Mutex::new(None));
        let muted_clone = muted.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_source_output_info(index, move |res| {
                if let ListResult::Item(item) = res {
                    *muted_clone.lock().unwrap() = Some(item.mute);
                }
                *done_clone.lock().unwrap() = true;
            });

        for _ in 0..20 {
            if *done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let is_muted_opt = *muted.lock().unwrap();
        if let Some(is_muted) = is_muted_opt {
            self.context
                .borrow()
                .introspect()
                .set_source_output_mute(index, !is_muted, None);
        }
    }

    fn set_default_source(&self, name: &str) {
        self.context.borrow_mut().set_default_source(name, |_| {});
    }
}

fn run_daemon<B: AudioBackend + 'static>(socket_path: &str, backend: B) -> anyhow::Result<()> {
    if std::path::Path::new(socket_path).exists() {
        if let Ok(_) = std::os::unix::net::UnixStream::connect(socket_path) {
            eprintln!("Daemon already running at {}", socket_path);
            return Ok(());
        }
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
    let client = Arc::new(Mutex::new(backend));

    let subscribers: Arc<Mutex<Vec<std::sync::mpsc::Sender<String>>>> =
        Arc::new(Mutex::new(Vec::new()));

    let sub_client = client.clone();
    let sub_list = subscribers.clone();
    thread::spawn(move || {
        let mut last_state_json = String::new();
        loop {
            thread::sleep(Duration::from_millis(200));
            if sub_list.lock().unwrap().is_empty() {
                continue;
            }

            if let Ok(client) = sub_client.lock() {
                if let Ok(state) = client.get_state() {
                    if let Ok(json) = serde_json::to_string(&state) {
                        if json != last_state_json {
                            last_state_json = json.clone();
                            let mut list = sub_list.lock().unwrap();
                            list.retain(|tx| tx.send(json.clone()).is_ok());
                        }
                    }
                }
            }
        }
    });

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let client = client.clone();
                let sub_list = subscribers.clone();
                thread::spawn(move || {
                    let mut de = serde_json::Deserializer::from_reader(&stream);
                    if let Ok(cmd) = CliCommand::deserialize(&mut de) {
                        match cmd {
                            CliCommand::Daemon => {}
                            CliCommand::Kill => std::process::exit(0),
                            CliCommand::Ping => {
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Pong);
                            }
                            CliCommand::Listen => {
                                let (tx, rx) = std::sync::mpsc::channel();
                                if let Ok(c) = client.lock() {
                                    if let Ok(s) = c.get_state() {
                                        if let Ok(j) = serde_json::to_string(&s) {
                                            let _ = write!(stream, "{}\n", j);
                                        }
                                    }
                                }
                                sub_list.lock().unwrap().push(tx);
                                while let Ok(msg) = rx.recv() {
                                    if write!(stream, "{}\n", msg).is_err() {
                                        break;
                                    }
                                }
                            }
                            CliCommand::GetState => {
                                let res = match client.lock().unwrap().get_state() {
                                    Ok(s) => DaemonResponse::State(s),
                                    Err(e) => DaemonResponse::Error(e),
                                };
                                let _ = serde_json::to_writer(&stream, &res);
                            }
                            CliCommand::SetSourceVolume {
                                source_index,
                                volume,
                            } => {
                                client
                                    .lock()
                                    .unwrap()
                                    .set_source_volume(source_index, volume);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::SetSourceOutputVolume { index, volume } => {
                                client.lock().unwrap().set_output_volume(index, volume);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::MuteSource { source_index, mute } => {
                                client.lock().unwrap().set_source_mute(source_index, mute);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::ToggleMuteSource { source_index } => {
                                client.lock().unwrap().toggle_source_mute(source_index);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::MuteSourceOutput { index, mute } => {
                                client.lock().unwrap().set_output_mute(index, mute);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::ToggleMuteSourceOutput { index } => {
                                client.lock().unwrap().toggle_output_mute(index);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::ToggleMuteDefault => {
                                if let Ok(state) = client.lock().unwrap().get_state() {
                                    if let Some(default_source) = state.sources.iter().find(|s| s.is_default) {
                                        client.lock().unwrap().toggle_source_mute(default_source.index);
                                    }
                                }
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::VolumeUp => {
                                if let Ok(state) = client.lock().unwrap().get_state() {
                                    if let Some(default_source) = state.sources.iter().find(|s| s.is_default) {
                                        let new_vol = (default_source.volume + 5).min(100);
                                        client.lock().unwrap().set_source_volume(default_source.index, new_vol);
                                    }
                                }
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::VolumeDown => {
                                if let Ok(state) = client.lock().unwrap().get_state() {
                                    if let Some(default_source) = state.sources.iter().find(|s| s.is_default) {
                                        let new_vol = default_source.volume.saturating_sub(5);
                                        client.lock().unwrap().set_source_volume(default_source.index, new_vol);
                                    }
                                }
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::SetDefaultSource { source_name } => {
                                client.lock().unwrap().set_default_source(&source_name);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                        }
                    }
                });
            }
            Err(_) => {
                break;
            }
        }
    }
    Ok(())
}

fn send_client_command(socket_path: &str, cmd: CliCommand) -> anyhow::Result<()> {
    let stream = UnixStream::connect(socket_path).map_err(|_| {
        anyhow::anyhow!("Daemon not running at {}. Run 'daemon' first.", socket_path)
    })?;

    if let CliCommand::Listen = cmd {
        serde_json::to_writer(&stream, &cmd)?;
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            println!("{}", line?);
        }
        return Ok(());
    }

    serde_json::to_writer(&stream, &cmd)?;

    let mut de = serde_json::Deserializer::from_reader(stream);
    let response = DaemonResponse::deserialize(&mut de)?;

    match response {
        DaemonResponse::Success => Ok(()),
        DaemonResponse::Pong => {
            println!("Pong");
            Ok(())
        }
        DaemonResponse::State(s) => {
            println!("{}", serde_json::to_string(&s)?);
            Ok(())
        }
        DaemonResponse::Error(e) => Err(anyhow::anyhow!("Daemon error: {}", e)),
    }
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        CliCommand::Daemon => {
            let client = PulseAudioClient::new().map_err(|e| anyhow::anyhow!(e))?;
            run_daemon(&args.socket, client)
        }
        cmd => send_client_command(&args.socket, cmd),
    }
}
