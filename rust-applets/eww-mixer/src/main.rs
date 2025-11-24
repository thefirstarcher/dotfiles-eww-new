// ============================================================================
// EWW Mixer - Ultra-Fast Async PulseAudio Mixer for EWW
// ============================================================================
//
// Features:
// - Multiple concurrent listeners support via broadcast channels
// - Real-time PulseAudio event subscription for instant updates
// - **REAL-TIME PEAK VOLUME LEVEL MONITORING for visual feedback**
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
// - **Monitor Stream: Dedicated stream for calculating peak audio level**
//
// ============================================================================

use clap::{Parser, Subcommand, ValueEnum};
use libpulse_binding::{
    callbacks::ListResult,
    context::{subscribe::Facility, Context, FlagSet as ContextFlagSet},
    mainloop::threaded::Mainloop,
    proplist::Proplist,
    sample::{Format, Spec},
    stream::{FlagSet as StreamFlagSet, Stream},
    volume::Volume,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-mixer.sock";
const BROADCAST_CHANNEL_SIZE: usize = 100;
const STATE_UPDATE_INTERVAL_MS: u64 = 50;

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
    volume_level: u8, // Peak level for visualization (0-100) - now real-time
    mic_percent: u8,
    mic_muted: bool,
    mic_level: u8, // Peak level for visualization (0-100) - now real-time
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
    GetState(std::sync::mpsc::Sender<MixerState>),
    SetVolume {
        target: AudioTarget,
        index: u32,
        percent: u8,
        response: std::sync::mpsc::Sender<Result<(), String>>,
    },
    ToggleMute {
        target: AudioTarget,
        index: u32,
        response: std::sync::mpsc::Sender<Result<(), String>>,
    },
    SetDefault {
        target: DefaultTarget,
        name: String,
        response: std::sync::mpsc::Sender<Result<(), String>>,
    },
    Subscribe,
}

// ============================================================================
// PULSEAUDIO ACTOR (Runs in dedicated thread)
// ============================================================================

struct PulseAudioActor {
    mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
    last_state: MixerState,
    broadcast_tx: Option<std::sync::mpsc::Sender<MixerState>>,

    // Sink monitoring
    monitor_stream: Option<Rc<RefCell<Stream>>>,
    peak_level: Arc<AtomicU8>,

    // Source (Mic) monitoring
    mic_monitor_stream: Option<Rc<RefCell<Stream>>>, // NEW
    mic_peak_level: Arc<AtomicU8>,
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

        let mainloop =
            Mainloop::new().ok_or_else(|| anyhow::anyhow!("Failed to create mainloop"))?;

        let context = Context::new_with_proplist(&mainloop, "EWW Mixer Context", &proplist)
            .ok_or_else(|| anyhow::anyhow!("Failed to create context"))?;

        let mainloop_rc = Rc::new(RefCell::new(mainloop));
        let context_rc = Rc::new(RefCell::new(context));

        context_rc
            .borrow_mut()
            .connect(None, ContextFlagSet::NOFLAGS, None)?;
        mainloop_rc.borrow_mut().start()?;

        // Wait for context to be ready
        let start = std::time::Instant::now();
        loop {
            match context_rc.borrow().get_state() {
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
            mainloop: mainloop_rc,
            context: context_rc,
            last_state: MixerState::default(),
            broadcast_tx: None,
            monitor_stream: None,
            peak_level: Arc::new(AtomicU8::new(0)),

            mic_monitor_stream: None, // NEW
            mic_peak_level: Arc::new(AtomicU8::new(0)),
        })
    }

    /// Calculates the peak volume (0-100) from raw audio data (PCM).
    fn calculate_peak_volume(data: &[u8]) -> u8 {
        if data.len() < 2 {
            return 0;
        }

        let samples: Vec<i16> = data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        if samples.is_empty() {
            return 0;
        }

        // Root mean square calculation (Adjusted reference for visual feedback)
        let sum_squares: f64 = samples.iter().map(|&s| (s as f64 * s as f64)).sum();
        let rms = (sum_squares / samples.len() as f64).sqrt();

        let reference_level = 12000.0;
        let normalized = (rms / reference_level).min(1.0);
        let compressed = normalized.powf(0.7);
        let result = (compressed * 115.0).min(100.0);
        result.round() as u8
    }

    /// Setup the monitor stream for the default sink (Output Volume Level)
    fn setup_monitor_stream(&mut self, default_sink_name: Option<String>) {
        // Destroy existing stream if it exists
        if let Some(stream_rc) = self.monitor_stream.take() {
            self.mainloop.borrow_mut().lock();
            stream_rc.borrow_mut().disconnect().unwrap_or_default();
            self.mainloop.borrow_mut().unlock();
        }

        let Some(default_sink_name) = default_sink_name else {
            self.peak_level.store(0, Ordering::Relaxed);
            return;
        };

        // Monitor stream reads from the default sink's monitor source
        let monitor_source_name = format!("{}.monitor", default_sink_name);

        let spec = Spec {
            format: Format::S16le,
            channels: 2,
            rate: 44100,
        };

        if !spec.is_valid() {
            eprintln!("Invalid sample spec for sink monitor");
            return;
        }

        self.mainloop.borrow_mut().lock();

        let stream = match Stream::new(
            &mut *self.context.borrow_mut(),
            "EWW Mixer Sink Monitor Stream", // Updated name
            &spec,
            None,
        ) {
            Some(s) => s,
            None => {
                eprintln!("Failed to create new sink monitor stream");
                self.mainloop.borrow_mut().unlock();
                return;
            }
        };

        let peak_level_clone = Arc::clone(&self.peak_level);
        let stream_rc = Rc::new(RefCell::new(stream));
        let stream_clone = Rc::clone(&stream_rc);

        let read_callback = move |len: usize| {
            if len == 0 {
                return;
            }

            let mut stream_ref = stream_clone.borrow_mut();

            match stream_ref.peek() {
                Ok(peek_result) => {
                    match peek_result {
                        libpulse_binding::stream::PeekResult::Data(data_slice) => {
                            if !data_slice.is_empty() && data_slice.len() % 2 == 0 {
                                let peak = PulseAudioActor::calculate_peak_volume(data_slice);
                                peak_level_clone.fetch_max(peak, Ordering::Relaxed);
                            }
                        }
                        _ => {}
                    }
                    stream_ref.discard().unwrap_or_default();
                }
                Err(e) => {
                    eprintln!("Sink Monitor Stream peek error: {:?}", e);
                }
            }
        };

        stream_rc
            .borrow_mut()
            .set_read_callback(Some(Box::new(read_callback)));

        let _ = stream_rc.borrow_mut().connect_record(
            Some(&monitor_source_name),
            None,
            StreamFlagSet::PEAK_DETECT,
        );

        self.monitor_stream = Some(stream_rc);
        self.mainloop.borrow_mut().unlock();
    }

    /// Setup the monitor stream for the default source (Mic Input Level)
    fn setup_mic_monitor_stream(&mut self, default_source_name: Option<String>) {
        // NEW FUNCTION
        // Destroy existing stream if it exists
        if let Some(stream_rc) = self.mic_monitor_stream.take() {
            self.mainloop.borrow_mut().lock();
            stream_rc.borrow_mut().disconnect().unwrap_or_default();
            self.mainloop.borrow_mut().unlock();
        }

        let Some(default_source_name) = default_source_name else {
            self.mic_peak_level.store(0, Ordering::Relaxed);
            return;
        };

        // Monitor stream reads directly from the default source
        let source_name = default_source_name;

        let spec = Spec {
            format: Format::S16le,
            channels: 1, // Mic input is often mono, use 1 channel
            rate: 44100,
        };

        if !spec.is_valid() {
            eprintln!("Invalid sample spec for source monitor");
            return;
        }

        self.mainloop.borrow_mut().lock();

        let stream = match Stream::new(
            &mut *self.context.borrow_mut(),
            "EWW Mixer Source Monitor Stream", // Updated name
            &spec,
            None,
        ) {
            Some(s) => s,
            None => {
                eprintln!("Failed to create new source monitor stream");
                self.mainloop.borrow_mut().unlock();
                return;
            }
        };

        let peak_level_clone = Arc::clone(&self.mic_peak_level);
        let stream_rc = Rc::new(RefCell::new(stream));
        let stream_clone = Rc::clone(&stream_rc);

        let read_callback = move |len: usize| {
            if len == 0 {
                return;
            }

            let mut stream_ref = stream_clone.borrow_mut();

            match stream_ref.peek() {
                Ok(peek_result) => {
                    match peek_result {
                        libpulse_binding::stream::PeekResult::Data(data_slice) => {
                            if !data_slice.is_empty() && data_slice.len() % 2 == 0 {
                                let peak = PulseAudioActor::calculate_peak_volume(data_slice);
                                peak_level_clone.fetch_max(peak, Ordering::Relaxed);
                            }
                        }
                        _ => {}
                    }
                    stream_ref.discard().unwrap_or_default();
                }
                Err(e) => {
                    eprintln!("Source Monitor Stream peek error: {:?}", e);
                }
            }
        };

        stream_rc
            .borrow_mut()
            .set_read_callback(Some(Box::new(read_callback)));

        // Connect record directly to the source name
        let _ = stream_rc.borrow_mut().connect_record(
            Some(&source_name),
            None,
            StreamFlagSet::PEAK_DETECT,
        );

        self.mic_monitor_stream = Some(stream_rc); // NEW FIELD
        self.mainloop.borrow_mut().unlock();
    }

    /// Subscribe to PulseAudio events for real-time updates
    fn setup_event_subscription(&mut self) {
        self.mainloop.borrow_mut().lock();

        let interest = Facility::Sink.to_interest_mask()
            | Facility::SinkInput.to_interest_mask()
            | Facility::Source.to_interest_mask()
            | Facility::SourceOutput.to_interest_mask()
            | Facility::Server.to_interest_mask();

        self.context.borrow_mut().subscribe(interest, |_| {});
        self.context
            .borrow_mut()
            .set_subscribe_callback(Some(Box::new(|_, _, _| {})));

        self.mainloop.borrow_mut().unlock();
    }

    /// Get complete mixer state with all devices and applications
    fn get_state(&mut self) -> MixerState {
        let mut state = MixerState::default();

        let (tx, rx) = std::sync::mpsc::channel();
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        introspect.get_server_info(move |info| {
            let sink_name = info.default_sink_name.as_ref().map(|s| s.to_string());
            let source_name = info.default_source_name.as_ref().map(|s| s.to_string());
            let _ = tx.send((sink_name, source_name));
        });
        self.mainloop.borrow_mut().unlock();

        let (default_sink_name, default_source_name) = rx.recv().unwrap_or((None, None));

        // Get all sinks
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = std::sync::mpsc::channel();
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
            sink.is_default = Some(&sink.name) == default_sink_name.as_ref();
            state.sinks.push(sink);
        }

        // Get all sink inputs
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
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
        self.mainloop.borrow_mut().unlock();

        while let Ok(Some(input)) = rx.recv() {
            state.sink_inputs.push(input);
        }

        // Get all sources
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
        let (tx, rx) = std::sync::mpsc::channel();
        introspect.get_source_info_list(move |res| match res {
            ListResult::Item(item) => {
                if let Some(name) = item.name.as_ref() {
                    if name.to_string().ends_with(".monitor") {
                        return;
                    }
                }
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
            source.is_default = Some(&source.name) == default_source_name.as_ref();
            state.sources.push(source);
        }

        // Get all source outputs
        self.mainloop.borrow_mut().lock();
        let introspect = self.context.borrow().introspect();
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
        self.mainloop.borrow_mut().unlock();

        while let Ok(Some(output)) = rx.recv() {
            state.source_outputs.push(output);
        }

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

        state.volume_level = self.peak_level.load(Ordering::Relaxed);
        state.mic_level = self.mic_peak_level.load(Ordering::Relaxed);

        state
    }

    /// Set volume for any audio target
    fn set_volume(&mut self, target: AudioTarget, index: u32, percent: u8) -> Result<(), String> {
        self.mainloop.borrow_mut().lock();

        let (tx, rx) = std::sync::mpsc::channel();
        let introspect = self.context.borrow().introspect();

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

        self.mainloop.borrow_mut().unlock();

        if let Ok(Some(mut volumes)) = rx.recv() {
            self.mainloop.borrow_mut().lock();
            let vol_value = (Volume::NORMAL.0 as f64 * percent.min(100) as f64 / 100.0) as u32;
            volumes.scale(Volume(vol_value));

            let mut introspect = self.context.borrow().introspect();
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
            self.mainloop.borrow_mut().unlock();
            Ok(())
        } else {
            Err(format!(
                "Failed to get current volume for {:?} {}",
                target, index
            ))
        }
    }

    /// Toggle mute for any audio target
    fn toggle_mute(&mut self, target: AudioTarget, index: u32) -> Result<(), String> {
        self.mainloop.borrow_mut().lock();

        let introspect = self.context.borrow().introspect();
        let (tx, rx) = std::sync::mpsc::channel();

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

        self.mainloop.borrow_mut().unlock();

        if let Ok(Some(currently_muted)) = rx.recv() {
            self.mainloop.borrow_mut().lock();
            let mut introspect = self.context.borrow().introspect();

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
            self.mainloop.borrow_mut().unlock();
            Ok(())
        } else {
            Err(format!(
                "Failed to get current mute state for {:?} {}",
                target, index
            ))
        }
    }

    /// Set default audio device
    fn set_default(&mut self, target: DefaultTarget, name: &str) -> Result<(), String> {
        self.mainloop.borrow_mut().lock();

        let (tx, rx) = std::sync::mpsc::channel();
        match target {
            DefaultTarget::Sink => {
                self.context
                    .borrow_mut()
                    .set_default_sink(name, move |success| {
                        let _ = tx.send(success);
                    });
            }
            DefaultTarget::Source => {
                self.context
                    .borrow_mut()
                    .set_default_source(name, move |success| {
                        let _ = tx.send(success);
                    });
            }
        }

        self.mainloop.borrow_mut().unlock();

        if rx.recv().unwrap_or(false) {
            // If the default device was changed, update the monitor streams
            if target == DefaultTarget::Sink {
                self.setup_monitor_stream(Some(name.to_string()));
            } else if target == DefaultTarget::Source {
                self.setup_mic_monitor_stream(Some(name.to_string()));
            }
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

    /// Main actor loop - processes commands from sync thread
    fn run_actor_loop(mut self, rx: std::sync::mpsc::Receiver<ActorCommand>) {
        self.setup_event_subscription();

        // Setup initial monitor streams
        let initial_state = self.get_state();
        let default_sink_name = initial_state
            .sinks
            .iter()
            .find(|s| s.is_default)
            .map(|s| s.name.clone());
        let default_source_name = initial_state
            .sources
            .iter()
            .find(|s| s.is_default)
            .map(|s| s.name.clone());

        // Initialize both monitor streams
        self.setup_monitor_stream(default_sink_name);
        self.setup_mic_monitor_stream(default_source_name);

        let mut last_update = std::time::Instant::now();
        let mut last_decay = std::time::Instant::now();

        loop {
            match rx.recv_timeout(std::time::Duration::from_millis(16)) {
                // ~60fps
                Ok(cmd) => match cmd {
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
                        self.broadcast_state_if_changed();
                    }
                    ActorCommand::ToggleMute {
                        target,
                        index,
                        response,
                    } => {
                        let result = self.toggle_mute(target, index);
                        let _ = response.send(result);
                        self.broadcast_state_if_changed();
                    }
                    ActorCommand::SetDefault {
                        target,
                        name,
                        response,
                    } => {
                        let result = self.set_default(target, &name);
                        let _ = response.send(result);

                        // Monitor stream update is now handled inside set_default
                        self.broadcast_state_if_changed();
                    }
                    ActorCommand::Subscribe => {
                        // Subscription handled via separate channel
                    }
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Timeout - continue to updates
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            // Decay peak levels smoothly
            if last_decay.elapsed() > std::time::Duration::from_millis(50) {
                let current = self.peak_level.load(Ordering::Relaxed);
                if current > 0 {
                    let new_val = current.saturating_sub(3);
                    self.peak_level.store(new_val, Ordering::Relaxed);
                }
                let current_mic = self.mic_peak_level.load(Ordering::Relaxed);
                if current_mic > 0 {
                    let new_val = current_mic.saturating_sub(3);
                    self.mic_peak_level.store(new_val, Ordering::Relaxed);
                }

                last_decay = std::time::Instant::now();
            }

            // Periodic state broadcast
            if last_update.elapsed() > std::time::Duration::from_millis(STATE_UPDATE_INTERVAL_MS) {
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
    cmd_tx: std::sync::mpsc::Sender<ActorCommand>,
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
            let (response_tx, response_rx) = std::sync::mpsc::channel();
            cmd_tx.send(ActorCommand::GetState(response_tx))?;
            let state = response_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap_or_default();

            let resp = serde_json::to_string(&DaemonResponse::State(state))?;
            writer.write_all(resp.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
        CliCommand::SetVolume {
            target,
            index,
            volume,
        } => {
            let (response_tx, response_rx) = std::sync::mpsc::channel();
            cmd_tx.send(ActorCommand::SetVolume {
                target,
                index,
                percent: volume,
                response: response_tx,
            })?;

            match response_rx.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(Ok(_)) => {
                    let resp = serde_json::to_string(&DaemonResponse::Success)?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Ok(Err(e)) => {
                    let resp = serde_json::to_string(&DaemonResponse::Error(e))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(_) => {
                    let resp =
                        serde_json::to_string(&DaemonResponse::Error("Timeout".to_string()))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        }
        CliCommand::ToggleMute { target, index } => {
            let (response_tx, response_rx) = std::sync::mpsc::channel();
            cmd_tx.send(ActorCommand::ToggleMute {
                target,
                index,
                response: response_tx,
            })?;

            match response_rx.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(Ok(_)) => {
                    let resp = serde_json::to_string(&DaemonResponse::Success)?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Ok(Err(e)) => {
                    let resp = serde_json::to_string(&DaemonResponse::Error(e))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(_) => {
                    let resp =
                        serde_json::to_string(&DaemonResponse::Error("Timeout".to_string()))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        }
        CliCommand::SetDefault { target, name } => {
            let (response_tx, response_rx) = std::sync::mpsc::channel();
            cmd_tx.send(ActorCommand::SetDefault {
                target,
                name,
                response: response_tx,
            })?;

            match response_rx.recv_timeout(std::time::Duration::from_secs(1)) {
                Ok(Ok(_)) => {
                    let resp = serde_json::to_string(&DaemonResponse::Success)?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Ok(Err(e)) => {
                    let resp = serde_json::to_string(&DaemonResponse::Error(e))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                Err(_) => {
                    let resp =
                        serde_json::to_string(&DaemonResponse::Error("Timeout".to_string()))?;
                    writer.write_all(resp.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
            }
        }
        CliCommand::Listen => {
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
            if std::path::Path::new(&args.socket).exists() {
                std::fs::remove_file(&args.socket)?;
            }

            let listener = UnixListener::bind(&args.socket)?;
            eprintln!("EWW Mixer: Listening on {}", args.socket);

            // Create channels
            let (broadcast_tx, broadcast_rx): (
                std::sync::mpsc::Sender<MixerState>,
                std::sync::mpsc::Receiver<MixerState>,
            ) = std::sync::mpsc::channel();

            let (cmd_tx, cmd_rx): (
                std::sync::mpsc::Sender<ActorCommand>,
                std::sync::mpsc::Receiver<ActorCommand>,
            ) = std::sync::mpsc::channel();

            let cmd_tx_clone = cmd_tx.clone();

            // Spawn actor thread
            let _actor_handle = std::thread::spawn(move || {
                let mut actor = PulseAudioActor::new().expect("Failed to create PulseAudio actor");
                // Set the broadcast channel
                actor.broadcast_tx = Some(broadcast_tx);
                actor.run_actor_loop(cmd_rx);
            });

            // Wait for actor to initialize
            std::thread::sleep(std::time::Duration::from_millis(300));

            // Get and print initial state
            let (response_tx, response_rx) = std::sync::mpsc::channel();
            cmd_tx.send(ActorCommand::GetState(response_tx))?;

            match response_rx.recv_timeout(std::time::Duration::from_secs(2)) {
                Ok(initial_state) => {
                    println!("{}", serde_json::to_string(&initial_state)?);
                }
                Err(e) => {
                    eprintln!("Failed to get initial state: {}", e);
                    return Err(anyhow::anyhow!("Initialization failed"));
                }
            }

            // Spawn stdout printer for state updates
            tokio::task::spawn_blocking(move || {
                while let Ok(state) = broadcast_rx.recv() {
                    println!("{}", serde_json::to_string(&state).unwrap_or_default());
                }
            });

            // Main connection loop
            loop {
                let (stream, _) = listener.accept().await?;
                let cmd_tx_clone = cmd_tx_clone.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, cmd_tx_clone).await {
                        eprintln!("Client error: {}", e);
                    }
                });
            }
        }
        cmd => match send_command(&args.socket, cmd).await {
            Ok(DaemonResponse::Success) => {}
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
        },
    }

    Ok(())
}
