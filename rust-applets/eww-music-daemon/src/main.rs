use clap::{Parser, Subcommand};
use mpris::{LoopStatus, PlaybackStatus, PlayerFinder};
use serde::Serialize;
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
    /// Start the listening daemon that outputs JSON
    Listen,
    /// Toggle play/pause
    PlayPause,
    /// Previous track
    Previous,
    /// Next track
    Next,
    /// Seek to a percentage (0-100)
    Seek { percent: f64 },
    /// Switch active player
    Switch { name: String },
    /// Set volume (0.0 - 1.0) or "up"/"down"
    Volume { action: String },
    /// Toggle Shuffle
    Shuffle,
    /// Toggle Loop
    Loop,
}

#[derive(Debug, Clone, Serialize)]
struct EwwMusicState {
    active_player: String,
    available_players: Vec<String>,
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
}

impl Default for EwwMusicState {
    fn default() -> Self {
        EwwMusicState {
            active_player: String::new(),
            available_players: vec![],
            title: "No Player".to_string(),
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
        }
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

fn get_active_player_name(finder: &PlayerFinder) -> Option<String> {
    // 1. Check manual override file
    if let Ok(name) = fs::read_to_string(SWITCH_FILE) {
        let clean = name.trim().to_string();
        // Verify it still exists
        if let Ok(players) = finder.find_all() {
            if players.iter().any(|p| p.identity() == clean) {
                return Some(clean);
            }
        }
    }

    // 2. Find playing player
    if let Ok(players) = finder.find_all() {
        for p in &players {
            if p.get_playback_status().ok() == Some(PlaybackStatus::Playing) {
                return Some(p.identity().to_string());
            }
        }
        // 3. Fallback to first
        if let Some(first) = players.first() {
            return Some(first.identity().to_string());
        }
    }
    None
}

fn collect_state(finder: &PlayerFinder) -> EwwMusicState {
    let players_list = match finder.find_all() {
        Ok(l) => l,
        Err(_) => return EwwMusicState::default(),
    };

    let available_players: Vec<String> = players_list
        .iter()
        .map(|p| p.identity().to_string())
        .collect();

    if available_players.is_empty() {
        return EwwMusicState::default();
    }

    let active_name = get_active_player_name(finder);

    let player = match active_name {
        Some(ref name) => players_list.into_iter().find(|p| p.identity() == name),
        None => None,
    };

    let player = match player {
        Some(p) => p,
        None => return EwwMusicState::default(),
    };

    let metadata = player.get_metadata().ok();
    let title = metadata
        .as_ref()
        .and_then(|m| m.title())
        .unwrap_or("Unknown Title")
        .to_string();
    let artist = metadata
        .as_ref()
        .and_then(|m| m.artists())
        .map(|a| a.join(", "))
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = metadata
        .as_ref()
        .and_then(|m| m.album_name())
        .unwrap_or("")
        .to_string();

    let art_url_raw = metadata.as_ref().and_then(|m| m.art_url()).unwrap_or("");
    let art_url = cache_album_art(art_url_raw).unwrap_or_default();

    let length_dur = metadata.as_ref().and_then(|m| m.length());
    let length_secs = length_dur.map(|d| d.as_secs()).unwrap_or(0);

    let position_dur = player.get_position().ok().unwrap_or(Duration::from_secs(0));
    let position_secs = position_dur.as_secs();

    let status = player
        .get_playback_status()
        .ok()
        .unwrap_or(PlaybackStatus::Stopped);
    let playing = status == PlaybackStatus::Playing;

    // Jitter Fix: Cast to u64 seconds removes sub-second flicker when paused
    let position_percent = if length_secs > 0 {
        (position_secs as f64 * 100.0) / length_secs as f64
    } else {
        0.0
    };

    let volume = player.get_volume().unwrap_or(0.0);

    // In mpris crate, get_shuffle returns Result<bool>
    let shuffle = player.get_shuffle().unwrap_or(false);

    let loop_status = match player.get_loop_status().unwrap_or(LoopStatus::None) {
        LoopStatus::None => "None",
        LoopStatus::Track => "Track",
        LoopStatus::Playlist => "Playlist",
    }
    .to_string();

    EwwMusicState {
        active_player: player.identity().to_string(),
        available_players,
        title,
        artist,
        album,
        art_url,
        playing,
        status: format!("{:?}", status),
        position_percent,
        position_time: format_time(position_secs),
        duration_time: format_time(length_secs),
        volume,
        can_play: player.can_play().unwrap_or(false),
        can_pause: player.can_pause().unwrap_or(false),
        can_go_next: player.can_go_next().unwrap_or(false),
        can_go_previous: player.can_go_previous().unwrap_or(false),
        can_seek: player.can_seek().unwrap_or(false),
        shuffle,
        loop_status,
    }
}

fn perform_action(command: Commands) {
    // Handle Switch separately because it doesn't need the DBus connection logic immediately
    // and we want to avoid move errors.
    match command {
        Commands::Switch { name } => {
            let _ = fs::write(SWITCH_FILE, name);
        }
        // All other commands require finding the active player
        other_cmd => {
            let finder = PlayerFinder::new().expect("Failed to connect to DBus");
            let active_name = get_active_player_name(&finder);

            if let Some(name) = active_name {
                if let Ok(player) = finder.find_by_name(&name) {
                    match other_cmd {
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
                                    // We must have a track_id to seek safely using set_position
                                    if let Some(track_id) = metadata.track_id() {
                                        let total_micros = len.as_micros() as f64;
                                        let target = (total_micros * (percent / 100.0)) as i64;
                                        let _ = player.set_position(
                                            track_id,
                                            &Duration::from_micros(target as u64),
                                        );
                                    }
                                }
                            }
                        }
                        Commands::Volume { action } => {
                            let current = player.get_volume().unwrap_or(1.0);
                            let new_vol = match action.as_str() {
                                "up" => (current + 0.05).min(1.0),
                                "down" => (current - 0.05).max(0.0),
                                val => val.parse::<f64>().unwrap_or(current).clamp(0.0, 1.0),
                            };
                            let _ = player.set_volume(new_vol);
                        }
                        Commands::Shuffle => {
                            let current = player.get_shuffle().unwrap_or(false);
                            let _ = player.set_shuffle(!current);
                        }
                        Commands::Loop => {
                            let current = player.get_loop_status().unwrap_or(LoopStatus::None);
                            let next = match current {
                                LoopStatus::None => LoopStatus::Playlist,
                                LoopStatus::Playlist => LoopStatus::Track,
                                LoopStatus::Track => LoopStatus::None,
                            };
                            let _ = player.set_loop_status(next);
                        }
                        // Switch is handled in the outer match
                        Commands::Switch { .. } => unreachable!(),
                        Commands::Listen => unreachable!(),
                    }
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
            // 500ms poll is a good balance between CPU usage and UI responsiveness
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
        cmd => {
            perform_action(cmd);
        }
    }
}
