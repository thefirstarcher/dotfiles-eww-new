use serde::Serialize;
use std::env;
use swayipc::{Connection, Event, EventType};

#[derive(Serialize)]
struct KeyboardLayout {
    layout: String,
    icon: String,
}

fn get_layout_code(layout_name: &str) -> String {
    let lower = layout_name.to_lowercase();

    // Try to extract language code (first 2-3 letters)
    if let Some(code) = lower.chars().take(3).collect::<String>().get(0..2) {
        if code.chars().all(|c| c.is_ascii_alphabetic()) {
            return code.to_string();
        }
    }

    // Fallback: check for common patterns
    if lower.contains("english") || lower.contains("us") {
        "us".to_string()
    } else if lower.contains("ukrain") {
        "uk".to_string()
    } else if lower.contains("russian") {
        "ru".to_string()
    } else if lower.contains("german") {
        "de".to_string()
    } else if lower.contains("french") {
        "fr".to_string()
    } else if lower.contains("spanish") {
        "es".to_string()
    } else if lower.contains("italian") {
        "it".to_string()
    } else if lower.contains("polish") {
        "pl".to_string()
    } else if lower.contains("portuguese") {
        "pt".to_string()
    } else if lower.contains("dutch") {
        "nl".to_string()
    } else if lower.contains("swedish") {
        "se".to_string()
    } else if lower.contains("norwegian") {
        "no".to_string()
    } else if lower.contains("danish") {
        "dk".to_string()
    } else if lower.contains("finnish") {
        "fi".to_string()
    } else if lower.contains("turkish") {
        "tr".to_string()
    } else if lower.contains("arabic") {
        "sa".to_string()
    } else if lower.contains("hebrew") {
        "il".to_string()
    } else if lower.contains("greek") {
        "gr".to_string()
    } else if lower.contains("japanese") {
        "jp".to_string()
    } else if lower.contains("korean") {
        "kr".to_string()
    } else if lower.contains("chinese") {
        "cn".to_string()
    } else if lower.contains("belarusian") || lower.contains("belarus") {
        "by".to_string()
    } else {
        "us".to_string()
    }
}

fn code_to_flag(mut code: String) -> String {
    // Special mappings
    code = match code.as_str() {
        "uk" => "ua".to_string(),
        "en" => "us".to_string(),
        "ar" => "sa".to_string(),
        "eng" => "us".to_string(),
        "rus" => "ru".to_string(),
        "he" => "il".to_string(),
        "el" => "gr".to_string(),
        "sv" => "se".to_string(),
        "da" => "dk".to_string(),
        "cs" => "cz".to_string(),
        "et" => "ee".to_string(),
        "sl" => "si".to_string(),
        "sr" => "rs".to_string(),
        "bs" => "ba".to_string(),
        "sq" => "al".to_string(),
        "vi" => "vn".to_string(),
        "hi" => "in".to_string(),
        "bn" => "bd".to_string(),
        "ta" => "lk".to_string(),
        "fa" => "ir".to_string(),
        "ur" => "pk".to_string(),
        "kk" => "kz".to_string(),
        "ky" => "kg".to_string(),
        "ka" => "ge".to_string(),
        "hy" => "am".to_string(),
        "be" => "by".to_string(),
        "af" => "za".to_string(),
        "my" => "mm".to_string(),
        "km" => "kh".to_string(),
        "lo" => "la".to_string(),
        "ne" => "np".to_string(),
        _ => code,
    };

    if code.len() == 2 {
        let upper = code.to_uppercase();
        let chars: Vec<char> = upper.chars().collect();

        let c1 = chars[0] as u32 - 'A' as u32 + 0x1F1E6;
        let c2 = chars[1] as u32 - 'A' as u32 + 0x1F1E6;

        format!(
            "{}{}",
            char::from_u32(c1).unwrap_or('?'),
            char::from_u32(c2).unwrap_or('?')
        )
    } else {
        format!("⌨️ {}", code)
    }
}

fn get_current_layout() -> KeyboardLayout {
    match Connection::new() {
        Ok(mut conn) => {
            if let Ok(inputs) = conn.get_inputs() {
                for input in inputs {
                    if input.input_type == "keyboard" {
                        if let Some(layout_name) = input.xkb_active_layout_name {
                            let code = get_layout_code(&layout_name);
                            let icon = code_to_flag(code.clone());
                            return KeyboardLayout { layout: code, icon };
                        }
                    }
                }
            }
        }
        Err(_) => {}
    }

    KeyboardLayout {
        layout: "us".to_string(),
        icon: code_to_flag("us".to_string()),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "listen" {
        // Listen mode: output current layout and monitor for changes
        let layout = get_current_layout();
        println!("{}", serde_json::to_string(&layout).unwrap());

        // Subscribe to input events
        if let Ok(conn) = Connection::new() {
            if let Ok(events) = conn.subscribe(&[EventType::Input]) {
                for event in events {
                    if let Ok(Event::Input(_)) = event {
                        let layout = get_current_layout();
                        println!("{}", serde_json::to_string(&layout).unwrap());
                    }
                }
            }
        }
    } else {
        // Default: just get current layout
        let layout = get_current_layout();
        println!("{}", serde_json::to_string(&layout).unwrap());
    }
}
