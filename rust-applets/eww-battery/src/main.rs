use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BatteryInfo {
    present: bool,
    percent: u32,
    status: String,
    time: String,
    power: f64,
    icon: String,
    health: u32,
    cycles: u32,
    voltage: String,
    temp: i32,
    design_capacity: String,
    current_capacity: String,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            present: false,
            percent: 0,
            status: "N/A".to_string(),
            time: String::new(),
            power: 0.0,
            icon: "".to_string(),
            health: 0,
            cycles: 0,
            voltage: "0.0V".to_string(),
            temp: 0,
            design_capacity: "N/A".to_string(),
            current_capacity: "N/A".to_string(),
        }
    }
}

fn find_battery() -> Option<PathBuf> {
    for bat in &["BAT0", "BAT1", "BAT2"] {
        let path = PathBuf::from(format!("/sys/class/power_supply/{}", bat));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn read_file_u64(path: &PathBuf, file: &str) -> u64 {
    fs::read_to_string(path.join(file))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn read_file_string(path: &PathBuf, file: &str) -> String {
    fs::read_to_string(path.join(file))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn get_battery_icon(status: &str, percent: u32) -> &'static str {
    if status == "Charging" {
        "󰂄"
    } else if status == "Full" {
        "󰁹"
    } else if percent >= 90 {
        "󰁹"
    } else if percent >= 70 {
        "󰂀"
    } else if percent >= 50 {
        "󰁾"
    } else if percent >= 30 {
        "󰁼"
    } else if percent >= 10 {
        "󰁺"
    } else {
        "󰂎"
    }
}

fn get_battery_info() -> BatteryInfo {
    let Some(battery_path) = find_battery() else {
        return BatteryInfo::default();
    };

    let percent = read_file_u64(&battery_path, "capacity") as u32;
    let status = read_file_string(&battery_path, "status");
    let power_now = read_file_u64(&battery_path, "power_now");
    let power = power_now as f64 / 1_000_000.0;

    let voltage_now = read_file_u64(&battery_path, "voltage_now");
    let voltage = format!("{:.1}V", voltage_now as f64 / 1_000_000.0);

    let temp_raw = read_file_u64(&battery_path, "temp") as i64;
    let temp = (temp_raw / 10) as i32;

    let cycles = read_file_u64(&battery_path, "cycle_count") as u32;

    // Try charge_full_design first, then energy_full_design
    let design_capacity_raw = {
        let charge = read_file_u64(&battery_path, "charge_full_design");
        if charge > 0 {
            charge
        } else {
            read_file_u64(&battery_path, "energy_full_design")
        }
    };

    let current_capacity_raw = {
        let charge = read_file_u64(&battery_path, "charge_full");
        if charge > 0 {
            charge
        } else {
            read_file_u64(&battery_path, "energy_full")
        }
    };

    let design_capacity = format!("{} mAh", design_capacity_raw / 1000);
    let current_capacity = format!("{} mAh", current_capacity_raw / 1000);

    let health = if design_capacity_raw > 0 {
        ((current_capacity_raw as f64 / design_capacity_raw as f64) * 100.0) as u32
    } else {
        0
    };

    // Calculate time remaining/to full
    let current_now = {
        let current = read_file_u64(&battery_path, "current_now");
        if current > 0 {
            current
        } else {
            read_file_u64(&battery_path, "power_now")
        }
    };

    let time = if current_now > 0 {
        if status == "Discharging" {
            let hours = current_capacity_raw / current_now;
            let minutes = ((current_capacity_raw as f64 / current_now as f64) - hours as f64) * 60.0;
            format!("{}h {}m", hours, minutes as u32)
        } else if status == "Charging" {
            let remaining = design_capacity_raw.saturating_sub(current_capacity_raw);
            let hours = remaining / current_now;
            let minutes = ((remaining as f64 / current_now as f64) - hours as f64) * 60.0;
            format!("{}h {}m", hours, minutes as u32)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let icon = get_battery_icon(&status, percent).to_string();

    BatteryInfo {
        present: true,
        percent,
        status,
        time,
        power,
        icon,
        health,
        cycles,
        voltage,
        temp,
        design_capacity,
        current_capacity,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let listen_mode = args.len() > 1 && args[1] == "listen";

    if listen_mode {
        // Listen mode: poll every 2 seconds
        loop {
            let info = get_battery_info();
            println!("{}", serde_json::to_string(&info).unwrap());
            thread::sleep(Duration::from_secs(2));
        }
    } else {
        // One-shot mode
        let info = get_battery_info();
        println!("{}", serde_json::to_string(&info).unwrap());
    }
}
