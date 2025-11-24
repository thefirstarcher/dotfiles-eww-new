use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
struct Updates {
    official: u32,
    aur: u32,
    total: u32,
    icon: String,
}

fn count_official_updates() -> u32 {
    // Try checkupdates first (from pacman-contrib)
    if let Ok(output) = Command::new("checkupdates").output() {
        return output.stdout.iter().filter(|&&c| c == b'\n').count() as u32;
    }

    // Fallback to pacman
    if let Ok(output) = Command::new("pacman").args(&["-Qu"]).output() {
        return output.stdout.iter().filter(|&&c| c == b'\n').count() as u32;
    }

    0
}

fn count_aur_updates() -> u32 {
    // Try paru first
    if let Ok(output) = Command::new("paru").args(&["-Qua"]).output() {
        return output.stdout.iter().filter(|&&c| c == b'\n').count() as u32;
    }

    // Fallback to yay
    if let Ok(output) = Command::new("yay").args(&["-Qua"]).output() {
        return output.stdout.iter().filter(|&&c| c == b'\n').count() as u32;
    }

    0
}

fn get_updates() -> Updates {
    let official = count_official_updates();
    let aur = count_aur_updates();
    let total = official + aur;

    let icon = "".to_string(); //

    Updates {
        official,
        aur,
        total,
        icon,
    }
}

fn main() {
    let updates = get_updates();
    println!("{}", serde_json::to_string(&updates).unwrap());
}
