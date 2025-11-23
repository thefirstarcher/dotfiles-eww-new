#!/bin/bash
# Volume control commands for unified eww-mixer

MIXER="$HOME/.config/eww/rust-applets/eww-mixer/target/release/eww-mixer"

# Get default sink index and volume from mixer state
get_default_sink_info() {
    $MIXER get-state 2>/dev/null | jq -r '.sinks[] | select(.is_default == true) | "\(.index) \(.volume)"'
}

# Parse command
case "$1" in
    volume-up)
        read -r SINK CURRENT <<< $(get_default_sink_info)
        if [ -n "$SINK" ]; then
            NEW=$((CURRENT + 5))
            [ $NEW -gt 150 ] && NEW=150
            $MIXER set-volume sink $SINK $NEW &
        fi
        ;;
    volume-down)
        read -r SINK CURRENT <<< $(get_default_sink_info)
        if [ -n "$SINK" ]; then
            NEW=$((CURRENT - 5))
            [ $NEW -lt 0 ] && NEW=0
            $MIXER set-volume sink $SINK $NEW &
        fi
        ;;
    toggle-mute-default)
        SINK=$(get_default_sink_info | awk '{print $1}')
        if [ -n "$SINK" ]; then
            $MIXER toggle-mute sink $SINK &
        fi
        ;;
    *)
        exec "$MIXER" "$@"
        ;;
esac
