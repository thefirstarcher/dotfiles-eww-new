use reqwest::blocking::Client;
use serde::Serialize;

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

fn fetch_weather() -> Weather {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    match client.get("https://wttr.in/?format=%t|%C|%c").send() {
        Ok(response) => {
            if let Ok(data) = response.text() {
                let parts: Vec<&str> = data.split('|').collect();

                if parts.len() >= 3 {
                    let temp = parts[0].trim().to_string();
                    let condition = parts[1].trim().to_string();
                    let icon_raw = parts[2].trim();
                    let icon = get_icon_from_condition(&condition, icon_raw);

                    return Weather { temp, condition, icon };
                }
            }
        }
        Err(_) => {}
    }

    Weather {
        temp: "".to_string(),
        condition: "No data".to_string(),
        icon: "󰖐".to_string(),
    }
}

fn main() {
    let weather = fetch_weather();
    println!("{}", serde_json::to_string(&weather).unwrap());
}
