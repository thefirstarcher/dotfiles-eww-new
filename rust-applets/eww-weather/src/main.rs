use reqwest::blocking::Client;
use serde::Serialize;
use std::thread;
use std::time::Duration;

#[derive(Serialize)]
struct Weather {
    temp: String,
    condition: String,
    icon: String,
}

fn get_icon_from_condition(condition: &str, icon_raw: &str) -> String {
    let lower = condition.to_lowercase();
    let icon_lower = icon_raw.to_lowercase();

    if lower.contains("sunny") || lower.contains("clear") || icon_lower.contains("☀") {
        "󰖙".to_string()
    } else if lower.contains("partly") || lower.contains("cloudy") || icon_lower.contains("⛅") {
        "󰖕".to_string()
    } else if lower.contains("overcast") || icon_lower.contains("☁") {
        "󰖐".to_string()
    } else if lower.contains("rain") || lower.contains("drizzle") || icon_lower.contains("🌧") {
        "󰖗".to_string()
    } else if lower.contains("thunder") || lower.contains("storm") || icon_lower.contains("⛈") {
        "󰙾".to_string()
    } else if lower.contains("snow") || icon_lower.contains("🌨") {
        "󰖘".to_string()
    } else if lower.contains("fog") || lower.contains("mist") || icon_lower.contains("🌫") {
        "󰖑".to_string()
    } else {
        "󰖐".to_string()
    }
}

/// Check if internet is available by pinging a reliable host
fn check_internet() -> bool {
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    // Try multiple reliable hosts
    let hosts = vec![
        "https://www.google.com",
        "https://1.1.1.1",
        "https://8.8.8.8",
    ];

    for host in hosts {
        if client.head(host).send().is_ok() {
            return true;
        }
    }
    false
}

fn fetch_weather_with_retry() -> Weather {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let max_retries = 10;
    let mut retry_delay = Duration::from_secs(1);

    for attempt in 0..max_retries {
        match client.get("https://wttr.in/?format=%t|%C|%c").send() {
            Ok(response) => {
                if let Ok(data) = response.text() {
                    let parts: Vec<&str> = data.split('|').collect();

                    if parts.len() >= 3 {
                        let temp = parts[0].trim().to_string();
                        let condition = parts[1].trim().to_string();
                        let icon_raw = parts[2].trim();
                        let icon = get_icon_from_condition(&condition, icon_raw);

                        return Weather {
                            temp,
                            condition,
                            icon,
                        };
                    }
                }
            }
            Err(e) => {
                eprintln!("Weather fetch attempt {} failed: {}", attempt + 1, e);

                // Only retry if we have internet connectivity and attempts remain
                if attempt < max_retries - 1 && check_internet() {
                    eprintln!("Internet is available, retrying after {:?}...", retry_delay);
                    thread::sleep(retry_delay);
                    retry_delay *= 2; // Exponential backoff
                } else if !check_internet() {
                    eprintln!("No internet connection detected");
                    break;
                }
            }
        }
    }

    Weather {
        temp: "".to_string(),
        condition: "No data".to_string(),
        icon: "󰖐".to_string(),
    }
}

fn main() {
    let weather = fetch_weather_with_retry();
    println!("{}", serde_json::to_string(&weather).unwrap());
}
