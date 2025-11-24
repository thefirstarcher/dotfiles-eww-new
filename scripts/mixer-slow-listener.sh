#!/usr/bin/env bash
# Slow-updating mixer state listener (device and application lists)
# Only updates when devices are added/removed or properties change

SLOW_FIFO="/tmp/eww-mixer-slow.fifo"

# Wait for FIFO to be created (daemon manager should be started by fast listener)
timeout=200  # 20 seconds (longer than fast listener to ensure daemon is up)
while [ ! -p "$SLOW_FIFO" ] && [ $timeout -gt 0 ]; do
    sleep 0.1
    timeout=$((timeout - 1))
done

if [ ! -p "$SLOW_FIFO" ]; then
    # Print default state but don't exit - keep trying to reconnect
    echo '{"sinks":[],"sink_inputs":[],"sources":[],"source_outputs":[]}'

    # Keep retrying in background
    while ! [ -p "$SLOW_FIFO" ]; do
        sleep 1
    done
fi

# Read from FIFO (will block until data is available)
cat "$SLOW_FIFO"
