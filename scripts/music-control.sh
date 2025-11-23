#!/bin/bash
# ============================================================================
# Music Control Script for EWW Dashboard
# ============================================================================
# Handles playerctl commands and signals daemon for instant updates

SWITCH_FILE="/tmp/eww-music-player-switch"
PID_FILE="/tmp/eww-music-daemon.pid"

# Get active player from music daemon state
get_active_player() {
  eww get music | jq -r '.active_player'
}

# Function to wake up the daemon safely using PID file
refresh_daemon() {
  if [ -f "$PID_FILE" ]; then
    # Read PID and check if process exists
    daemon_pid=$(cat "$PID_FILE")
    if kill -0 "$daemon_pid" 2>/dev/null; then
      kill -USR1 "$daemon_pid"
    else
      # Process dead, remove stale file
      rm -f "$PID_FILE"
    fi
  else
    # Fallback if PID file missing (try to find via pgrep but safer)
    pkill -USR1 -f "scripts/music-daemon.py" 2>/dev/null
  fi
}

# Helper to cycle players
cycle_player() {
  direction=$1 # "next" or "prev"

  # Get full state once
  state=$(eww get music)

  # Calculate next player using jq
  # Logic: Find index of active player, add/subtract 1, modulo length
  target_player=$(echo "$state" | jq -r --arg dir "$direction" '
    .active_player as $active |
    .available_players as $players |
    ($players | length) as $len |
    ($players | index($active)) as $idx |

    if $idx == null then
      $players[0]
    elif $dir == "next" then
      $players[($idx + 1) % $len]
    else
      $players[($idx - 1 + $len) % $len]
    end
  ')

  if [ -n "$target_player" ] && [ "$target_player" != "null" ]; then
    echo "$target_player" > "$SWITCH_FILE"
    refresh_daemon
  fi
}

case "$1" in
  "playpause")
    player=$(get_active_player)
    # Run blocking to ensure state changes before we refresh
    [ -n "$player" ] && playerctl -p "$player" play-pause
    refresh_daemon
    ;;
  "previous")
    player=$(get_active_player)
    [ -n "$player" ] && playerctl -p "$player" previous
    refresh_daemon
    ;;
  "next")
    player=$(get_active_player)
    [ -n "$player" ] && playerctl -p "$player" next
    refresh_daemon
    ;;
  "switch")
    # Write player name to switch file
    echo "$2" > "$SWITCH_FILE"
    # Wake up daemon immediately to process the switch
    refresh_daemon
    ;;
  "next_player")
    cycle_player "next"
    ;;
  "prev_player")
    cycle_player "prev"
    ;;
  "seek")
    player=$(get_active_player)
    if [ -n "$player" ]; then
      duration=$(playerctl -p "$player" metadata mpris:length 2>/dev/null)
      if [ -n "$duration" ]; then
        duration_sec=$((duration / 1000000))
        position=$(echo "scale=2; $duration_sec * $2 / 100" | bc)
        playerctl -p "$player" position "$position"
        refresh_daemon
      fi
    fi
    ;;
  *)
    echo "Usage: $0 {playpause|previous|next|switch PLAYER|next_player|prev_player|seek PERCENT}"
    exit 1
    ;;
esac
