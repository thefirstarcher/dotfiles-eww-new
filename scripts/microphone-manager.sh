#!/bin/bash
# Wrapper for eww-microphone-mixer to auto-start daemon

BINARY="$HOME/.config/eww/rust-applets/eww-microphone-mixer/target/release/eww-microphone-mixer"
SOCKET="/tmp/eww-microphone-mixer.sock"

# Function to ensure daemon is running
ensure_daemon() {
    if [ ! -S "$SOCKET" ] || ! "$BINARY" ping &>/dev/null; then
        # Remove stale socket if it exists
        rm -f "$SOCKET"
        # Start daemon in background
        "$BINARY" daemon >/dev/null 2>&1 &
        # Wait for it to initialize
        sleep 0.2
    fi
}

# Main logic
ensure_daemon

# Pass all arguments to the rust binary
"$BINARY" "$@"

