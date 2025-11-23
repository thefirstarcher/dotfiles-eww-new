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
use std::sync::mpsc;
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
    Listen,
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
    percent: u8,
    muted: bool,
    level: u8,
    sources: Vec<SourceInfo>,
    source_outputs: Vec<SourceOutputInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
enum DaemonResponse {
    Success,
    Error(String),
    State(MicMixerState),
}

enum ActorMessage {
    Command(CliCommand, mpsc::Sender<DaemonResponse>),
    Refresh,
}

// --- PULSEAUDIO ACTOR ---

struct PulseAudioActor {
    mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
}

impl PulseAudioActor {
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

        // Wait for ready
        let start = std::time::Instant::now();
        loop {
            match context.borrow().get_state() {
                pulse::context::State::Ready => break,
                pulse::context::State::Failed | pulse::context::State::Terminated => {
                    return Err("Context connection failed".into());
                }
                _ => {
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err("Timeout waiting for PulseAudio".into());
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }

        Ok(Self { mainloop, context })
    }

    fn get_state(&self) -> MicMixerState {
        let mut state = MicMixerState::default();

        self.mainloop.borrow_mut().lock();

        // 1. Get Default Source
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();
        introspect.get_server_info(move |info| {
            let name = info
                .default_source_name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_default();
            let _ = tx.send(name);
        });

        self.mainloop.borrow_mut().unlock();
        let default_source_name = rx.recv().unwrap_or_default();
        self.mainloop.borrow_mut().lock();

        // 2. Get Sources
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();
        introspect.get_source_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let _ = tx.send(Some(SourceInfo {
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
                    is_default: false,
                }));
            }
            ListResult::End => {
                let _ = tx.send(None);
            }
            _ => {}
        });

        self.mainloop.borrow_mut().unlock();
        while let Ok(Some(mut source)) = rx.recv() {
            source.is_default = source.name == default_source_name;
            state.sources.push(source);
        }
        self.mainloop.borrow_mut().lock();

        // 3. Get Source Outputs
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();
        introspect.get_source_output_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let name = item
                    .proplist
                    .get_str(pulse::proplist::properties::APPLICATION_NAME)
                    .unwrap_or_else(|| "Unknown".to_string());
                let _ = tx.send(Some(SourceOutputInfo {
                    index: item.index,
                    name,
                    volume: vol,
                    muted: item.mute,
                    source_index: item.source,
                }));
            }
            ListResult::End => {
                let _ = tx.send(None);
            }
            _ => {}
        });

        self.mainloop.borrow_mut().unlock();
        while let Ok(Some(output)) = rx.recv() {
            state.source_outputs.push(output);
        }

        // Finalize
        state
            .sources
            .sort_by(|a, b| b.is_default.cmp(&a.is_default));
        if let Some(def) = state.sources.iter().find(|s| s.is_default) {
            state.percent = def.volume;
            state.muted = def.muted;
        } else if let Some(first) = state.sources.first() {
            state.percent = first.volume;
            state.muted = first.muted;
        }

        state
    }

    // Actions
    fn set_source_volume(&self, index: u32, percent: u8) {
        self.mainloop.borrow_mut().lock();
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_source_info_by_index(index, move |res| {
            if let ListResult::Item(item) = res {
                let _ = tx.send(Some(item.volume));
            } else if let ListResult::End = res {
                let _ = tx.send(None);
            }
        });
        self.mainloop.borrow_mut().unlock();

        if let Ok(Some(mut cv)) = rx.recv() {
            self.mainloop.borrow_mut().lock();
            let mut introspect = self.context.borrow().introspect();
            let v_val = (Volume::NORMAL.0 as f64 * (percent.min(150) as f64 / 100.0)) as u32;
            cv.scale(Volume(v_val));
            introspect.set_source_volume_by_index(index, &cv, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn set_output_volume(&self, index: u32, percent: u8) {
        self.mainloop.borrow_mut().lock();
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_source_output_info(index, move |res| {
            if let ListResult::Item(item) = res {
                let _ = tx.send(Some(item.volume));
            } else if let ListResult::End = res {
                let _ = tx.send(None);
            }
        });
        self.mainloop.borrow_mut().unlock();

        if let Ok(Some(mut cv)) = rx.recv() {
            self.mainloop.borrow_mut().lock();
            let mut introspect = self.context.borrow().introspect();
            let v_val = (Volume::NORMAL.0 as f64 * (percent.min(150) as f64 / 100.0)) as u32;
            cv.scale(Volume(v_val));
            introspect.set_source_output_volume(index, &cv, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn set_source_mute(&self, index: u32, mute: bool) {
        self.mainloop.borrow_mut().lock();
        let mut introspect = self.context.borrow().introspect();
        introspect.set_source_mute_by_index(index, mute, None);
        self.mainloop.borrow_mut().unlock();
    }

    fn set_output_mute(&self, index: u32, mute: bool) {
        self.mainloop.borrow_mut().lock();
        let mut introspect = self.context.borrow().introspect();
        introspect.set_source_output_mute(index, mute, None);
        self.mainloop.borrow_mut().unlock();
    }

    fn toggle_source_mute(&self, index: u32) {
        self.mainloop.borrow_mut().lock();
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_source_info_by_index(index, move |res| {
            if let ListResult::Item(item) = res {
                let _ = tx.send(Some(item.mute));
            } else if let ListResult::End = res {
                let _ = tx.send(None);
            }
        });
        self.mainloop.borrow_mut().unlock();

        if let Ok(Some(muted)) = rx.recv() {
            self.mainloop.borrow_mut().lock();
            let mut introspect = self.context.borrow().introspect();
            introspect.set_source_mute_by_index(index, !muted, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn toggle_output_mute(&self, index: u32) {
        self.mainloop.borrow_mut().lock();
        let mut introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_source_output_info(index, move |res| {
            if let ListResult::Item(item) = res {
                let _ = tx.send(Some(item.mute));
            } else if let ListResult::End = res {
                let _ = tx.send(None);
            }
        });
        self.mainloop.borrow_mut().unlock();

        if let Ok(Some(muted)) = rx.recv() {
            self.mainloop.borrow_mut().lock();
            let mut introspect = self.context.borrow().introspect();
            introspect.set_source_output_mute(index, !muted, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn set_default_source(&self, name: &str) {
        self.mainloop.borrow_mut().lock();
        self.context.borrow_mut().set_default_source(name, |_| {});
        self.mainloop.borrow_mut().unlock();
    }
}

// --- SERVER LOGIC ---

fn run_server(socket_path: &str) -> anyhow::Result<()> {
    if std::path::Path::new(socket_path).exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    let (sender, receiver) = mpsc::channel::<ActorMessage>();
    let sender_sock = sender.clone();

    // 1. ACTOR THREAD
    thread::spawn(move || {
        let actor = match PulseAudioActor::new() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Failed to init PulseAudio: {}", e);
                return;
            }
        };

        let init = actor.get_state();
        if let Ok(json) = serde_json::to_string(&init) {
            println!("{}", json);
        }

        while let Ok(msg) = receiver.recv() {
            match msg {
                ActorMessage::Refresh => {
                    let state = actor.get_state();
                    if let Ok(json) = serde_json::to_string(&state) {
                        println!("{}", json);
                    }
                }
                ActorMessage::Command(cmd, reply_tx) => {
                    let mut success = true;
                    match cmd {
                        CliCommand::GetState => {
                            let state = actor.get_state();
                            let _ = reply_tx.send(DaemonResponse::State(state));
                            continue;
                        }
                        CliCommand::Kill => std::process::exit(0),

                        CliCommand::SetSourceVolume {
                            source_index,
                            volume,
                        } => actor.set_source_volume(source_index, volume),
                        CliCommand::SetSourceOutputVolume { index, volume } => {
                            actor.set_output_volume(index, volume)
                        }
                        CliCommand::MuteSource { source_index, mute } => {
                            actor.set_source_mute(source_index, mute)
                        }
                        CliCommand::MuteSourceOutput { index, mute } => {
                            actor.set_output_mute(index, mute)
                        }
                        CliCommand::ToggleMuteSource { source_index } => {
                            actor.toggle_source_mute(source_index)
                        }
                        CliCommand::ToggleMuteSourceOutput { index } => {
                            actor.toggle_output_mute(index)
                        }
                        CliCommand::SetDefaultSource { source_name } => {
                            actor.set_default_source(&source_name)
                        }

                        CliCommand::ToggleMuteDefault => {
                            let s = actor.get_state();
                            if let Some(def) = s.sources.iter().find(|x| x.is_default) {
                                actor.toggle_source_mute(def.index);
                            }
                        }
                        CliCommand::VolumeUp => {
                            let s = actor.get_state();
                            if let Some(def) = s.sources.iter().find(|x| x.is_default) {
                                actor.set_source_volume(def.index, (def.volume + 5).min(100));
                            }
                        }
                        CliCommand::VolumeDown => {
                            let s = actor.get_state();
                            if let Some(def) = s.sources.iter().find(|x| x.is_default) {
                                actor.set_source_volume(def.index, def.volume.saturating_sub(5));
                            }
                        }
                        _ => {
                            success = false;
                        }
                    }

                    if success {
                        let _ = reply_tx.send(DaemonResponse::Success);
                        let state = actor.get_state();
                        if let Ok(json) = serde_json::to_string(&state) {
                            println!("{}", json);
                        }
                    } else {
                        let _ = reply_tx.send(DaemonResponse::Error("Unknown command".into()));
                    }
                }
            }
        }
    });

    // 2. MONITOR THREAD
    let sender_timer = sender.clone();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(2));
            let _ = sender_timer.send(ActorMessage::Refresh);
        }
    });

    // 3. LISTENER THREAD
    let listener = UnixListener::bind(socket_path)?;
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            let sender = sender_sock.clone();
            thread::spawn(move || {
                handle_client(stream, sender);
            });
        }
    }

    Ok(())
}

fn handle_client(stream: UnixStream, sender: mpsc::Sender<ActorMessage>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    if let Ok(_) = reader.read_line(&mut line) {
        if let Ok(cmd) = serde_json::from_str::<CliCommand>(line.trim()) {
            let (tx, rx) = mpsc::channel();
            if sender.send(ActorMessage::Command(cmd, tx)).is_ok() {
                if let Ok(response) = rx.recv() {
                    let mut stream = reader.into_inner();
                    if let Ok(json) = serde_json::to_string(&response) {
                        let _ = stream.write_all(json.as_bytes());
                        let _ = stream.write_all(b"\n");
                    }
                }
            }
        }
    }
}

fn send_command(socket_path: &str, cmd: CliCommand) -> anyhow::Result<()> {
    let mut stream =
        UnixStream::connect(socket_path).map_err(|_| anyhow::anyhow!("Daemon not running"))?;

    let json = serde_json::to_string(&cmd)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let response: DaemonResponse = serde_json::from_str(&line)?;

    match response {
        DaemonResponse::Success => Ok(()),
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
        CliCommand::Listen => run_server(&args.socket),
        cmd => send_command(&args.socket, cmd),
    }
}
