use serde::Serialize;
use std::env;

#[derive(Serialize)]
struct UserInfo {
    username: String,
    uptime_days: String,
    uptime_hours: String,
    uptime_minutes: String,
}

fn main() {
    let username = env::var("USER").unwrap_or_else(|_| "unknown".to_string());

    // Read uptime from /proc/uptime (first field is uptime in seconds)
    let uptime_seconds = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|content| {
            content
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0) as u64;

    let days = uptime_seconds / 86400;
    let hours = (uptime_seconds % 86400) / 3600;
    let minutes = (uptime_seconds % 3600) / 60;

    let info = UserInfo {
        username,
        uptime_days: format!("{} days", days),
        uptime_hours: format!("{} hours", hours),
        uptime_minutes: format!("{} minutes", minutes),
    };

    println!("{}", serde_json::to_string(&info).unwrap());
}
