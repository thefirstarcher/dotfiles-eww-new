use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// --- CONFIGURATION CONSTANTS AND STRUCT ---

// Default values in SECONDS
const DEFAULT_WORK_DURATION: u32 = 1500; // 25 minutes
const DEFAULT_SHORT_BREAK: u32 = 300; // 5 minutes
const DEFAULT_LONG_BREAK: u32 = 900; // 15 minutes
const DEFAULT_LONG_BREAK_INTERVAL: u32 = 4; // Sessions before long break
const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-pomodoro.sock";

/// Holds all the timing configuration for the Pomodoro timer.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PomodoroConfig {
    // Note: Durations are stored in seconds internally for consistency.
    work_duration: u32,
    short_break: u32,
    long_break: u32,
    long_break_interval: u32,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        PomodoroConfig {
            work_duration: DEFAULT_WORK_DURATION,
            short_break: DEFAULT_SHORT_BREAK,
            long_break: DEFAULT_LONG_BREAK,
            long_break_interval: DEFAULT_LONG_BREAK_INTERVAL,
        }
    }
}

// --- CLI ARGUMENTS (Clap) ---

#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,

    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket: String,

    // New optional CLI overrides (in minutes for user convenience)
    #[arg(long, short, help = "Work duration in minutes (overrides config)")]
    work: Option<u32>,

    #[arg(
        long,
        short = 's',
        help = "Short break duration in minutes (overrides config)"
    )]
    short: Option<u32>,

    #[arg(
        long,
        short = 'l',
        help = "Long break duration in minutes (overrides config)"
    )]
    long: Option<u32>,

    #[arg(
        long,
        short = 'i',
        help = "Number of sessions before long break (overrides config)"
    )]
    interval: Option<u32>,

    #[arg(
        long,
        help = "Path to custom configuration file (default: ~/.config/eww-pomodoro/config.json)"
    )]
    config: Option<PathBuf>,
}

// ... (CliCommand and PomodoroState remain the same)

#[derive(Subcommand, Serialize, Deserialize, Debug, Clone, PartialEq)]
enum CliCommand {
    Daemon,
    Listen,
    Toggle,
    Stop,
    Skip,
    GetState,
    Kill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PomodoroState {
    status: String,
    time_left: u32,
    time_display: String,
    sessions: u32,
    is_break: bool,
    percent: u32,
    icon: String,
}

// --- TIMER IMPLEMENTATION ---

struct PomodoroTimer {
    config: PomodoroConfig, // Store the final, resolved config
    status: String,
    time_left: u32,
    sessions: u32,
    is_break: bool,
    running: bool,
    duration: u32, // The total duration of the current phase (used for percent calculation)
}

impl PomodoroTimer {
    fn new(config: PomodoroConfig) -> Self {
        Self {
            status: "idle".to_string(),
            time_left: config.work_duration,
            sessions: 0,
            is_break: false,
            running: false,
            duration: config.work_duration,
            config,
        }
    }

    fn get_state(&self) -> PomodoroState {
        let time_display = format_time(self.time_left);
        let percent = if self.duration > 0 {
            ((self.duration.saturating_sub(self.time_left)) * 100) / self.duration
        } else {
            0
        };
        let icon = if self.is_break { "休憩" } else { "作業" };

        PomodoroState {
            status: self.status.clone(),
            time_left: self.time_left,
            time_display,
            sessions: self.sessions,
            is_break: self.is_break,
            percent,
            icon: icon.to_string(),
        }
    }

    fn toggle(&mut self) {
        if self.running {
            self.status = "paused".to_string();
            self.running = false;
        } else if self.status == "paused" {
            self.status = "running".to_string();
            self.running = true;
        } else {
            // Starting a new session from idle/toggled back on
            self.status = "running".to_string();
            self.running = true;

            // Determine the duration if the timer was at 0 before starting,
            // otherwise continue the current time_left/duration
            if self.time_left == 0 || self.status == "idle" {
                if self.is_break {
                    self.duration = if self.sessions % self.config.long_break_interval == 0
                        && self.sessions > 0
                    {
                        self.config.long_break
                    } else {
                        self.config.short_break
                    };
                } else {
                    self.duration = self.config.work_duration;
                    self.sessions = self.sessions.saturating_add(1);
                }
                self.time_left = self.duration;
            }
        }
    }

    fn stop(&mut self) {
        self.status = "idle".to_string();
        self.running = false;
        self.time_left = self.config.work_duration;
        self.duration = self.config.work_duration;
        self.sessions = 0;
        self.is_break = false;
    }

    fn skip(&mut self) {
        // Stop running state immediately
        self.running = false;
        self.status = "idle".to_string();

        if self.is_break {
            // Skip break, start next work session
            self.is_break = false;
            self.time_left = self.config.work_duration;
            self.duration = self.config.work_duration;
        } else {
            // Skip work, start next break
            self.is_break = true;
            self.sessions = self.sessions.saturating_add(1); // Count the session that was skipped
            self.duration = if self.sessions % self.config.long_break_interval == 0 {
                self.config.long_break
            } else {
                self.config.short_break
            };
            self.time_left = self.duration;
        }
    }

    fn tick(&mut self) -> bool {
        if !self.running || self.time_left == 0 {
            return false;
        }

        self.time_left = self.time_left.saturating_sub(1);

        if self.time_left == 0 {
            self.running = false;
            self.status = "idle".to_string();

            if self.is_break {
                // Break finished
                let _ = std::process::Command::new("notify-send")
                    .args(["Break Over!", "Time to focus!", "-u", "normal"])
                    .spawn();

                self.is_break = false;
                self.time_left = self.config.work_duration;
                self.duration = self.config.work_duration;
            } else {
                // Work finished
                if self.sessions % self.config.long_break_interval == 0 {
                    let _ = std::process::Command::new("notify-send")
                        .args(["Pomodoro Complete!", "Take a long break!", "-u", "normal"])
                        .spawn();
                    self.is_break = true;
                    self.time_left = self.config.long_break;
                    self.duration = self.config.long_break;
                } else {
                    let _ = std::process::Command::new("notify-send")
                        .args(["Pomodoro Complete!", "Take a short break!", "-u", "normal"])
                        .spawn();
                    self.is_break = true;
                    self.time_left = self.config.short_break;
                    self.duration = self.config.short_break;
                }
            }
            return true;
        }

        false
    }
}

// --- CONFIGURATION MANAGEMENT FUNCTIONS ---

fn get_config_path(cli_path: Option<&PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = cli_path {
        return Ok(path.clone());
    }

    // Attempt to find the standard config directory
    let config_dir = dirs::config_dir()
        .context("Could not determine user configuration directory")?
        .join("eww-pomodoro");

    // Ensure the directory exists
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).context(format!(
            "Failed to create config directory: {:?}",
            config_dir
        ))?;
    }

    Ok(config_dir.join("config.json"))
}

fn load_config(cli: &Cli) -> anyhow::Result<PomodoroConfig> {
    let config_path = get_config_path(cli.config.as_ref())?;
    let mut config = PomodoroConfig::default();

    // 1. Load from Config File
    if config_path.exists() {
        let file_content = std::fs::read_to_string(&config_path)
            .context(format!("Failed to read config file: {:?}", config_path))?;

        let file_config: PomodoroConfig = serde_json::from_str(&file_content).context(format!(
            "Failed to parse JSON config from: {:?}",
            config_path
        ))?;

        // Use file values to override defaults
        config.work_duration = file_config.work_duration;
        config.short_break = file_config.short_break;
        config.long_break = file_config.long_break;
        config.long_break_interval = file_config.long_break_interval;
    } else {
        // If config file doesn't exist, create it with default values for user editing
        let default_json = serde_json::to_string_pretty(&config)?;
        let _ = std::fs::write(&config_path, default_json).context(format!(
            "Failed to write default config to: {:?}",
            config_path
        ));
    }

    // 2. Apply CLI Overrides (Highest Precedence)
    // CLI arguments are in minutes, so convert to seconds (* 60)
    if let Some(work) = cli.work {
        config.work_duration = work * 60;
    }
    if let Some(short) = cli.short {
        config.short_break = short * 60;
    }
    if let Some(long) = cli.long {
        config.long_break = long * 60;
    }
    if let Some(interval) = cli.interval {
        config.long_break_interval = interval;
    }

    Ok(config)
}

// Helper function to format seconds into MM:SS string
fn format_time(total_seconds: u32) -> String {
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{}:{:02}", minutes, seconds)
}

// --- DAEMON AND MAIN FUNCTIONS ---

// Helper to retry connection until success or timeout (10 seconds total)
fn retry_connect(socket_path: &str, retries: u32, sleep_ms: u64) -> anyhow::Result<UnixStream> {
    for _ in 0..retries {
        match UnixStream::connect(socket_path) {
            Ok(s) => return Ok(s),
            Err(_) => thread::sleep(Duration::from_millis(sleep_ms)),
        }
    }
    // If we exit the loop, connection failed
    Err(anyhow::anyhow!(
        "Failed to connect to daemon at {}",
        socket_path
    ))
}

// Update run_daemon to accept the resolved config
fn run_daemon(socket_path: &str, config: PomodoroConfig) -> anyhow::Result<()> {
    // If a daemon is already running and using the socket, exit early.
    if std::path::Path::new(socket_path).exists() {
        if UnixStream::connect(socket_path).is_ok() {
            eprintln!("Daemon already running at {}", socket_path);
            return Ok(());
        }
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
    let timer = Arc::new(Mutex::new(PomodoroTimer::new(config))); // Pass config here
    let subscribers: Arc<Mutex<Vec<std::sync::mpsc::Sender<String>>>> =
        Arc::new(Mutex::new(Vec::new()));

    // ARC the socket path string so it can be moved into the Kill command thread
    let socket_path_arc = Arc::new(socket_path.to_string());

    // Tick thread
    let tick_timer = timer.clone();
    let tick_subs = subscribers.clone();
    thread::spawn(move || {
        let mut last_state_json = String::new();
        loop {
            thread::sleep(Duration::from_secs(1));

            if let Ok(mut t) = tick_timer.lock() {
                t.tick();
                let state = t.get_state();

                if let Ok(json) = serde_json::to_string(&state) {
                    if json != last_state_json {
                        last_state_json = json.clone();
                        let mut list = tick_subs.lock().unwrap();
                        // Retain only the subscribers that are still alive (send successful)
                        list.retain(|tx| tx.send(json.clone()).is_ok());
                    }
                }
            }
        }
    });

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let timer = timer.clone();
                let sub_list = subscribers.clone();
                // Clone the Arc for the thread that might handle Kill
                let socket_path_for_kill = socket_path_arc.clone();

                thread::spawn(move || {
                    let mut de = serde_json::Deserializer::from_reader(&stream);
                    if let Ok(cmd) = CliCommand::deserialize(&mut de) {
                        match cmd {
                            CliCommand::Daemon => {}
                            CliCommand::Kill => {
                                // Use the owned path from the Arc
                                let _ = std::fs::remove_file(socket_path_for_kill.as_str());
                                std::process::exit(0)
                            }
                            CliCommand::Listen => {
                                let (tx, rx) = std::sync::mpsc::channel();
                                if let Ok(t) = timer.lock() {
                                    let state = t.get_state();
                                    if let Ok(j) = serde_json::to_string(&state) {
                                        let _ = write!(stream, "{}\n", j);
                                    }
                                }
                                sub_list.lock().unwrap().push(tx);
                                while let Ok(msg) = rx.recv() {
                                    // Exit loop if client stream is closed/broken
                                    if write!(stream, "{}\n", msg).is_err() {
                                        break;
                                    }
                                }
                            }
                            CliCommand::GetState => {
                                let state = timer.lock().unwrap().get_state();
                                let res = DaemonResponse::State(state);
                                let _ = serde_json::to_writer(&stream, &res);
                            }
                            CliCommand::Toggle => {
                                timer.lock().unwrap().toggle();
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::Stop => {
                                timer.lock().unwrap().stop();
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                            CliCommand::Skip => {
                                timer.lock().unwrap().skip();
                                let _ = serde_json::to_writer(&stream, &DaemonResponse::Success);
                            }
                        }
                    }
                });
            }
            // If listener fails (e.g., interrupted), break the loop
            Err(_) => break,
        }
    }
    // Clean up socket file on natural daemon exit (e.g., interrupted or break)
    let _ = std::fs::remove_file(socket_path_arc.as_str());
    Ok(())
}

// Updated function signature to accept the already-parsed Cli arguments
fn send_client_command(socket_path: &str, cmd: CliCommand, cli_args: &Cli) -> anyhow::Result<()> {
    // 1. Try to connect to an already running daemon
    let stream = match retry_connect(socket_path, 1, 0) {
        // Single, immediate attempt
        Ok(s) => s,
        Err(_) => {
            // 2. Daemon not running, start it by forking and executing run_daemon in the child
            eprintln!("Daemon not running, attempting to start in background...");

            // Fork and start daemon
            unsafe {
                let pid = libc::fork();
                if pid == 0 {
                    // Child process - become daemon
                    libc::setsid();

                    // Disconnect from controlling terminal (important for clean exit)
                    let _ = libc::close(0);
                    let _ = libc::close(1);
                    let _ = libc::close(2);

                    // Re-calculate the config using the CLI arguments provided to the parent.
                    // This is safe because cli_args are the same args passed to the parent process.
                    let config_for_daemon = match load_config(cli_args) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("Daemon startup: Failed to load configuration: {}", e);
                            std::process::exit(1);
                        }
                    };

                    // Execute the daemon logic directly in this process.
                    // This is synchronous and blocks until the listener is bound, ensuring the parent waits.
                    if let Err(e) = run_daemon(socket_path, config_for_daemon) {
                        eprintln!("Daemon error: {}", e);
                        // Exit with an error code if the daemon fails to bind/run
                        std::process::exit(1);
                    }
                    // This should be unreachable as run_daemon runs an infinite loop,
                    // but if it exits naturally, we exit the process.
                    std::process::exit(0);
                }
            }

            // 3. Parent - wait for and connect to the new daemon
            // 100 retries * 100ms = 10 seconds total wait time
            retry_connect(socket_path, 100, 100).context(format!(
                "Failed to connect to daemon after launch attempt at {}",
                socket_path
            ))?
        }
    };

    if let CliCommand::Listen = cmd {
        serde_json::to_writer(&stream, &cmd)?;
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            println!("{}", line?);
        }
        return Ok(());
    }

    serde_json::to_writer(&stream, &cmd)?;

    if matches!(cmd, CliCommand::GetState) {
        let mut de = serde_json::Deserializer::from_reader(stream);
        let response = DaemonResponse::deserialize(&mut de)?;
        if let DaemonResponse::State(s) = response {
            println!("{}", serde_json::to_string(&s)?);
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum DaemonResponse {
    Success,
    State(PomodoroState),
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Load config based on CLI arguments
    let config = load_config(&args)?;

    match args.command {
        // The Daemon case needs the config passed in directly
        CliCommand::Daemon => run_daemon(&args.socket, config),
        // Use 'ref cmd' to BORROW args.command instead of moving it.
        // Then we clone the command (cmd.clone()) to pass an owned value
        // to send_client_command, while keeping the rest of 'args' intact for '&args'.
        ref cmd => send_client_command(&args.socket, cmd.clone(), &args),
    }
}
