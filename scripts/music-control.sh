#!/bin/bash
SWITCH_FILE="/tmp/eww-music-player-switch"

get_active_player() {
  eww get music | jq -r '.active_player'
}

cycle_player() {
  direction=$1
  state=$(eww get music)
  target_player=$(echo "$state" | jq -r --arg dir "$direction" '
    .active_player as $active | .available_players as $players | ($players | length) as $len | ($players | index($active)) as $idx |
    if $idx == null then $players[0] elif $dir == "next" then $players[($idx + 1) % $len] else $players[($idx - 1 + $len) % $len] end
  ')
  [ -n "$target_player" ] && [ "$target_player" != "null" ] && echo "$target_player" > "$SWITCH_FILE"
}

case "$1" in
  "playpause")
    player=$(get_active_player); [ -n "$player" ] && playerctl -p "$player" play-pause & ;;
  "previous")
    player=$(get_active_player); [ -n "$player" ] && playerctl -p "$player" previous & ;;
  "next")
    player=$(get_active_player); [ -n "$player" ] && playerctl -p "$player" next & ;;
  "switch")
    echo "$2" > "$SWITCH_FILE" ;;
  "next_player")
    cycle_player "next" ;;
  "prev_player")
    cycle_player "prev" ;;
  "seek")
    # Check if numeric to prevent 50% reset bug
    if [[ "$2" =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
        player=$(get_active_player)
        if [ -n "$player" ]; then
            duration=$(playerctl -p "$player" metadata mpris:length 2>/dev/null)
            if [ -n "$duration" ] && [ "$duration" -gt 0 ]; then
                duration_sec=$((duration / 1000000))
                position=$(echo "scale=2; $duration_sec * $2 / 100" | bc)
                playerctl -p "$player" position "$position" &
            fi
        fi
    fi
    ;;
  *)
    echo "Usage: $0 {playpause|previous|next|switch PLAYER|next_player|prev_player|seek PERCENT}"
    exit 1
    ;;
esac
