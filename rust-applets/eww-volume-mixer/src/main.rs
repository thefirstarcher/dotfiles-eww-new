use clap::{Parser, Subcommand};
use libpulse_binding as pulse;
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet as ContextFlagSet};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::Proplist;
use libpulse_binding::volume::{ChannelVolumes, Volume};
use serde::Serialize;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;
use std::time::Duration;
use std::fs;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Listen,
    Daemon,
    SetSinkVolume { sink_index: u32, volume: u8 },
    SetSinkInputVolume { index: u32, volume: u8 },
    SetDefaultSink { sink_name: String },
    MuteSink { sink_index: u32 },
    MuteSinkInput { index: u32 },
}

#[derive(Debug, Clone, Serialize)]
struct SinkInfo {
    index: u32,
    name: String,
    description: String,
    volume: u8,
    muted: bool,
    is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SinkInputInfo {
    index: u32,
    name: String,
    volume: u8,
    muted: bool,
    sink_index: u32,
}

#[derive(Debug, Clone, Serialize)]
struct MixerState {
    sinks: Vec<SinkInfo>,
    sink_inputs: Vec<SinkInputInfo>,
}

struct PulseAudioClient {
    mainloop: Rc<RefCell<Mainloop>>,
    context: Rc<RefCell<Context>>,
}

impl Drop for PulseAudioClient {
    fn drop(&mut self) {
        // Disconnect context before dropping
        self.context.borrow_mut().disconnect();
        // Stop mainloop
        self.mainloop.borrow_mut().stop();
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
            Context::new_with_proplist(
                &*mainloop.borrow(),
                "EWW Volume Mixer Context",
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

        // Wait for context to be ready
        loop {
            match context.borrow().get_state() {
                pulse::context::State::Ready => break,
                pulse::context::State::Failed | pulse::context::State::Terminated => {
                    return Err("Context failed".to_string());
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }

        Ok(Self { mainloop, context })
    }

    fn get_state(&self) -> Result<MixerState, String> {
        let sinks = Rc::new(RefCell::new(Vec::new()));
        let sink_inputs = Rc::new(RefCell::new(Vec::new()));
        let default_sink = Rc::new(RefCell::new(String::new()));

        // Get default sink name
        {
            let default_sink_clone = default_sink.clone();
            let introspector = self.context.borrow().introspect();
            introspector.get_server_info(move |info| {
                if let Some(default) = &info.default_sink_name {
                    *default_sink_clone.borrow_mut() = default.to_string();
                }
            });
        }

        std::thread::sleep(Duration::from_millis(50));

        // Get sinks
        {
            let sinks_clone = sinks.clone();
            let default_sink_clone = default_sink.clone();
            let introspector = self.context.borrow().introspect();
            introspector.get_sink_info_list(move |list_result| {
                if let ListResult::Item(sink) = list_result {
                    let volume_avg = (sink.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                    let default_name = default_sink_clone.borrow();
                    sinks_clone.borrow_mut().push(SinkInfo {
                        index: sink.index,
                        name: sink.name.as_ref().map(|s| s.to_string()).unwrap_or_default(),
                        description: sink.description.as_ref().map(|s| s.to_string()).unwrap_or_default(),
                        volume: volume_avg,
                        muted: sink.mute,
                        is_default: sink.name.as_ref().map(|s| s.as_ref() == default_name.as_str()).unwrap_or(false),
                    });
                }
            });
        }

        std::thread::sleep(Duration::from_millis(50));

        // Get sink inputs (playing apps)
        {
            let sink_inputs_clone = sink_inputs.clone();
            let introspector = self.context.borrow().introspect();
            introspector.get_sink_input_info_list(move |list_result| {
                if let ListResult::Item(input) = list_result {
                    let volume_avg = (input.volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0) as u8;
                    let name = input
                        .proplist
                        .get_str(pulse::proplist::properties::APPLICATION_NAME)
                        .unwrap_or_else(|| "Unknown".to_string());

                    sink_inputs_clone.borrow_mut().push(SinkInputInfo {
                        index: input.index,
                        name,
                        volume: volume_avg,
                        muted: input.mute,
                        sink_index: input.sink,
                    });
                }
            });
        }

        std::thread::sleep(Duration::from_millis(50));

        let sinks_vec = sinks.borrow().clone();
        let sink_inputs_vec = sink_inputs.borrow().clone();

        Ok(MixerState {
            sinks: sinks_vec,
            sink_inputs: sink_inputs_vec,
        })
    }

    fn set_sink_volume(&self, sink_index: u32, volume_percent: u8) -> Result<(), String> {
        let volume_percent = volume_percent.min(150); // Cap at 150%
        let volume = Volume((Volume::NORMAL.0 as f64 * volume_percent as f64 / 100.0) as u32);

        // Get current channel map from the sink
        let channels = Rc::new(RefCell::new(ChannelVolumes::default()));
        let channels_clone = channels.clone();

        let introspector = self.context.borrow().introspect();
        introspector.get_sink_info_by_index(sink_index, move |list_result| {
            if let ListResult::Item(sink) = list_result {
                *channels_clone.borrow_mut() = sink.volume;
            }
        });

        std::thread::sleep(Duration::from_millis(50));

        // Set all channels to the same volume
        let mut volumes = channels.borrow().clone();
        volumes.scale(volume);

        let mut introspector = self.context.borrow_mut().introspect();
        let success = Rc::new(RefCell::new(false));
        let success_clone = success.clone();

        introspector.set_sink_volume_by_index(sink_index, &volumes, Some(Box::new(move |s| {
            *success_clone.borrow_mut() = s;
        })));

        std::thread::sleep(Duration::from_millis(100));

        Ok(())
    }

    fn set_sink_input_volume(&self, index: u32, volume_percent: u8) -> Result<(), String> {
        let volume_percent = volume_percent.min(150);
        let volume = Volume((Volume::NORMAL.0 as f64 * volume_percent as f64 / 100.0) as u32);

        // Get current channel map from the sink input
        let channels = Rc::new(RefCell::new(ChannelVolumes::default()));
        let channels_clone = channels.clone();

        let introspector = self.context.borrow().introspect();
        introspector.get_sink_input_info(index, move |list_result| {
            if let ListResult::Item(input) = list_result {
                *channels_clone.borrow_mut() = input.volume;
            }
        });

        std::thread::sleep(Duration::from_millis(50));

        // Set all channels to the same volume
        let mut volumes = channels.borrow().clone();
        volumes.scale(volume);

        let mut introspector = self.context.borrow_mut().introspect();
        let success = Rc::new(RefCell::new(false));
        let success_clone = success.clone();

        introspector.set_sink_input_volume(index, &volumes, Some(Box::new(move |s| {
            *success_clone.borrow_mut() = s;
        })));

        std::thread::sleep(Duration::from_millis(100));

        Ok(())
    }

    fn set_default_sink(&self, sink_name: &str) -> Result<(), String> {
        let mut context = self.context.borrow_mut();
        context.set_default_sink(sink_name, |_| {});
        Ok(())
    }

    fn mute_sink(&self, sink_index: u32) -> Result<(), String> {
        let introspector = self.context.borrow().introspect();

        // Get current mute state and toggle it
        let muted = Rc::new(RefCell::new(false));
        let muted_clone = muted.clone();

        introspector.get_sink_info_by_index(sink_index, move |list_result| {
            if let ListResult::Item(sink) = list_result {
                *muted_clone.borrow_mut() = sink.mute;
            }
        });

        std::thread::sleep(Duration::from_millis(50));

        let new_mute = !*muted.borrow();

        let mut introspector = self.context.borrow_mut().introspect();
        introspector.set_sink_mute_by_index(sink_index, new_mute, None);

        Ok(())
    }

    fn mute_sink_input(&self, index: u32) -> Result<(), String> {
        let introspector = self.context.borrow().introspect();

        let muted = Rc::new(RefCell::new(false));
        let muted_clone = muted.clone();

        introspector.get_sink_input_info(index, move |list_result| {
            if let ListResult::Item(input) = list_result {
                *muted_clone.borrow_mut() = input.mute;
            }
        });

        std::thread::sleep(Duration::from_millis(50));

        let new_mute = !*muted.borrow();

        let mut introspector = self.context.borrow_mut().introspect();
        introspector.set_sink_input_mute(index, new_mute, None);

        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Listen => {
            let client = PulseAudioClient::new()?;
            loop {
                let state = client.get_state()?;
                let json = serde_json::to_string(&state)?;
                println!("{}", json);
                std::io::stdout().flush()?;
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        Commands::Daemon => {
            let client = PulseAudioClient::new()?;
            eprintln!("Daemon mode started, waiting for commands on stdin");

            let stdin = std::io::stdin();
            let reader = BufReader::new(stdin);

            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("Error reading line: {}", e);
                        break;
                    }
                };

                let parts: Vec<&str> = line.trim().split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                match parts[0] {
                    "set-sink-volume" if parts.len() == 3 => {
                        if let (Ok(sink_index), Ok(volume)) = (parts[1].parse::<u32>(), parts[2].parse::<u8>()) {
                            if let Err(e) = client.set_sink_volume(sink_index, volume) {
                                eprintln!("Error setting sink volume: {}", e);
                            }
                        }
                    }
                    "set-sink-input-volume" if parts.len() == 3 => {
                        if let (Ok(index), Ok(volume)) = (parts[1].parse::<u32>(), parts[2].parse::<u8>()) {
                            if let Err(e) = client.set_sink_input_volume(index, volume) {
                                eprintln!("Error setting sink input volume: {}", e);
                            }
                        }
                    }
                    "mute-sink" if parts.len() == 2 => {
                        if let Ok(sink_index) = parts[1].parse::<u32>() {
                            if let Err(e) = client.mute_sink(sink_index) {
                                eprintln!("Error muting sink: {}", e);
                            }
                        }
                    }
                    "mute-sink-input" if parts.len() == 2 => {
                        if let Ok(index) = parts[1].parse::<u32>() {
                            if let Err(e) = client.mute_sink_input(index) {
                                eprintln!("Error muting sink input: {}", e);
                            }
                        }
                    }
                    "set-default-sink" if parts.len() == 2 => {
                        if let Err(e) = client.set_default_sink(parts[1]) {
                            eprintln!("Error setting default sink: {}", e);
                        }
                    }
                    "quit" => {
                        eprintln!("Daemon shutting down");
                        break;
                    }
                    _ => {
                        eprintln!("Unknown command: {}", line);
                    }
                }
            }

            drop(client);
        }
        Commands::SetSinkVolume { sink_index, volume } => {
            let client = PulseAudioClient::new()?;
            client.set_sink_volume(sink_index, volume)?;
            // Explicitly drop to trigger cleanup
            drop(client);
        }
        Commands::SetSinkInputVolume { index, volume } => {
            let client = PulseAudioClient::new()?;
            client.set_sink_input_volume(index, volume)?;
            drop(client);
        }
        Commands::SetDefaultSink { sink_name } => {
            let client = PulseAudioClient::new()?;
            client.set_default_sink(&sink_name)?;
            drop(client);
        }
        Commands::MuteSink { sink_index } => {
            let client = PulseAudioClient::new()?;
            client.mute_sink(sink_index)?;
            drop(client);
        }
        Commands::MuteSinkInput { index } => {
            let client = PulseAudioClient::new()?;
            client.mute_sink_input(index)?;
            drop(client);
        }
    }

    Ok(())
}
