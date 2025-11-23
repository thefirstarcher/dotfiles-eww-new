use serde::{Deserialize, Serialize};
use swayipc::{Connection, Event, EventType, Node};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowTitle {
    title: String,
}

fn find_focused_node(node: &Node) -> Option<String> {
    if node.focused {
        return node.name.clone();
    }

    for child in &node.nodes {
        if let Some(title) = find_focused_node(child) {
            return Some(title);
        }
    }

    for child in &node.floating_nodes {
        if let Some(title) = find_focused_node(child) {
            return Some(title);
        }
    }

    None
}

fn get_window_title() -> Result<String, Box<dyn std::error::Error>> {
    let mut conn = Connection::new()?;
    let tree = conn.get_tree()?;

    let title = find_focused_node(&tree).unwrap_or_default();

    // Truncate if too long (max 80 characters)
    let truncated = if title.len() > 80 {
        format!("{}...", &title[..77])
    } else {
        title
    };

    Ok(truncated)
}

fn output_title() {
    if let Ok(title) = get_window_title() {
        let info = WindowTitle { title };
        if let Ok(json) = serde_json::to_string(&info) {
            println!("{}", json);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let listen_mode = args.len() > 1 && args[1] == "listen";

    if listen_mode {
        // Output initial title
        output_title();

        // Subscribe to window events
        let events = Connection::new()?.subscribe(&[EventType::Window])?;

        // Listen for events
        for event in events {
            match event {
                Ok(Event::Window(_)) => {
                    // Small delay to let window state stabilize
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    output_title();
                }
                Err(e) => eprintln!("Error: {}", e),
                _ => {}
            }
        }
    } else {
        // One-shot mode
        output_title();
    }

    Ok(())
}
