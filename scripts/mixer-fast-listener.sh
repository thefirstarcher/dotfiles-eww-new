#!/usr/bin/env bash
# Fast-updating mixer state listener (volume levels, mute status, counts)
# Updates frequently for smooth visualizations

FAST_FIFO="/tmp/eww-mixer-fast.fifo"
DAEMON_MANAGER="$HOME/.config/eww/scripts/mixer-daemon-manager.sh"

# Start daemon manager if not running
if ! pgrep -f "mixer-daemon-manager.sh" > /dev/null; then
    "$DAEMON_MANAGER" &
fi

# Wait for FIFO to be created (increased timeout for startup)
timeout=150  # 15 seconds
while [ ! -p "$FAST_FIFO" ] && [ $timeout -gt 0 ]; do
    sleep 0.1
    timeout=$((timeout - 1))
done

if [ ! -p "$FAST_FIFO" ]; then
    # Print default state but don't exit - keep trying to reconnect
    echo '{"volume_percent":0,"volume_muted":false,"volume_level":0,"mic_percent":0,"mic_muted":false,"mic_level":0,"sink_count":0,"sink_input_count":0,"source_count":0,"source_output_count":0}'

    # Keep retrying in background
    while ! [ -p "$FAST_FIFO" ]; do
        sleep 1
    done
fi

# Read from FIFO (will block until data is available)
cat "$FAST_FIFO"
