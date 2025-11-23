#!/bin/bash
# Auto-start microphone daemon and listen for updates

BINARY="$HOME/.config/eww/rust-applets/eww-microphone-mixer/target/release/eww-microphone-mixer"
SOCKET="/tmp/eww-microphone-mixer.sock"

# Function to ensure daemon is running
ensure_daemon() {
    # Check if socket exists and daemon responds
    if [ -S "$SOCKET" ] && "$BINARY" ping &>/dev/null; then
        return 0
    fi

    # Remove stale socket
    rm -f "$SOCKET"

    # Start daemon in background
    "$BINARY" daemon >/dev/null 2>&1 &
    local daemon_pid=$!

    # Wait for daemon to start (max 3 seconds)
    for i in {1..30}; do
        if [ -S "$SOCKET" ] && "$BINARY" ping &>/dev/null; then
            return 0
        fi
        sleep 0.1
    done

    # Failed to start
    echo '{"percent": 0, "muted": false, "level": 0}' >&2
    return 1
}

# Output initial state immediately (prevents null errors)
echo '{"percent": 0, "muted": false, "level": 0}'

# Ensure daemon is running
if ensure_daemon; then
    # Start listening (this will block and stream updates)
    exec "$BINARY" listen 2>/dev/null
else
    # Daemon failed to start, output default state repeatedly
    while true; do
        echo '{"percent": 0, "muted": false, "level": 0}'
        sleep 1
    done
fi
