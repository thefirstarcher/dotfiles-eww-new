#!/usr/bin/env bash
# EWW startup script - ensures all daemons are ready before opening windows

MIXER_DAEMON_MANAGER="$HOME/.config/eww/scripts/mixer-daemon-manager.sh"
FAST_FIFO="/tmp/eww-mixer-fast.fifo"
SLOW_FIFO="/tmp/eww-mixer-slow.fifo"

# Kill existing EWW and mixer processes
pkill eww 2>/dev/null
pkill -f "mixer-daemon-manager.sh" 2>/dev/null
pkill -f "eww-mixer listen" 2>/dev/null

# Clean up old FIFOs and sockets
rm -f /tmp/eww-mixer*.fifo /tmp/eww-mixer*.sock /tmp/eww-mixer-daemon.lock

# Wait a moment for cleanup
sleep 0.5

# Start mixer daemon manager in background
echo "Starting mixer daemon..."
"$MIXER_DAEMON_MANAGER" &

# Wait for FIFOs to be created (max 10 seconds)
echo "Waiting for mixer FIFOs..."
timeout=100
while [ ! -p "$FAST_FIFO" ] || [ ! -p "$SLOW_FIFO" ]; do
    if [ $timeout -le 0 ]; then
        echo "ERROR: Mixer daemon failed to start properly"
        exit 1
    fi
    sleep 0.1
    timeout=$((timeout - 1))
done

echo "Mixer daemon ready!"

# Start EWW daemon
echo "Starting EWW daemon..."
eww daemon

# Wait for daemon to be ready
sleep 1

# Open bar
echo "Opening EWW bar..."
eww open bar

echo "EWW startup complete!"
