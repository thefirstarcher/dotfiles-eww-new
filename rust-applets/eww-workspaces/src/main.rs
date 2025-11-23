use serde::{Deserialize, Serialize};
use swayipc::{Connection, EventType};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceInfo {
    num: i32,
    name: String,
    visible: bool,
    focused: bool,
    urgent: bool,
    output: String,
}

fn get_workspaces() -> Result<Vec<WorkspaceInfo>, Box<dyn std::error::Error>> {
    let mut conn = Connection::new()?;
    let workspaces = conn.get_workspaces()?;

    Ok(workspaces
        .iter()
        .map(|ws| WorkspaceInfo {
            num: ws.num,
            name: ws.name.clone(),
            visible: ws.visible,
            focused: ws.focused,
            urgent: ws.urgent,
            output: ws.output.clone(),
        })
        .collect())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Output initial state
    let workspaces = get_workspaces()?;
    println!("{}", serde_json::to_string(&workspaces)?);

    // Subscribe to workspace events
    let events = Connection::new()?
        .subscribe(&[EventType::Workspace])?;

    // Listen for events
    for event in events {
        match event {
            Ok(_) => {
                let workspaces = get_workspaces()?;
                println!("{}", serde_json::to_string(&workspaces)?);
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}
