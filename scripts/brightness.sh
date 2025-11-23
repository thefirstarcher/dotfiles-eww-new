#!/bin/bash
# ============================================================================
# Brightness Control Script for EWW
# ============================================================================
# Returns JSON with brightness percentage and handles up/down commands

get_brightness() {
  # Try brightnessctl first, then light, then sys interface
  if command -v brightnessctl &> /dev/null; then
    brightnessctl get | awk -v max="$(brightnessctl max)" '{printf "%.0f", ($1/max)*100}'
  elif command -v light &> /dev/null; then
    light -G | awk '{printf "%.0f", $1}'
  else
    # Fallback to sys interface
    max=$(cat /sys/class/backlight/*/max_brightness 2>/dev/null | head -1)
    current=$(cat /sys/class/backlight/*/brightness 2>/dev/null | head -1)
    if [ -n "$max" ] && [ "$max" -gt 0 ]; then
      awk "BEGIN {printf \"%.0f\", ($current/$max)*100}"
    else
      echo "0"
    fi
  fi
}

case "$1" in
  "listen")
    # Listen mode: output current brightness and monitor for changes
    percent=$(get_brightness)
    if [ "$percent" -lt 1 ]; then
      percent=1
    fi
    echo "{\"percent\": $percent}"

    # Try to find brightness file to monitor
    brightness_file=$(find /sys/class/backlight/*/brightness 2>/dev/null | head -1)

    if [ -n "$brightness_file" ] && command -v inotifywait &> /dev/null; then
      # Use inotifywait if available for immediate updates
      inotifywait -m -e modify "$brightness_file" 2>/dev/null | while read -r; do
        percent=$(get_brightness)
        if [ "$percent" -lt 1 ]; then
          percent=1
        fi
        echo "{\"percent\": $percent}"
      done
    else
      # Fallback to fast polling (every 0.1s for responsive updates)
      while true; do
        sleep 0.1
        percent=$(get_brightness)
        if [ "$percent" -lt 1 ]; then
          percent=1
        fi
        echo "{\"percent\": $percent}"
      done
    fi
    ;;
  "up")
    # Increase brightness by 5% - run in background for instant response
    if command -v brightnessctl &> /dev/null; then
      brightnessctl set +1% > /dev/null &
    elif command -v light &> /dev/null; then
      light -A 5 > /dev/null &
    fi
    ;;
  "down")
    # Decrease brightness by 5% - run in background for instant response
    # But never go below 1%
    current=$(get_brightness)
    if [ "$current" -gt 1 ]; then
      if command -v brightnessctl &> /dev/null; then
        # Set to max of (current - 1%) or 1%
        new_value=$((current - 1))
        if [ "$new_value" -lt 1 ]; then
          new_value=1
        fi
        brightnessctl set ${new_value}% > /dev/null &
      elif command -v light &> /dev/null; then
        # Check if result would be less than 1%
        new_value=$(awk "BEGIN {printf \"%.0f\", $current - 5}")
        if [ "$new_value" -lt 1 ]; then
          light -S 1 > /dev/null &
        else
          light -U 5 > /dev/null &
        fi
      fi
    fi
    ;;
  *)
    # Default: just get current brightness
    percent=$(get_brightness)
    # Ensure minimum is 1%
    if [ "$percent" -lt 1 ]; then
      percent=1
    fi
    echo "{\"percent\": $percent}"
    ;;
esac
