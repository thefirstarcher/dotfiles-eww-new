#!/bin/bash
# Auto-start volume daemon and listen for updates

BINARY="$HOME/.config/eww/rust-applets/eww-volume-mixer/target/release/eww-volume-mixer"
SOCKET="/tmp/eww-volume-mixer.sock"

# Function to ensure daemon is running
ensure_daemon() {
    # Check if socket exists and daemon responds
    if [ -S "$SOCKET" ]; then
        if timeout 0.5 "$BINARY" ping &>/dev/null; then
            return 0
        fi
    fi

    # Remove stale socket
    rm -f "$SOCKET"

    # Start daemon in background with error suppression
    nohup "$BINARY" daemon >/dev/null 2>&1 &

    # Wait for daemon to start (max 2 seconds)
    for i in {1..20}; do
        if [ -S "$SOCKET" ]; then
            if timeout 0.5 "$BINARY" ping &>/dev/null; then
                # Give it a moment to fully initialize
                sleep 0.1
                return 0
            fi
        fi
        sleep 0.1
    done

    return 1
}

# Try to start daemon
if ! ensure_daemon; then
    # Daemon failed to start - output default state and keep trying
    echo '{"percent": 50, "muted": false, "level": 0}'

    # Try to diagnose the issue
    if [ ! -x "$BINARY" ]; then
        >&2 echo "ERROR: Binary not executable: $BINARY"
    elif ! ldd "$BINARY" &>/dev/null; then
        >&2 echo "ERROR: Binary missing dependencies"
    fi

    # Keep outputting default state
    while true; do
        sleep 1
        echo '{"percent": 50, "muted": false, "level": 0}'
    done
fi

# Daemon is running - start listening
# Use stdbuf to ensure line buffering for immediate output
exec stdbuf -oL "$BINARY" listen 2>/dev/null || {
    # If listen fails, output default state
    while true; do
        echo '{"percent": 50, "muted": false, "level": 0}'
        sleep 1
    done
}
