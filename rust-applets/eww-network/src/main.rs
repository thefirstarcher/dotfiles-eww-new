use serde::Serialize;
use std::process::Command;

#[derive(Serialize)]
struct Network {
    #[serde(rename = "type")]
    net_type: String,
    icon: String,
    name: String,
    percent: u32,
}

fn get_network_status() -> Network {
    let output = Command::new("nmcli")
        .args(&["-t", "-f", "TYPE,STATE", "device"])
        .output();

    if let Ok(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);

        // Check for WiFi connection
        if text.contains("wifi:connected") {
            let ssid_output = Command::new("nmcli")
                .args(&["-t", "-f", "active,ssid", "dev", "wifi"])
                .output();

            if let Ok(ssid_out) = ssid_output {
                let ssid_text = String::from_utf8_lossy(&ssid_out.stdout);
                for line in ssid_text.lines() {
                    if line.starts_with("yes:") {
                        let ssid = line.strip_prefix("yes:").unwrap_or("WiFi").to_string();

                        // Get signal strength
                        let signal_output = Command::new("nmcli")
                            .args(&["-t", "-f", "active,signal", "dev", "wifi"])
                            .output();

                        let signal = if let Ok(sig_out) = signal_output {
                            let sig_text = String::from_utf8_lossy(&sig_out.stdout);
                            sig_text
                                .lines()
                                .find(|l| l.starts_with("yes:"))
                                .and_then(|l| l.strip_prefix("yes:"))
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(50)
                        } else {
                            50
                        };

                        let icon = if signal >= 80 {
                            "󰤨"
                        } else if signal >= 60 {
                            "󰤥"
                        } else if signal >= 40 {
                            "󰤢"
                        } else if signal >= 20 {
                            "󰤟"
                        } else {
                            "󰤯"
                        };

                        return Network {
                            net_type: "wifi".to_string(),
                            icon: icon.to_string(),
                            name: ssid,
                            percent: signal,
                        };
                    }
                }
            }
        }

        // Check for Ethernet connection
        if text.contains("ethernet:connected") {
            return Network {
                net_type: "ethernet".to_string(),
                icon: "󰈀".to_string(),
                name: "Ethernet".to_string(),
                percent: 100,
            };
        }
    }

    // Disconnected
    Network {
        net_type: "disconnected".to_string(),
        icon: "󰤭".to_string(),
        name: "Disconnected".to_string(),
        percent: 0,
    }
}

fn main() {
    let status = get_network_status();
    println!("{}", serde_json::to_string(&status).unwrap());
}
