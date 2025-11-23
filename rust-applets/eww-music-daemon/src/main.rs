use clap::{Parser, Subcommand};
use mpris::{LoopStatus, PlaybackStatus, PlayerFinder};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::time::Duration;

const SWITCH_FILE: &str = "/tmp/eww-music-player-switch";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Listen,
    PlayPause,
    Previous,
    Next,
    Seek { percent: f64 },
    Switch { bus_name: String },
    Cycle { direction: String },
    Volume { action: String },
    Shuffle,
    Loop,
    PlaybackRate { rate: f64 },
}

#[derive(Debug, Clone, Serialize)]
struct EwwMusicState {
    active_player: String,
    active_bus: String,
    available_players: Vec<String>,
    next_player: String,
    prev_player: String,
    title: String,
    artist: String,
    album: String,
    art_url: String,
    playing: bool,
    status: String,
    position_percent: f64,
    position_time: String,
    duration_time: String,
    volume: f64,
    can_play: bool,
    can_pause: bool,
    can_go_next: bool,
    can_go_previous: bool,
    can_seek: bool,
    shuffle: bool,
    loop_status: String,
    playback_rate: f64,
}

impl Default for EwwMusicState {
    fn default() -> Self {
        EwwMusicState {
            active_player: "No Player".to_string(),
            active_bus: String::new(),
            available_players: vec![],
            next_player: String::new(),
            prev_player: String::new(),
            title: "".to_string(),
            artist: "".to_string(),
            album: "".to_string(),
            art_url: "".to_string(),
            playing: false,
            status: "Stopped".to_string(),
            position_percent: 0.0,
            position_time: "0:00".to_string(),
            duration_time: "0:00".to_string(),
            volume: 0.0,
            can_play: false,
            can_pause: false,
            can_go_next: false,
            can_go_previous: false,
            can_seek: false,
            shuffle: false,
            loop_status: "None".to_string(),
            playback_rate: 1.0,
        }
    }
}

fn log_to_file(msg: &str) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open("/tmp/eww-volume-debug.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

fn format_time(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}

fn cache_album_art(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let cache_dir = dirs::cache_dir()?.join("eww").join("covers");
    fs::create_dir_all(&cache_dir).ok()?;

    if let Some(path) = url.strip_prefix("file://") {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
        return None;
    }

    let hash = format!("{:x}", md5::compute(url.as_bytes()));
    let cache_path = cache_dir.join(format!("{}.jpg", hash));

    if cache_path.exists() {
        return Some(cache_path.to_string_lossy().to_string());
    }

    if url.starts_with("http") {
        if let Ok(response) = reqwest::blocking::get(url) {
            if let Ok(bytes) = response.bytes() {
                if fs::write(&cache_path, &bytes).is_ok() {
                    return Some(cache_path.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

fn get_target_bus_name(finder: &PlayerFinder) -> Option<String> {
    if let Ok(bus_name) = fs::read_to_string(SWITCH_FILE) {
        let clean = bus_name.trim().to_string();
        if let Ok(players) = finder.find_all() {
            if players.iter().any(|p| p.bus_name() == clean) {
                return Some(clean);
            }
        }
    }

    if let Ok(players) = finder.find_all() {
        let mut sorted_players: Vec<_> = players.iter().collect();
        sorted_players.sort_by_key(|p| (p.identity().to_string(), p.bus_name().to_string()));

        for p in &sorted_players {
            if p.get_playback_status().ok() == Some(PlaybackStatus::Playing) {
                return Some(p.bus_name().to_string());
            }
        }
        if let Some(first) = sorted_players.first() {
            return Some(first.bus_name().to_string());
        }
    }
    None
}

struct PlayerDisplayInfo {
    bus_name: String,
    display_name: String,
}

fn collect_state(finder: &PlayerFinder) -> EwwMusicState {
    let players = match finder.find_all() {
        Ok(l) => l,
        Err(_) => return EwwMusicState::default(),
    };

    if players.is_empty() {
        return EwwMusicState::default();
    }

    let mut sorted_players: Vec<_> = players.iter().collect();
    sorted_players.sort_by_key(|p| (p.identity().to_string(), p.bus_name().to_string()));

    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in &sorted_players {
        *counts.entry(p.identity().to_string()).or_insert(0) += 1;
    }

    let mut player_list: Vec<PlayerDisplayInfo> = Vec::new();
    let mut current_counts: HashMap<String, usize> = HashMap::new();

    for p in &sorted_players {
        let id = p.identity().to_string();
        let total = *counts.get(&id).unwrap_or(&0);
        let display_name = if total > 1 {
            let c = current_counts.entry(id.clone()).or_insert(0);
            *c += 1;
            format!("{} ({})", id, c)
        } else {
            id
        };

        player_list.push(PlayerDisplayInfo {
            bus_name: p.bus_name().to_string(),
            display_name,
        });
    }

    let target_bus = get_target_bus_name(finder);
    let active_idx = match target_bus {
        Some(ref bus) => player_list
            .iter()
            .position(|p| &p.bus_name == bus)
            .unwrap_or(0),
        None => 0,
    };

    let active_info = &player_list[active_idx];
    let active_player_obj = sorted_players
        .iter()
        .find(|p| p.bus_name() == active_info.bus_name)
        .unwrap();

    let count = player_list.len();
    let next_idx = (active_idx + 1) % count;
    let prev_idx = (active_idx + count - 1) % count;

    let metadata = active_player_obj.get_metadata().ok();
    let title = metadata
        .as_ref()
        .and_then(|m| m.title())
        .unwrap_or("Unknown Title")
        .to_string();
    let artist = metadata
        .as_ref()
        .and_then(|m| m.artists())
        .map(|a| a.join(", "))
        .unwrap_or_default();
    let album = metadata
        .as_ref()
        .and_then(|m| m.album_name())
        .unwrap_or("")
        .to_string();
    let art_url = cache_album_art(metadata.as_ref().and_then(|m| m.art_url()).unwrap_or(""))
        .unwrap_or_default();

    let length_secs = metadata
        .as_ref()
        .and_then(|m| m.length())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let position_secs = active_player_obj
        .get_position()
        .ok()
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let position_percent = if length_secs > 0 {
        (position_secs as f64 * 100.0) / length_secs as f64
    } else {
        0.0
    };

    EwwMusicState {
        active_player: active_info.display_name.clone(),
        active_bus: active_info.bus_name.clone(),
        available_players: player_list.iter().map(|p| p.display_name.clone()).collect(),
        next_player: player_list[next_idx].display_name.clone(),
        prev_player: player_list[prev_idx].display_name.clone(),
        title,
        artist,
        album,
        art_url,
        playing: active_player_obj.get_playback_status().ok() == Some(PlaybackStatus::Playing),
        status: format!(
            "{:?}",
            active_player_obj
                .get_playback_status()
                .ok()
                .unwrap_or(PlaybackStatus::Stopped)
        ),
        position_percent,
        position_time: format_time(position_secs),
        duration_time: format_time(length_secs),
        volume: active_player_obj.get_volume().unwrap_or(0.0),
        can_play: active_player_obj.can_play().unwrap_or(false),
        can_pause: active_player_obj.can_pause().unwrap_or(false),
        can_go_next: active_player_obj.can_go_next().unwrap_or(false),
        can_go_previous: active_player_obj.can_go_previous().unwrap_or(false),
        can_seek: active_player_obj.can_seek().unwrap_or(false),
        shuffle: active_player_obj.get_shuffle().unwrap_or(false),
        loop_status: match active_player_obj
            .get_loop_status()
            .unwrap_or(LoopStatus::None)
        {
            LoopStatus::None => "None",
            LoopStatus::Track => "Track",
            LoopStatus::Playlist => "Playlist",
        }
        .to_string(),
        playback_rate: active_player_obj.get_playback_rate().unwrap_or(1.0),
    }
}

fn perform_action(command: Commands) {
    // Commands that don't require looking up the player first
    match command {
        Commands::Switch { bus_name } => {
            let _ = fs::write(SWITCH_FILE, bus_name);
            return;
        }
        Commands::Cycle { direction } => {
            let finder = PlayerFinder::new().expect("DBus error");
            let players = match finder.find_all() {
                Ok(l) => l,
                Err(_) => return,
            };
            if players.is_empty() {
                return;
            }

            // Sort exactly like collect_state
            let mut sorted_players: Vec<_> = players.iter().collect();
            sorted_players.sort_by_key(|p| (p.identity().to_string(), p.bus_name().to_string()));

            let bus_names: Vec<String> = sorted_players
                .iter()
                .map(|p| p.bus_name().to_string())
                .collect();

            let current_bus = get_target_bus_name(&finder).unwrap_or_else(|| bus_names[0].clone());
            let current_idx = bus_names
                .iter()
                .position(|b| b == &current_bus)
                .unwrap_or(0);

            let count = bus_names.len();
            let new_idx = if direction == "next" {
                (current_idx + 1) % count
            } else {
                (current_idx + count - 1) % count
            };

            let _ = fs::write(SWITCH_FILE, &bus_names[new_idx]);
            return;
        }
        Commands::Listen => unreachable!(),
        _ => {}
    }

    // Commands that act on the active player
    let finder = PlayerFinder::new().expect("DBus error");
    if let Some(bus_name) = get_target_bus_name(&finder) {
        if let Ok(players) = finder.find_all() {
            if let Some(player) = players.iter().find(|p| p.bus_name() == bus_name) {
                match command {
                    Commands::PlayPause => {
                        if player.get_playback_status().ok() == Some(PlaybackStatus::Playing) {
                            let _ = player.pause();
                        } else {
                            let _ = player.play();
                        }
                    }
                    Commands::Next => {
                        let _ = player.next();
                    }
                    Commands::Previous => {
                        let _ = player.previous();
                    }
                    Commands::Seek { percent } => {
                        if let Ok(metadata) = player.get_metadata() {
                            if let Some(len) = metadata.length() {
                                if let Some(track_id) = metadata.track_id() {
                                    let total = len.as_micros() as f64;
                                    let target = (total * (percent / 100.0)) as i64;
                                    let _ = player.set_position(
                                        track_id,
                                        &Duration::from_micros(target as u64),
                                    );
                                }
                            }
                        }
                    }
                    Commands::Volume { action } => {
                        log_to_file(&format!("Received input: '{}'", action));

                        let current = player.get_volume().unwrap_or(1.0);
                        let new_vol = match action.as_str() {
                            // Relative steps (from eventbox scroll)
                            "up" => (current + 0.05).min(1.0),
                            "down" => (current - 0.05).max(0.0),

                            // Numeric input (from scale or absolute value)
                            val => {
                                let num = val.parse::<f64>().unwrap_or(current);
                                if num > 1.0 {
                                    // Treat as 0-100 scale (e.g. "99" -> 0.99)
                                    (num / 100.0).clamp(0.0, 1.0)
                                } else if num < 0.0 {
                                    // Treat negative numbers as 0
                                    0.0
                                } else {
                                    // Treat as 0.0-1.0 scale
                                    num.clamp(0.0, 1.0)
                                }
                            }
                        };

                        log_to_file(&format!("Setting volume to: {}", new_vol));
                        let _ = player.set_volume(new_vol);
                    }
                    Commands::Shuffle => {
                        let c = player.get_shuffle().unwrap_or(false);
                        let _ = player.set_shuffle(!c);
                    }
                    Commands::Loop => {
                        let c = player.get_loop_status().unwrap_or(LoopStatus::None);
                        let n = match c {
                            LoopStatus::None => LoopStatus::Playlist,
                            LoopStatus::Playlist => LoopStatus::Track,
                            LoopStatus::Track => LoopStatus::None,
                        };
                        let _ = player.set_loop_status(n);
                    }
                    Commands::PlaybackRate { rate } => {
                        // Handle both percentage (25-400) and decimal (0.25-4.0) formats
                        let actual_rate = if rate > 4.0 {
                            // Treat as percentage (25-400 -> 0.25-4.0)
                            (rate / 100.0).clamp(0.25, 4.0)
                        } else {
                            // Treat as decimal (0.25-4.0)
                            rate.clamp(0.25, 4.0)
                        };
                        let _ = player.set_playback_rate(actual_rate);
                    }
                    _ => {}
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Listen => {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;
                let state = tokio::task::spawn_blocking(move || {
                    let f = PlayerFinder::new().unwrap();
                    collect_state(&f)
                })
                .await
                .unwrap_or_default();
                if let Ok(json) = serde_json::to_string(&state) {
                    println!("{}", json);
                    let _ = std::io::stdout().flush();
                }
            }
        }
        cmd => perform_action(cmd),
    }
}
