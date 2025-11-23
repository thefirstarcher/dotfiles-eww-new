#!/bin/bash
# ============================================================================
# Volume Control Script for EWW
# ============================================================================
# Returns JSON with volume percentage and mute status
# Supports scroll up/down to change volume and click to toggle mute

get_volume() {
  # Get default sink volume and mute status using pactl
  local volume=$(pactl get-sink-volume @DEFAULT_SINK@ | grep -oP '\d+%' | head -1 | tr -d '%')
  local muted=$(pactl get-sink-mute @DEFAULT_SINK@ | grep -oP '(yes|no)')

  # Convert mute status to boolean
  if [ "$muted" = "yes" ]; then
    muted="true"
  else
    muted="false"
  fi

  # Output JSON
  echo "{\"percent\": $volume, \"muted\": $muted}"
}

case "$1" in
  "listen")
    # Listen mode: output current volume and subscribe to changes
    get_volume
    pactl subscribe | while read -r event; do
      # Only react to sink events (volume/mute changes)
      if echo "$event" | grep -q "sink"; then
        get_volume
      fi
    done
    ;;
  "up")
    # Increase volume by 3%, use relative adjustment for instant response
    pactl set-sink-volume @DEFAULT_SINK@ +2% &
    ;;
  "down")
    # Decrease volume by 3%, use relative adjustment for instant response
    pactl set-sink-volume @DEFAULT_SINK@ -2% &
    ;;
  "toggle")
    # Toggle mute - run in background for instant response
    pactl set-sink-mute @DEFAULT_SINK@ toggle &
    ;;
  *)
    # Default: just get current volume
    get_volume
    ;;
esac
