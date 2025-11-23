use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_SOCKET_PATH: &str = "/tmp/eww-pomodoro.sock";
const WORK_DURATION: u32 = 1500; // 25 minutes
const SHORT_BREAK: u32 = 300; // 5 minutes
const LONG_BREAK: u32 = 900; // 15 minutes

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

impl PomodoroState {
    fn new() -> Self {
        Self {
            status: "idle".to_string(),
            time_left: WORK_DURATION,
            time_display: "25:00".to_string(),
            sessions: 0,
            is_break: false,
            percent: 0,
            icon: "".to_string(),
        }
    }
}

struct PomodoroTimer {
    status: String,
    time_left: u32,
    sessions: u32,
    is_break: bool,
    running: bool,
    duration: u32,
}

impl PomodoroTimer {
    fn new() -> Self {
        Self {
            status: "idle".to_string(),
            time_left: WORK_DURATION,
            sessions: 0,
            is_break: false,
            running: false,
            duration: WORK_DURATION,
        }
    }

    fn get_state(&self) -> PomodoroState {
        let minutes = self.time_left / 60;
        let seconds = self.time_left % 60;
        let time_display = format!("{}:{:02}", minutes, seconds);
        let percent = if self.duration > 0 {
            ((self.duration - self.time_left) * 100) / self.duration
        } else {
            0
        };
        let icon = if self.is_break { "" } else { "" };

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
            self.status = "running".to_string();
            self.running = true;
            if self.is_break {
                self.duration = if self.sessions % 4 == 0 && self.sessions > 0 {
                    LONG_BREAK
                } else {
                    SHORT_BREAK
                };
            } else {
                self.duration = WORK_DURATION;
                self.sessions += 1;
            }
            self.time_left = self.duration;
        }
    }

    fn stop(&mut self) {
        self.status = "idle".to_string();
        self.running = false;
        self.time_left = WORK_DURATION;
        self.duration = WORK_DURATION;
        self.sessions = 0;
        self.is_break = false;
    }

    fn skip(&mut self) {
        self.running = false;
        self.status = "idle".to_string();
        if self.is_break {
            self.is_break = false;
            self.time_left = WORK_DURATION;
            self.duration = WORK_DURATION;
        } else {
            self.is_break = true;
            self.duration = if self.sessions % 4 == 0 && self.sessions > 0 {
                LONG_BREAK
            } else {
                SHORT_BREAK
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
                // Send notification
                let _ = std::process::Command::new("notify-send")
                    .args(["Break Over!", "Time to focus!", "-u", "normal"])
                    .spawn();

                self.is_break = false;
                self.time_left = WORK_DURATION;
                self.duration = WORK_DURATION;
            } else {
                // Work finished
                if self.sessions % 4 == 0 {
                    let _ = std::process::Command::new("notify-send")
                        .args(["Pomodoro Complete!", "Take a long break!", "-u", "normal"])
                        .spawn();
                    self.is_break = true;
                    self.time_left = LONG_BREAK;
                    self.duration = LONG_BREAK;
                } else {
                    let _ = std::process::Command::new("notify-send")
                        .args(["Pomodoro Complete!", "Take a short break!", "-u", "normal"])
                        .spawn();
                    self.is_break = true;
                    self.time_left = SHORT_BREAK;
                    self.duration = SHORT_BREAK;
                }
            }
            return true;
        }

        false
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum DaemonResponse {
    Success,
    State(PomodoroState),
}

fn run_daemon(socket_path: &str) -> anyhow::Result<()> {
    if std::path::Path::new(socket_path).exists() {
        if UnixStream::connect(socket_path).is_ok() {
            eprintln!("Daemon already running at {}", socket_path);
            return Ok(());
        }
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
    let timer = Arc::new(Mutex::new(PomodoroTimer::new()));
    let subscribers: Arc<Mutex<Vec<std::sync::mpsc::Sender<String>>>> =
        Arc::new(Mutex::new(Vec::new()));

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
                thread::spawn(move || {
                    let mut de = serde_json::Deserializer::from_reader(&stream);
                    if let Ok(cmd) = CliCommand::deserialize(&mut de) {
                        match cmd {
                            CliCommand::Daemon => {}
                            CliCommand::Kill => std::process::exit(0),
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
            Err(_) => break,
        }
    }
    Ok(())
}

fn send_client_command(socket_path: &str, cmd: CliCommand) -> anyhow::Result<()> {
    // Try to connect, if fails, start daemon
    let stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(_) => {
            // Daemon not running, start it
            eprintln!("Daemon not running, starting...");

            // Fork and start daemon
            unsafe {
                let pid = libc::fork();
                if pid == 0 {
                    // Child process - become daemon
                    libc::setsid();

                    // Start daemon
                    if let Err(e) = run_daemon(socket_path) {
                        eprintln!("Daemon error: {}", e);
                        std::process::exit(1);
                    }
                    std::process::exit(0);
                }
            }

            // Parent - wait for socket to appear
            for _ in 0..50 {
                if std::path::Path::new(socket_path).exists() {
                    thread::sleep(Duration::from_millis(100));
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }

            // Retry connection
            UnixStream::connect(socket_path)?
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

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        CliCommand::Daemon => run_daemon(&args.socket),
        cmd => send_client_command(&args.socket, cmd),
    }
}
