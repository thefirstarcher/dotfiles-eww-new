#!/bin/bash
# Microphone control commands for unified eww-mixer

MIXER="$HOME/.config/eww/rust-applets/eww-mixer/target/release/eww-mixer"

# Get default source index and volume from mixer state
get_default_source_info() {
    $MIXER get-state 2>/dev/null | jq -r '.sources[] | select(.is_default == true) | "\(.index) \(.volume)"'
}

# Parse command
case "$1" in
    volume-up)
        read -r SOURCE CURRENT <<< $(get_default_source_info)
        if [ -n "$SOURCE" ]; then
            NEW=$((CURRENT + 5))
            [ $NEW -gt 150 ] && NEW=150
            $MIXER set-volume source $SOURCE $NEW &
        fi
        ;;
    volume-down)
        read -r SOURCE CURRENT <<< $(get_default_source_info)
        if [ -n "$SOURCE" ]; then
            NEW=$((CURRENT - 5))
            [ $NEW -lt 0 ] && NEW=0
            $MIXER set-volume source $SOURCE $NEW &
        fi
        ;;
    toggle-mute-default)
        SOURCE=$(get_default_source_info | awk '{print $1}')
        if [ -n "$SOURCE" ]; then
            $MIXER toggle-mute source $SOURCE &
        fi
        ;;
    *)
        exec "$MIXER" "$@"
        ;;
esac
