#!/bin/bash
# ============================================================================
# Notifications Status Script for EWW
# ============================================================================
# Returns JSON with notification count and DND status

# Check if using swaync, dunst, or mako
if command -v swaync-client &> /dev/null; then
  # Using swaync
  count=$(swaync-client -c 2>/dev/null || echo "0")
  dnd=$(swaync-client -D 2>/dev/null)

  if [ "$dnd" = "true" ]; then
    dnd="true"
  else
    dnd="false"
  fi
elif command -v dunstctl &> /dev/null; then
  # Using dunst
  count=$(dunstctl count waiting 2>/dev/null || echo "0")
  dnd=$(dunstctl is-paused 2>/dev/null)

  if [ "$dnd" = "true" ]; then
    dnd="true"
  else
    dnd="false"
  fi
elif command -v makoctl &> /dev/null; then
  # Using mako
  count=$(makoctl list | jq '. | length' 2>/dev/null || echo "0")
  dnd="false"  # Mako doesn't have a simple DND check
else
  # No notification daemon found
  count=0
  dnd="false"
fi

echo "{\"count\": $count, \"dnd\": $dnd}"
