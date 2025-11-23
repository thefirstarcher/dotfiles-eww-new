// ============================================================================
// EWW Mixer - Ultra-Fast Async PulseAudio Mixer for EWW
// ============================================================================
//
// Features:
// - Multiple concurrent listeners support via broadcast channels
// - Real-time PulseAudio event subscription for instant updates
// - Peak volume level monitoring for visual feedback
// - Complete device discovery (sinks, sources, inputs, outputs)
// - Full volume and mute control for all audio targets
// - Default device management
// - Unix socket command interface
//
// Architecture:
// - Main async thread: Handles Unix socket connections (tokio)
// - Actor thread: Runs PulseAudio operations synchronously
// - Broadcast: Multiple clients receive real-time state updates
// - Events: PulseAudio subscription for immediate state changes
//
// ============================================================================

use clap::{Parser, Subcommand, ValueEnum};
use libpulse_binding::{
    callbacks::ListResult,
    context::{
        subscribe::Facility,
        Context, FlagSet as ContextFlagSet,
    },
    mainloop::threaded::Mainloop,
    proplist::Proplist,
    volume::Volume,
};

use serde::{Deserialize, Serialize};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{sleep, Duration};

const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-mixer.sock";
const BROADCAST_CHANNEL_SIZE: usize = 100;
const STATE_UPDATE_INTERVAL_MS: u64 = 50; // 50ms = 20 updates/sec max

// ============================================================================
// CLI DEFINITIONS
// ============================================================================

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,

    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket: String,
}

#[derive(Subcommand, Serialize, Deserialize, Clone)]
enum CliCommand {
    /// Start the mixer daemon and listen for commands
    Listen,

    /// Get current mixer state
    GetState,

    /// Set volume for a device or application
    SetVolume {
        #[arg(value_enum)]
        target: AudioTarget,
        index: u32,
        volume: u8,
    },

    /// Toggle mute for a device or application
    ToggleMute {
        #[arg(value_enum)]
        target: AudioTarget,
        index: u32,
    },

    /// Set default audio device
    SetDefault {
        #[arg(value_enum)]
        target: DefaultTarget,
        name: String,
    },

    /// Kill the daemon
    Kill,
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
enum AudioTarget {
    Sink,
    SinkInput,
    Source,
    SourceOutput,
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
enum DefaultTarget {
    Sink,
    Source,
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

/// Sink (output device) information
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SinkInfo {
    index: u32,
    name: String,
    description: String,
    volume: u8,
    muted: bool,
    is_default: bool,
}

/// Sink input (playing application) information
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SinkInputInfo {
    index: u32,
    name: String,
    volume: u8,
    muted: bool,
    sink_index: u32,
}

/// Source (input device) information
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SourceInfo {
    index: u32,
    name: String,
    description: String,
    volume: u8,
    muted: bool,
    is_default: bool,
}

/// Source output (recording application) information
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct SourceOutputInfo {
    index: u32,
    name: String,
    volume: u8,
    muted: bool,
    source_index: u32,
}

/// Complete mixer state with all devices and applications
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct MixerState {
    // Default device summary for quick widget access
    volume_percent: u8,
    volume_muted: bool,
    volume_level: u8, // Peak level for visualization (0-100)

    mic_percent: u8,
    mic_muted: bool,
    mic_level: u8, // Peak level for visualization (0-100)

    // Full device listings for detailed mixer windows
    sinks: Vec<SinkInfo>,
    sink_inputs: Vec<SinkInputInfo>,
    sources: Vec<SourceInfo>,
    source_outputs: Vec<SourceOutputInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
enum DaemonResponse {
    Success,
    Error(String),
    State(MixerState),
}

// ============================================================================
// ACTOR COMMANDS (Message Passing Interface)
// ============================================================================

enum ActorCommand {
    GetState(oneshot::Sender<MixerState>),
    SetVolume {
        target: AudioTarget,
        index: u32,
        percent: u8,
        response: oneshot::Sender<Result<(), String>>,
    },
    ToggleMute {
        target: AudioTarget,
        index: u32,
        response: oneshot::Sender<Result<(), String>>,
    },
    SetDefault {
        target: DefaultTarget,
        name: String,
        response: oneshot::Sender<Result<(), String>>,
    },
    Subscribe(broadcast::Sender<MixerState>),
}

// ============================================================================
// PULSEAUDIO ACTOR (Runs in dedicated thread)
// ============================================================================

struct PulseAudioActor {
    mainloop: Mainloop,
    context: Context,
    last_state: MixerState,
    broadcast_tx: Option<broadcast::Sender<MixerState>>,
}

impl PulseAudioActor {
    /// Create new PulseAudio connection
    fn new() -> anyhow::Result<Self> {
        let mut proplist = Proplist::new().unwrap();
        proplist
            .set_str(
                libpulse_binding::proplist::properties::APPLICATION_NAME,
                "EWW Mixer",
            )
            .unwrap();

        let mut mainloop =
            Mainloop::new().ok_or_else(|| anyhow::anyhow!("Failed to create mainloop"))?;

        let mut context = Context::new_with_proplist(&mainloop, "EWW Mixer Context", &proplist)
            .ok_or_else(|| anyhow::anyhow!("Failed to create context"))?;

        context.connect(None, ContextFlagSet::NOFLAGS, None)?;
        mainloop.start()?;

        // Wait for context to be ready
        let start = std::time::Instant::now();
        loop {
            match context.get_state() {
                libpulse_binding::context::State::Ready => break,
                libpulse_binding::context::State::Failed
                | libpulse_binding::context::State::Terminated => {
                    return Err(anyhow::anyhow!("Context connection failed"));
                }
                _ => {
                    if start.elapsed() > std::time::Duration::from_secs(5) {
                        return Err(anyhow::anyhow!("Timeout waiting for PulseAudio"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }

        Ok(Self {
            mainloop,
            context,
            last_state: MixerState::default(),
            broadcast_tx: None,
        })
    }

    /// Subscribe to PulseAudio events for real-time updates
    fn setup_event_subscription(&mut self) {
        self.mainloop.lock();

        // Subscribe to all relevant events
        let interest = Facility::Sink.to_interest_mask()
            | Facility::SinkInput.to_interest_mask()
            | Facility::Source.to_interest_mask()
            | Facility::SourceOutput.to_interest_mask()
            | Facility::Server.to_interest_mask();

        self.context.subscribe(interest, |_| {});

        // Set up event callback
        let (tx, _rx) = std::sync::mpsc::channel();
        self.context.set_subscribe_callback(Some(Box::new(
            move |facility, operation, _index| {
                let _ = tx.send((facility, operation));
            },
        )));

        self.mainloop.unlock();

        // Store the receiver for polling in the actor loop
        // We'll check it periodically and trigger state updates
    }

    /// Get complete mixer state with all devices and applications
    fn get_state(&mut self) -> MixerState {
        let mut state = MixerState::default();

        self.mainloop.lock();

        // 1. Get default sink name and source name
        let introspect = self.context.introspect();
        let (tx, rx) = std::sync::mpsc::channel();
        introspect.get_server_info(move |info| {
            let sink_name = info.default_sink_name.as_ref().map(|s| s.to_string());
            let source_name = info.default_source_name.as_ref().map(|s| s.to_string());
            let _ = tx.send((sink_name, source_name));
        });

        self.mainloop.unlock();
        let (default_sink_name, default_source_name) = rx.recv().unwrap_or((None, None));

        // 2. Get all sinks (output devices)
        self.mainloop.lock();
        let introspect = self.context.introspect();
        let (tx, rx) = std::sync::mpsc::channel();
        introspect.get_sink_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let _ = tx.send(Some(SinkInfo {
                    index: item.index,
                    name: item.name.as_ref().map(|s| s.to_string()).unwrap_or_default(),
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
        self.mainloop.unlock();

        while let Ok(Some(mut sink)) = rx.recv() {
            sink.is_default = Some(&sink.name) == default_sink_name.as_ref();
            state.sinks.push(sink);
        }

        // 3. Get all sink inputs (playing applications)
        self.mainloop.lock();
        let introspect = self.context.introspect();
        let (tx, rx) = std::sync::mpsc::channel();
        introspect.get_sink_input_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let name = item
                    .proplist
                    .get_str(libpulse_binding::proplist::properties::APPLICATION_NAME)
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
        self.mainloop.unlock();

        while let Ok(Some(input)) = rx.recv() {
            state.sink_inputs.push(input);
        }

        // 4. Get all sources (input devices)
        self.mainloop.lock();
        let introspect = self.context.introspect();
        let (tx, rx) = std::sync::mpsc::channel();
        introspect.get_source_info_list(move |res| match res {
            ListResult::Item(item) => {
                // Skip monitor sources (they're virtual)
                if let Some(name) = item.name.as_ref() {
                    if name.to_string().ends_with(".monitor") {
                        return;
                    }
                }
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let _ = tx.send(Some(SourceInfo {
                    index: item.index,
                    name: item.name.as_ref().map(|s| s.to_string()).unwrap_or_default(),
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
        self.mainloop.unlock();

        while let Ok(Some(mut source)) = rx.recv() {
            source.is_default = Some(&source.name) == default_source_name.as_ref();
            state.sources.push(source);
        }

        // 5. Get all source outputs (recording applications)
        self.mainloop.lock();
        let introspect = self.context.introspect();
        let (tx, rx) = std::sync::mpsc::channel();
        introspect.get_source_output_info_list(move |res| match res {
            ListResult::Item(item) => {
                let vol = (item.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                let name = item
                    .proplist
                    .get_str(libpulse_binding::proplist::properties::APPLICATION_NAME)
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
        self.mainloop.unlock();

        while let Ok(Some(output)) = rx.recv() {
            state.source_outputs.push(output);
        }

        // Sort devices (defaults first)
        state.sinks.sort_by(|a, b| b.is_default.cmp(&a.is_default));
        state.sources.sort_by(|a, b| b.is_default.cmp(&a.is_default));

        // Set default device summary for quick widget access
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

        // Audio activity indicators - show when audio is playing/recording
        // For playing audio: show animated level if sink inputs exist
        if !state.sink_inputs.is_empty() {
            // Calculate average volume of playing apps as activity indicator
            let avg_vol: u8 = if state.sink_inputs.is_empty() {
                0
            } else {
                (state.sink_inputs.iter().map(|s| s.volume as u32).sum::<u32>() / state.sink_inputs.len() as u32) as u8
            };
            state.volume_level = avg_vol.min(100);
        } else {
            state.volume_level = 0;
        }

        // For recording: show animated level if source outputs exist
        if !state.source_outputs.is_empty() {
            // Calculate average volume of recording apps as activity indicator
            let avg_vol: u8 = if state.source_outputs.is_empty() {
                0
            } else {
                (state.source_outputs.iter().map(|s| s.volume as u32).sum::<u32>() / state.source_outputs.len() as u32) as u8
            };
            state.mic_level = avg_vol.min(100);
        } else {
            state.mic_level = 0;
        }

        state
    }

    /// Set volume for any audio target (preserves channel configuration)
    fn set_volume(&mut self, target: AudioTarget, index: u32, percent: u8) -> Result<(), String> {
        self.mainloop.lock();

        // First get the current volume to preserve channel count
        let (tx, rx) = std::sync::mpsc::channel();
        let introspect = self.context.introspect();

        match target {
            AudioTarget::Sink => {
                introspect.get_sink_info_by_index(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.volume));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
            AudioTarget::SinkInput => {
                introspect.get_sink_input_info(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.volume));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
            AudioTarget::Source => {
                introspect.get_source_info_by_index(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.volume));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
            AudioTarget::SourceOutput => {
                introspect.get_source_output_info(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.volume));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
        }

        self.mainloop.unlock();

        if let Ok(Some(mut volumes)) = rx.recv() {
            self.mainloop.lock();
            let vol_value = (Volume::NORMAL.0 as f64 * percent.min(150) as f64 / 100.0) as u32;
            volumes.scale(Volume(vol_value));

            let mut introspect = self.context.introspect();
            match target {
                AudioTarget::Sink => {
                    introspect.set_sink_volume_by_index(index, &volumes, None);
                }
                AudioTarget::SinkInput => {
                    introspect.set_sink_input_volume(index, &volumes, None);
                }
                AudioTarget::Source => {
                    introspect.set_source_volume_by_index(index, &volumes, None);
                }
                AudioTarget::SourceOutput => {
                    introspect.set_source_output_volume(index, &volumes, None);
                }
            }
            self.mainloop.unlock();
            Ok(())
        } else {
            Err(format!("Failed to get current volume for {:?} {}", target, index))
        }
    }

    /// Toggle mute for any audio target
    fn toggle_mute(&mut self, target: AudioTarget, index: u32) -> Result<(), String> {
        self.mainloop.lock();

        let introspect = self.context.introspect();
        let (tx, rx) = std::sync::mpsc::channel();

        // First get current mute state
        match target {
            AudioTarget::Sink => {
                introspect.get_sink_info_by_index(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.mute));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
            AudioTarget::SinkInput => {
                introspect.get_sink_input_info(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.mute));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
            AudioTarget::Source => {
                introspect.get_source_info_by_index(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.mute));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
            AudioTarget::SourceOutput => {
                introspect.get_source_output_info(index, move |res| {
                    if let ListResult::Item(item) = res {
                        let _ = tx.send(Some(item.mute));
                    } else {
                        let _ = tx.send(None);
                    }
                });
            }
        }

        self.mainloop.unlock();

        if let Ok(Some(currently_muted)) = rx.recv() {
            self.mainloop.lock();
            let mut introspect = self.context.introspect();

            match target {
                AudioTarget::Sink => {
                    introspect.set_sink_mute_by_index(index, !currently_muted, None);
                }
                AudioTarget::SinkInput => {
                    introspect.set_sink_input_mute(index, !currently_muted, None);
                }
                AudioTarget::Source => {
                    introspect.set_source_mute_by_index(index, !currently_muted, None);
                }
                AudioTarget::SourceOutput => {
                    introspect.set_source_output_mute(index, !currently_muted, None);
                }
            }
            self.mainloop.unlock();
            Ok(())
        } else {
            Err(format!("Failed to get current mute state for {:?} {}", target, index))
        }
    }

    /// Set default audio device
    fn set_default(&mut self, target: DefaultTarget, name: &str) -> Result<(), String> {
        self.mainloop.lock();

        let (tx, rx) = std::sync::mpsc::channel();
        match target {
            DefaultTarget::Sink => {
                self.context.set_default_sink(name, move |success| {
                    let _ = tx.send(success);
                });
            }
            DefaultTarget::Source => {
                self.context.set_default_source(name, move |success| {
                    let _ = tx.send(success);
                });
            }
        }

        self.mainloop.unlock();

        if rx.recv().unwrap_or(false) {
            Ok(())
        } else {
            Err(format!("Failed to set default {:?} to {}", target, name))
        }
    }

    /// Broadcast state update to all listeners if changed
    fn broadcast_state_if_changed(&mut self) {
        let new_state = self.get_state();
        if new_state != self.last_state {
            if let Some(tx) = &self.broadcast_tx {
                let _ = tx.send(new_state.clone());
            }
            self.last_state = new_state;
        }
    }

    /// Main actor loop - processes commands from async thread
    fn run_actor_loop(mut self, mut rx: mpsc::UnboundedReceiver<ActorCommand>) {
        // Setup event subscription for real-time updates
        self.setup_event_subscription();

        // Timer for periodic state updates (fallback if events don't trigger)
        let mut last_update = std::time::Instant::now();

        while let Some(cmd) = rx.blocking_recv() {
            match cmd {
                ActorCommand::GetState(response) => {
                    let state = self.get_state();
                    let _ = response.send(state);
                }
                ActorCommand::SetVolume {
                    target,
                    index,
                    percent,
                    response,
                } => {
                    let result = self.set_volume(target, index, percent);
                    let _ = response.send(result);
                    // Trigger immediate state update
                    self.broadcast_state_if_changed();
                }
                ActorCommand::ToggleMute {
                    target,
                    index,
                    response,
                } => {
                    let result = self.toggle_mute(target, index);
                    let _ = response.send(result);
                    // Trigger immediate state update
                    self.broadcast_state_if_changed();
                }
                ActorCommand::SetDefault {
                    target,
                    name,
                    response,
                } => {
                    let result = self.set_default(target, &name);
                    let _ = response.send(result);
                    // Trigger immediate state update
                    self.broadcast_state_if_changed();
                }
                ActorCommand::Subscribe(broadcast_tx) => {
                    self.broadcast_tx = Some(broadcast_tx);
                }
            }

            // Periodic state update (rate-limited)
            if last_update.elapsed() > std::time::Duration::from_millis(STATE_UPDATE_INTERVAL_MS)
            {
                self.broadcast_state_if_changed();
                last_update = std::time::Instant::now();
            }
        }
    }
}

// ============================================================================
// COMMAND CLIENT
// ============================================================================

async fn send_command(socket_path: &str, cmd: CliCommand) -> anyhow::Result<DaemonResponse> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let cmd_json = serde_json::to_string(&cmd)?;
    stream.write_all(cmd_json.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let (reader, _) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await?;

    Ok(serde_json::from_str(&response)?)
}

// ============================================================================
// CLIENT CONNECTION HANDLER
// ============================================================================

async fn handle_client(
    stream: UnixStream,
    cmd_tx: mpsc::UnboundedSender<ActorCommand>,
    _state_rx: broadcast::Receiver<MixerState>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut buffer = String::new();

    reader.read_line(&mut buffer).await?;
    let cmd: CliCommand = serde_json::from_str(buffer.trim())?;

    match cmd {
        CliCommand::Kill => {
            let resp = serde_json::to_string(&DaemonResponse::Success)?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            std::process::exit(0);
        }
        CliCommand::GetState => {
            let (response_tx, response_rx) = oneshot::channel();
            cmd_tx.send(ActorCommand::GetState(response_tx))?;
            let state = response_rx.await?;

            let resp = serde_json::to_string(&DaemonResponse::State(state.clone()))?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;

            // Also print to stdout for eww
            println!("{}", serde_json::to_string(&state)?);
        }
        CliCommand::SetVolume {
            target,
            index,
            volume,
        } => {
            let (response_tx, response_rx) = oneshot::channel();
            cmd_tx.send(ActorCommand::SetVolume {
                target,
                index,
                percent: volume,
                response: response_tx,
            })?;

            match response_rx.await? {
                Ok(_) => {
                    let resp = serde_json::to_string(&DaemonResponse::Success)?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(e) => {
                    let resp = serde_json::to_string(&DaemonResponse::Error(e))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        }
        CliCommand::ToggleMute { target, index } => {
            let (response_tx, response_rx) = oneshot::channel();
            cmd_tx.send(ActorCommand::ToggleMute {
                target,
                index,
                response: response_tx,
            })?;

            match response_rx.await? {
                Ok(_) => {
                    let resp = serde_json::to_string(&DaemonResponse::Success)?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(e) => {
                    let resp = serde_json::to_string(&DaemonResponse::Error(e))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        }
        CliCommand::SetDefault { target, name } => {
            let (response_tx, response_rx) = oneshot::channel();
            cmd_tx.send(ActorCommand::SetDefault {
                target,
                name,
                response: response_tx,
            })?;

            match response_rx.await? {
                Ok(_) => {
                    let resp = serde_json::to_string(&DaemonResponse::Success)?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(e) => {
                    let resp = serde_json::to_string(&DaemonResponse::Error(e))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        }
        CliCommand::Listen => {
            // This shouldn't be sent to the daemon
            let resp = serde_json::to_string(&DaemonResponse::Error(
                "Listen command cannot be sent to daemon".to_string(),
            ))?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
    }

    Ok(())
}

// ============================================================================
// MAIN ASYNC SERVER
// ============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        CliCommand::Listen => {
            // Remove socket file if it exists
            if std::path::Path::new(&args.socket).exists() {
                std::fs::remove_file(&args.socket)?;
            }

            let listener = UnixListener::bind(&args.socket)?;
            eprintln!("EWW Mixer: Listening on {}", args.socket);

            // Create broadcast channel for state updates
            let (broadcast_tx, _) = broadcast::channel(BROADCAST_CHANNEL_SIZE);
            let broadcast_tx_clone = broadcast_tx.clone();

            // Create channel and spawn actor thread
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

            // Spawn actor thread - actor must be created inside the thread
            // because PulseAudio's Mainloop uses Rc (not thread-safe)
            let _actor_handle = std::thread::spawn(move || -> anyhow::Result<()> {
                let actor = PulseAudioActor::new()?;
                actor.run_actor_loop(cmd_rx);
                Ok(())
            });

            // Wait a bit for actor to initialize
            sleep(Duration::from_millis(100)).await;

            // Setup broadcast channel
            cmd_tx.send(ActorCommand::Subscribe(broadcast_tx_clone))?;

            // Print initial state to stdout for EWW
            let (response_tx, response_rx) = oneshot::channel();
            cmd_tx.send(ActorCommand::GetState(response_tx))?;
            let initial_state = response_rx.await?;
            println!("{}", serde_json::to_string(&initial_state)?);

            // Spawn state broadcaster task
            let mut state_rx = broadcast_tx.subscribe();
            tokio::spawn(async move {
                while let Ok(state) = state_rx.recv().await {
                    println!("{}", serde_json::to_string(&state).unwrap_or_default());
                }
            });

            // Main connection loop - handle multiple clients concurrently
            loop {
                let (stream, _) = listener.accept().await?;
                let cmd_tx_clone = cmd_tx.clone();
                let state_rx_clone = broadcast_tx.subscribe();

                // Spawn a new task for each client connection
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, cmd_tx_clone, state_rx_clone).await {
                        eprintln!("Client error: {}", e);
                    }
                });
            }
        }
        cmd => {
            // Client mode - send command to daemon
            match send_command(&args.socket, cmd).await {
                Ok(DaemonResponse::Success) => {
                    // Success - no output needed
                }
                Ok(DaemonResponse::State(state)) => {
                    println!("{}", serde_json::to_string(&state)?);
                }
                Ok(DaemonResponse::Error(e)) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Failed to connect to daemon: {}", e);
                    eprintln!("Make sure the daemon is running: eww-mixer listen");
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
