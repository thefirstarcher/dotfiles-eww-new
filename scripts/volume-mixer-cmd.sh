#!/bin/bash
# Helper script to send commands to the volume mixer daemon via named pipe

PIPE="/tmp/eww-volume-mixer.pipe"
DAEMON_BIN="$HOME/.config/eww/rust-applets/eww-volume-mixer/target/release/eww-volume-mixer"

# Create pipe if it doesn't exist
if [ ! -p "$PIPE" ]; then
    mkfifo "$PIPE"
fi

# Check if daemon is running, start it if not
if ! pgrep -f "eww-volume-mixer daemon" > /dev/null; then
    # Start daemon in background, reading from the pipe
    nohup "$DAEMON_BIN" daemon < "$PIPE" > /dev/null 2>&1 &
    sleep 0.1  # Give daemon time to start
fi

# Send command to daemon via pipe
echo "$*" >> "$PIPE"
