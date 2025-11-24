#!/usr/bin/env bash
# Mixer daemon manager - starts the daemon and manages output streams

MIXER_BIN="$HOME/.config/eww/rust-applets/eww-mixer/target/release/eww-mixer"
SOCKET_PATH="/tmp/eww-mixer.sock"
FAST_FIFO="/tmp/eww-mixer-fast.fifo"
SLOW_FIFO="/tmp/eww-mixer-slow.fifo"
LOCK_FILE="/tmp/eww-mixer-daemon.lock"

# Try to acquire lock
exec 200>"$LOCK_FILE"
flock -n 200 || exit 0

# Kill existing daemon if running
if [ -S "$SOCKET_PATH" ]; then
    "$MIXER_BIN" kill 2>/dev/null || true
    sleep 0.1
fi

# Clean up old FIFOs
rm -f "$FAST_FIFO" "$SLOW_FIFO"

# Create FIFOs
mkfifo "$FAST_FIFO"
mkfifo "$SLOW_FIFO"

# Function to cleanup on exit
cleanup() {
    "$MIXER_BIN" kill 2>/dev/null || true
    rm -f "$FAST_FIFO" "$SLOW_FIFO" "$LOCK_FILE"
    exit 0
}

trap cleanup SIGTERM SIGINT EXIT

# Start the daemon and split output to FIFOs using tee and grep
"$MIXER_BIN" listen 2>/dev/null | tee >(grep --line-buffered "^FAST:" | sed -u 's/^FAST://' > "$FAST_FIFO") \
    >(grep --line-buffered "^SLOW:" | sed -u 's/^SLOW://' > "$SLOW_FIFO") \
    > /dev/null

wait
