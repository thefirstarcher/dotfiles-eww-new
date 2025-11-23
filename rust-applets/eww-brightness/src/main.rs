use inotify::{Inotify, WatchMask};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize)]
struct Brightness {
    percent: u32,
}

fn find_brightness_path() -> Option<PathBuf> {
    let backlight_dir = Path::new("/sys/class/backlight");
    if let Ok(entries) = fs::read_dir(backlight_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("brightness").exists() {
                return Some(path);
            }
        }
    }
    None
}

fn get_brightness() -> u32 {
    if let Some(path) = find_brightness_path() {
        let max_path = path.join("max_brightness");
        let cur_path = path.join("brightness");

        if let (Ok(max_str), Ok(cur_str)) = (fs::read_to_string(&max_path), fs::read_to_string(&cur_path)) {
            if let (Ok(max), Ok(cur)) = (max_str.trim().parse::<f64>(), cur_str.trim().parse::<f64>()) {
                if max > 0.0 {
                    let percent = ((cur / max) * 100.0).round() as u32;
                    return percent.max(1);
                }
            }
        }
    }
    1
}

fn set_brightness_up() {
    let _ = Command::new("brightnessctl")
        .arg("set")
        .arg("+1%")
        .output();
}

fn set_brightness_down() {
    let current = get_brightness();
    if current > 1 {
        let _ = Command::new("brightnessctl")
            .arg("set")
            .arg("1%-")
            .output();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("listen") => {
            // Output current brightness
            let brightness = Brightness { percent: get_brightness() };
            println!("{}", serde_json::to_string(&brightness).unwrap());

            // Setup inotify to watch brightness file
            if let Some(path) = find_brightness_path() {
                let brightness_file = path.join("brightness");

                if let Ok(mut inotify) = Inotify::init() {
                    if inotify.watches().add(&brightness_file, WatchMask::MODIFY).is_ok() {
                        let mut buffer = [0; 1024];
                        loop {
                            if inotify.read_events_blocking(&mut buffer).is_ok() {
                                let brightness = Brightness { percent: get_brightness() };
                                println!("{}", serde_json::to_string(&brightness).unwrap());
                            }
                        }
                    }
                }
            }
        }
        Some("up") => {
            set_brightness_up();
        }
        Some("down") => {
            set_brightness_down();
        }
        _ => {
            let brightness = Brightness { percent: get_brightness() };
            println!("{}", serde_json::to_string(&brightness).unwrap());
        }
    }
}
