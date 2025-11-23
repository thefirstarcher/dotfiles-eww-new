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

const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-mixer.sock";

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

    // Sink (Output) Commands
    SetSinkVolume { sink_index: u32, volume: u8 },
    SetSinkInputVolume { index: u32, volume: u8 },
    SetDefaultSink { sink_name: String },
    ToggleMuteSink { sink_index: u32 },
    ToggleMuteDefault,
    VolumeUp,
    VolumeDown,
    ToggleMuteSinkInput { index: u32 },

    // Source (Input) Commands
    SetSourceVolume { source_index: u32, volume: u8 },
    SetSourceOutputVolume { index: u32, volume: u8 },
    SetDefaultSource { source_name: String },
    ToggleMuteSource { source_index: u32 },
    ToggleMuteMicDefault,
    MicVolumeUp,
    MicVolumeDown,
    ToggleMuteSourceOutput { index: u32 },

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
struct MixerState {
    // Volume (Output)
    volume_percent: u8,
    volume_muted: bool,
    volume_level: u8,
    sinks: Vec<SinkInfo>,
    sink_inputs: Vec<SinkInputInfo>,

    // Microphone (Input)
    mic_percent: u8,
    mic_muted: bool,
    mic_level: u8,
    sources: Vec<SourceInfo>,
    source_outputs: Vec<SourceOutputInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
enum DaemonResponse {
    Success,
    Error(String),
    State(MixerState),
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
                "EWW Unified Mixer",
            )
            .unwrap();

        let mainloop = Rc::new(RefCell::new(
            Mainloop::new().ok_or("Failed to create mainloop")?,
        ));
        let context = Rc::new(RefCell::new(
            Context::new_with_proplist(&*mainloop.borrow(), "EWW Unified Mixer Context", &proplist)
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
                    return Err("Context connection failed".into())
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

    fn get_state(&self) -> MixerState {
        let mut state = MixerState::default();

        self.mainloop.borrow_mut().lock();

        // 1. Get Default Sink
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();
        introspect.get_server_info(move |info| {
            let sink_name = info
                .default_sink_name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_default();
            let source_name = info
                .default_source_name
                .as_ref()
                .map(|n| n.to_string())
                .unwrap_or_default();
            let _ = tx.send((sink_name, source_name));
        });

        self.mainloop.borrow_mut().unlock();
        let (default_sink_name, default_source_name) = rx.recv().unwrap_or_default();
        self.mainloop.borrow_mut().lock();

        // 2. Get Sinks
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();
        introspect.get_sink_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let _ = tx.send(Some(SinkInfo {
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
        while let Ok(Some(mut sink)) = rx.recv() {
            sink.is_default = sink.name == default_sink_name;
            state.sinks.push(sink);
        }
        self.mainloop.borrow_mut().lock();

        // 3. Get Sink Inputs
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();
        introspect.get_sink_input_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let name = item
                    .proplist
                    .get_str(pulse::proplist::properties::APPLICATION_NAME)
                    .unwrap_or_else(|| "Unknown".to_string());
                let _ = tx.send(Some(SinkInputInfo {
                    index: item.index,
                    name,
                    volume: vol,
                    muted: item.mute,
                    sink_index: item.sink,
                }));
            }
            ListResult::End => {
                let _ = tx.send(None);
            }
            _ => {}
        });

        self.mainloop.borrow_mut().unlock();
        while let Ok(Some(input)) = rx.recv() {
            state.sink_inputs.push(input);
        }
        self.mainloop.borrow_mut().lock();

        // 4. Get Sources
        let introspect = self.context.borrow().introspect();
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

        // 5. Get Source Outputs
        let introspect = self.context.borrow().introspect();
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

        // Sort and finalize
        state.sinks.sort_by(|a, b| b.is_default.cmp(&a.is_default));
        state
            .sources
            .sort_by(|a, b| b.is_default.cmp(&a.is_default));

        if let Some(def) = state.sinks.iter().find(|s| s.is_default) {
            state.volume_percent = def.volume;
            state.volume_muted = def.muted;
        } else if let Some(first) = state.sinks.first() {
            state.volume_percent = first.volume;
            state.volume_muted = first.muted;
        }

        if let Some(def) = state.sources.iter().find(|s| s.is_default) {
            state.mic_percent = def.volume;
            state.mic_muted = def.muted;
        } else if let Some(first) = state.sources.first() {
            state.mic_percent = first.volume;
            state.mic_muted = first.muted;
        }

        state
    }

    // --- SINK ACTIONS ---

    fn set_sink_volume(&self, index: u32, percent: u8) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_sink_info_by_index(index, move |res| {
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
            introspect.set_sink_volume_by_index(index, &cv, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn set_sink_input_volume(&self, index: u32, percent: u8) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_sink_input_info(index, move |res| {
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
            introspect.set_sink_input_volume(index, &cv, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn toggle_sink_mute(&self, index: u32) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_sink_info_by_index(index, move |res| {
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
            introspect.set_sink_mute_by_index(index, !muted, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn toggle_sink_input_mute(&self, index: u32) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = mpsc::channel();

        introspect.get_sink_input_info(index, move |res| {
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
            introspect.set_sink_input_mute(index, !muted, None);
            self.mainloop.borrow_mut().unlock();
        }
    }

    fn set_default_sink(&self, name: &str) {
        self.mainloop.borrow_mut().lock();
        self.context.borrow_mut().set_default_sink(name, |_| {});
        self.mainloop.borrow_mut().unlock();
    }

    // --- SOURCE ACTIONS ---

    fn set_source_volume(&self, index: u32, percent: u8) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
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

    fn set_source_output_volume(&self, index: u32, percent: u8) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
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

    fn toggle_source_mute(&self, index: u32) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
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

    fn toggle_source_output_mute(&self, index: u32) {
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
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

// --- DAEMON RUNNER ---

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

        // Initial state print
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

                        // Sink commands
                        CliCommand::SetSinkVolume { sink_index, volume } => {
                            actor.set_sink_volume(sink_index, volume)
                        }
                        CliCommand::SetSinkInputVolume { index, volume } => {
                            actor.set_sink_input_volume(index, volume)
                        }
                        CliCommand::ToggleMuteSink { sink_index } => {
                            actor.toggle_sink_mute(sink_index)
                        }
                        CliCommand::ToggleMuteSinkInput { index } => {
                            actor.toggle_sink_input_mute(index)
                        }
                        CliCommand::SetDefaultSink { sink_name } => {
                            actor.set_default_sink(&sink_name)
                        }
                        CliCommand::ToggleMuteDefault => {
                            let s = actor.get_state();
                            if let Some(def) = s.sinks.iter().find(|x| x.is_default) {
                                actor.toggle_sink_mute(def.index);
                            }
                        }
                        CliCommand::VolumeUp => {
                            let s = actor.get_state();
                            if let Some(def) = s.sinks.iter().find(|x| x.is_default) {
                                actor.set_sink_volume(def.index, (def.volume + 5).min(100));
                            }
                        }
                        CliCommand::VolumeDown => {
                            let s = actor.get_state();
                            if let Some(def) = s.sinks.iter().find(|x| x.is_default) {
                                actor.set_sink_volume(def.index, def.volume.saturating_sub(5));
                            }
                        }

                        // Source commands
                        CliCommand::SetSourceVolume {
                            source_index,
                            volume,
                        } => actor.set_source_volume(source_index, volume),
                        CliCommand::SetSourceOutputVolume { index, volume } => {
                            actor.set_source_output_volume(index, volume)
                        }
                        CliCommand::ToggleMuteSource { source_index } => {
                            actor.toggle_source_mute(source_index)
                        }
                        CliCommand::ToggleMuteSourceOutput { index } => {
                            actor.toggle_source_output_mute(index)
                        }
                        CliCommand::SetDefaultSource { source_name } => {
                            actor.set_default_source(&source_name)
                        }
                        CliCommand::ToggleMuteMicDefault => {
                            let s = actor.get_state();
                            if let Some(def) = s.sources.iter().find(|x| x.is_default) {
                                actor.toggle_source_mute(def.index);
                            }
                        }
                        CliCommand::MicVolumeUp => {
                            let s = actor.get_state();
                            if let Some(def) = s.sources.iter().find(|x| x.is_default) {
                                actor.set_source_volume(def.index, (def.volume + 5).min(100));
                            }
                        }
                        CliCommand::MicVolumeDown => {
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
                        // Always refresh state after command
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

    // 2. MONITOR THREAD (Refresh every 2s)
    let sender_timer = sender.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        let _ = sender_timer.send(ActorMessage::Refresh);
    });

    // 3. LISTENER THREAD (Main)
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

    // Read command
    if let Ok(_) = reader.read_line(&mut line) {
        if let Ok(cmd) = serde_json::from_str::<CliCommand>(line.trim()) {
            let (tx, rx) = mpsc::channel();

            // Send to Actor
            if sender.send(ActorMessage::Command(cmd, tx)).is_ok() {
                // Wait for Actor response
                if let Ok(response) = rx.recv() {
                    // Write back
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
    // Attempt to connect
    let mut stream =
        UnixStream::connect(socket_path).map_err(|_| anyhow::anyhow!("Daemon not running"))?;

    // Send
    let json = serde_json::to_string(&cmd)?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n")?;

    // Receive
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
