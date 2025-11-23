use serde::Serialize;
use zbus::{Connection, Result, zvariant::{OwnedObjectPath, OwnedValue}};
use std::collections::HashMap;

#[derive(Serialize)]
struct Bluetooth {
    enabled: bool,
    connected: bool,
    device: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let status = get_bluetooth_status().await.unwrap_or(Bluetooth {
        enabled: false,
        connected: false,
        device: String::new(),
    });
    println!("{}", serde_json::to_string(&status).unwrap());
    Ok(())
}

async fn get_bluetooth_status() -> Result<Bluetooth> {
    let connection = Connection::system().await?;

    // BlueZ D-Bus API
    // Service: org.bluez
    // Adapter path: /org/bluez/hci0 (or similar)

    // Get adapter
    let proxy = zbus::Proxy::new(
        &connection,
        "org.bluez",
        "/org/bluez/hci0",
        "org.freedesktop.DBus.Properties",
    )
    .await?;

    // Check if adapter is powered
    let powered: bool = match proxy
        .call_method(
            "Get",
            &("org.bluez.Adapter1", "Powered"),
        )
        .await
    {
        Ok(response) => {
            // The response is a variant containing the actual value
            response.body().deserialize::<(bool,)>()
                .ok()
                .map(|(v,)| v)
                .unwrap_or(false)
        }
        Err(_) => false,
    };

    if !powered {
        return Ok(Bluetooth {
            enabled: false,
            connected: false,
            device: String::new(),
        });
    }

    // Get managed objects to find connected devices
    let object_manager = zbus::Proxy::new(
        &connection,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )
    .await?;

    let response = object_manager
        .call_method("GetManagedObjects", &())
        .await
        .ok();

    let objects: HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> =
        if let Some(resp) = response.as_ref() {
            resp.body().deserialize().unwrap_or_default()
        } else {
            HashMap::new()
        };

    // Find first connected device
    for (path, interfaces) in objects {
        if let Some(device_props) = interfaces.get("org.bluez.Device1") {
            if let Some(connected) = device_props.get("Connected") {
                if let Ok(true) = connected.downcast_ref::<bool>() {
                    // Get device name/alias
                    let device_name = device_props
                        .get("Alias")
                        .or_else(|| device_props.get("Name"))
                        .and_then(|v| v.downcast_ref::<String>().ok())
                        .unwrap_or_else(|| {
                            path.to_string()
                                .split('/')
                                .last()
                                .unwrap_or("Unknown")
                                .to_string()
                        });

                    return Ok(Bluetooth {
                        enabled: true,
                        connected: true,
                        device: device_name,
                    });
                }
            }
        }
    }

    // Powered but no connected devices
    Ok(Bluetooth {
        enabled: true,
        connected: false,
        device: String::new(),
    })
}
