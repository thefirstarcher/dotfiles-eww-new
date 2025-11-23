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

const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-volume-mixer.sock";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,

    /// Override socket path (mostly for testing)
    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket: String,
}

#[derive(Subcommand, Serialize, Deserialize, Debug, Clone, PartialEq)]
enum CliCommand {
    Daemon,
    Listen,
    Ping,
    GetState,
    SetSinkVolume { sink_index: u32, volume: u8 },
    SetSinkInputVolume { index: u32, volume: u8 },
    SetDefaultSink { sink_name: String },
    MuteSink { sink_index: u32, mute: bool },
    ToggleMuteSink { sink_index: u32 },
    MuteSinkInput { index: u32, mute: bool },
    ToggleMuteSinkInput { index: u32 },
    Kill,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SinkInfo {
    index: u32,
    name: String,
    description: String,
    volume: u8,
    muted: bool,
    is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SinkInputInfo {
    index: u32,
    name: String,
    volume: u8,
    muted: bool,
    sink_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct MixerState {
    sinks: Vec<SinkInfo>,
    sink_inputs: Vec<SinkInputInfo>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum DaemonResponse {
    Success,
    Error(String),
    State(MixerState),
    Pong,
}

// ==================================================================================
// TRAIT DEFINITION (Allows Mocking for Tests)
// ==================================================================================

trait AudioBackend: Send + Sync {
    fn get_state(&self) -> Result<MixerState, String>;
    fn set_sink_volume(&self, index: u32, percent: u8);
    fn set_input_volume(&self, index: u32, percent: u8);
    fn set_sink_mute(&self, index: u32, mute: bool);
    fn toggle_sink_mute(&self, index: u32);
    fn set_input_mute(&self, index: u32, mute: bool);
    fn toggle_input_mute(&self, index: u32);
    fn set_default_sink(&self, name: &str);
}

// ==================================================================================
// PULSE AUDIO CLIENT (Real Implementation)
// ==================================================================================

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
                "EWW Volume Mixer",
            )
            .unwrap();

        let mainloop = Rc::new(RefCell::new(
            Mainloop::new().ok_or("Failed to create mainloop")?,
        ));
        let context = Rc::new(RefCell::new(
            Context::new_with_proplist(&*mainloop.borrow(), "EWW Volume Mixer Context", &proplist)
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
                    return Err("Context failed".into())
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

    fn get_default_sink_name(&self) -> String {
        let result = Arc::new(Mutex::new(String::new()));
        let result_clone = result.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        let introspector = self.context.borrow().introspect();
        introspector.get_server_info(move |info| {
            if let Some(name) = &info.default_sink_name {
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

        let res = result.lock().unwrap().clone();
        res
    }
}

impl AudioBackend for PulseAudioClient {
    fn get_state(&self) -> Result<MixerState, String> {
        let default_sink = self.get_default_sink_name();
        let sinks = Arc::new(Mutex::new(Vec::new()));
        let sink_inputs = Arc::new(Mutex::new(Vec::new()));

        let sinks_clone = sinks.clone();
        let default_clone = default_sink.clone();
        let sinks_done = Arc::new(Mutex::new(false));
        let sinks_done_clone = sinks_done.clone();

        self.context
            .borrow()
            .introspect()
            .get_sink_info_list(move |res| {
                if let ListResult::Item(item) = res {
                    let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                    sinks_clone.lock().unwrap().push(SinkInfo {
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
                    *sinks_done_clone.lock().unwrap() = true;
                }
            });

        let inputs_clone = sink_inputs.clone();
        let inputs_done = Arc::new(Mutex::new(false));
        let inputs_done_clone = inputs_done.clone();

        self.context
            .borrow()
            .introspect()
            .get_sink_input_info_list(move |res| {
                if let ListResult::Item(item) = res {
                    let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                    let name = item
                        .proplist
                        .get_str(pulse::proplist::properties::APPLICATION_NAME)
                        .unwrap_or_else(|| "Unknown".to_string());
                    inputs_clone.lock().unwrap().push(SinkInputInfo {
                        index: item.index,
                        name,
                        volume: vol,
                        muted: item.mute,
                        sink_index: item.sink,
                    });
                } else if let ListResult::End = res {
                    *inputs_done_clone.lock().unwrap() = true;
                }
            });

        for _ in 0..100 {
            if *sinks_done.lock().unwrap() && *inputs_done.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let sinks_vec = sinks.lock().unwrap().clone();
        let inputs_vec = sink_inputs.lock().unwrap().clone();

        Ok(MixerState {
            sinks: sinks_vec,
            sink_inputs: inputs_vec,
        })
    }

    fn set_sink_volume(&self, index: u32, percent: u8) {
        let vol_val = (Volume::NORMAL.0 as f64 * (percent.min(150) as f64 / 100.0)) as u32;
        let volume = Volume(vol_val);
        let channels = Arc::new(Mutex::new(None));
        let channels_clone = channels.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_sink_info_by_index(index, move |res| {
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
                .set_sink_volume_by_index(index, &cv, None);
        }
    }

    fn set_input_volume(&self, index: u32, percent: u8) {
        let vol_val = (Volume::NORMAL.0 as f64 * (percent.min(150) as f64 / 100.0)) as u32;
        let volume = Volume(vol_val);
        let channels = Arc::new(Mutex::new(None));
        let channels_clone = channels.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_sink_input_info(index, move |res| {
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
                .set_sink_input_volume(index, &cv, None);
        }
    }

    fn set_sink_mute(&self, index: u32, mute: bool) {
        self.context
            .borrow()
            .introspect()
            .set_sink_mute_by_index(index, mute, None);
    }

    fn toggle_sink_mute(&self, index: u32) {
        let muted = Arc::new(Mutex::new(None));
        let muted_clone = muted.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_sink_info_by_index(index, move |res| {
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
                .set_sink_mute_by_index(index, !is_muted, None);
        }
    }

    fn set_input_mute(&self, index: u32, mute: bool) {
        self.context
            .borrow()
            .introspect()
            .set_sink_input_mute(index, mute, None);
    }

    fn toggle_input_mute(&self, index: u32) {
        let muted = Arc::new(Mutex::new(None));
        let muted_clone = muted.clone();
        let done = Arc::new(Mutex::new(false));
        let done_clone = done.clone();

        self.context
            .borrow()
            .introspect()
            .get_sink_input_info(index, move |res| {
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
                .set_sink_input_mute(index, !is_muted, None);
        }
    }

    fn set_default_sink(&self, name: &str) {
        self.context.borrow_mut().set_default_sink(name, |_| {});
    }
}

// ==================================================================================
// DAEMON LOGIC
// ==================================================================================

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

    // Subscribers for "Listen" mode
    let subscribers: Arc<Mutex<Vec<std::sync::mpsc::Sender<String>>>> =
        Arc::new(Mutex::new(Vec::new()));

    // Polling thread
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
                            CliCommand::SetSinkVolume { sink_index, volume } => {
                                client.lock().unwrap().set_sink_volume(sink_index, volume);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::SetSinkInputVolume { index, volume } => {
                                client.lock().unwrap().set_input_volume(index, volume);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::MuteSink { sink_index, mute } => {
                                client.lock().unwrap().set_sink_mute(sink_index, mute);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::ToggleMuteSink { sink_index } => {
                                client.lock().unwrap().toggle_sink_mute(sink_index);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::MuteSinkInput { index, mute } => {
                                client.lock().unwrap().set_input_mute(index, mute);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::ToggleMuteSinkInput { index } => {
                                client.lock().unwrap().toggle_input_mute(index);
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::SetDefaultSink { sink_name } => {
                                client.lock().unwrap().set_default_sink(&sink_name);
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

// ==================================================================================
// CLIENT LOGIC
// ==================================================================================

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

// ==================================================================================
// TESTS
// ==================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;

    // Mock Backend to simulate PulseAudio without needing a running server
    struct MockBackend {
        state: Arc<RwLock<MixerState>>,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                state: Arc::new(RwLock::new(MixerState {
                    sinks: vec![SinkInfo {
                        index: 1,
                        name: "test_sink".into(),
                        description: "Test Sink".into(),
                        volume: 50,
                        muted: false,
                        is_default: true,
                    }],
                    sink_inputs: vec![],
                })),
            }
        }
    }

    impl AudioBackend for MockBackend {
        fn get_state(&self) -> Result<MixerState, String> {
            Ok(self.state.read().unwrap().clone())
        }
        fn set_sink_volume(&self, _index: u32, percent: u8) {
            self.state.write().unwrap().sinks[0].volume = percent;
        }
        fn set_input_volume(&self, _index: u32, _percent: u8) {}
        fn set_sink_mute(&self, _index: u32, mute: bool) {
            self.state.write().unwrap().sinks[0].muted = mute;
        }
        fn toggle_sink_mute(&self, _index: u32) {
            let mut state = self.state.write().unwrap();
            let current = state.sinks[0].muted;
            state.sinks[0].muted = !current;
        }
        fn set_input_mute(&self, _index: u32, _mute: bool) {}
        fn toggle_input_mute(&self, _index: u32) {}
        fn set_default_sink(&self, _name: &str) {}
    }

    #[test]
    fn test_daemon_client_integration() {
        let socket_path = "/tmp/eww_test_mixer.sock";
        let backend = MockBackend::new();

        // Spawn Daemon in background thread
        thread::spawn(move || {
            let _ = run_daemon(socket_path, backend);
        });

        // Give daemon time to start
        thread::sleep(Duration::from_millis(100));

        // 1. Test Ping
        send_client_command(socket_path, CliCommand::Ping).expect("Ping failed");

        // 2. Test GetState
        // Note: send_client_command prints to stdout, so we assume success if no error.
        // For strict testing, we would refactor send_client_command to return data.
        send_client_command(socket_path, CliCommand::GetState).expect("GetState failed");

        // 3. Test SetVolume
        send_client_command(
            socket_path,
            CliCommand::SetSinkVolume {
                sink_index: 1,
                volume: 80,
            },
        )
        .expect("SetVolume failed");

        // 4. Verify State Changed (Ping again to ensure daemon still alive)
        send_client_command(socket_path, CliCommand::Ping).expect("Daemon died after set volume");

        // Cleanup
        let _ = send_client_command(socket_path, CliCommand::Kill);
    }

    #[test]
    fn test_serialization() {
        let cmd = CliCommand::SetSinkVolume {
            sink_index: 1,
            volume: 50,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: CliCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, parsed);
    }
}
