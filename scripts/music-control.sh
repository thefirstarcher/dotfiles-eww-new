#!/bin/bash
# ============================================================================
# Music Control Script for EWW Dashboard
# ============================================================================
# Handles playerctl commands for music playback control

SWITCH_FILE="/tmp/eww-music-player-switch"

# Get active player from music daemon state
get_active_player() {
  eww get music | jq -r '.active_player'
}

case "$1" in
  "playpause")
    player=$(get_active_player)
    [ -n "$player" ] && playerctl -p "$player" play-pause &
    ;;
  "previous")
    player=$(get_active_player)
    [ -n "$player" ] && playerctl -p "$player" previous &
    ;;
  "next")
    player=$(get_active_player)
    [ -n "$player" ] && playerctl -p "$player" next &
    ;;
  "switch")
    # Write player name to switch file for daemon to read
    echo "$2" > "$SWITCH_FILE"
    ;;
  "seek")
    player=$(get_active_player)
    if [ -n "$player" ]; then
      # Get duration in seconds
      duration=$(playerctl -p "$player" metadata mpris:length 2>/dev/null)
      if [ -n "$duration" ]; then
        # Convert from microseconds to seconds
        duration_sec=$((duration / 1000000))
        # Calculate position from percentage
        position=$(echo "scale=2; $duration_sec * $2 / 100" | bc)
        playerctl -p "$player" position "$position" &
      fi
    fi
    ;;
  *)
    echo "Usage: $0 {playpause|previous|next|switch PLAYER|seek PERCENT}"
    exit 1
    ;;
esac
