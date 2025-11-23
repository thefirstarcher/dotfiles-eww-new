use serde::Serialize;
use zbus::{Connection, Result};

#[derive(Serialize)]
struct Notifications {
    count: u32,
    dnd: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let notifications = get_notifications().await.unwrap_or(Notifications {
        count: 0,
        dnd: false,
    });
    println!("{}", serde_json::to_string(&notifications).unwrap());
    Ok(())
}

async fn get_notifications() -> Result<Notifications> {
    // Connect to session bus
    let connection = Connection::session().await?;

    // swaync D-Bus interface: org.freedesktop.Notifications
    // Service name: org.freedesktop.Notifications
    // Path: /org/freedesktop/Notifications

    // Try to get notification count
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await?;

    // Get swaync-specific properties
    // Count: cc.cccounts.swaync
    let count: u32 = proxy
        .get_property("cc.cccounts.swaync")
        .await
        .unwrap_or(0);

    // DND status
    let dnd: bool = proxy
        .get_property("cc.cccounts.swaync.dnd")
        .await
        .unwrap_or(false);

    Ok(Notifications { count, dnd })
}
