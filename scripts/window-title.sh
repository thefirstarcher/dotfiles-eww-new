#!/bin/bash
# ============================================================================
# Window Title Monitor for EWW (Sway)
# ============================================================================
# Monitors the focused window title and returns it as JSON

get_window_title() {
  # Get focused window title from sway and properly escape for JSON
  local title=$(swaymsg -t get_tree 2>/dev/null | jq -r '.. | select(.focused? == true) | .name // ""' 2>/dev/null || echo "")

  # Truncate if too long (max 80 characters)
  if [ ${#title} -gt 80 ]; then
    title="${title:0:77}..."
  fi

  # Use jq to properly escape the title for JSON output (compact single-line)
  jq -n -c --arg title "$title" '{title: $title}'
}

case "$1" in
  "listen")
    # Listen mode: output current title and subscribe to changes
    get_window_title
    swaymsg -t subscribe -m '["window"]' 2>/dev/null | while read -r event; do
      # On any window event, get the new focused window title
      # Add a small delay to ensure window state has stabilized
      sleep 0.05
      get_window_title
    done
    ;;
  *)
    # Default: just get current title
    get_window_title
    ;;
esac
