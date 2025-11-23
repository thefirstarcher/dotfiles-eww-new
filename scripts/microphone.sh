#!/bin/bash
# ============================================================================
# Microphone Control Script for EWW
# ============================================================================
# Controls microphone volume and mute status
# Supports scroll up/down to change volume and click to toggle mute

case "$1" in
  "up")
    # Increase microphone volume by 2%
    pactl set-source-volume @DEFAULT_SOURCE@ +2% &
    ;;
  "down")
    # Decrease microphone volume by 2%
    pactl set-source-volume @DEFAULT_SOURCE@ -2% &
    ;;
  "toggle")
    # Toggle mute - run in background for instant response
    pactl set-source-mute @DEFAULT_SOURCE@ toggle &
    ;;
  *)
    echo "Usage: $0 {up|down|toggle}"
    exit 1
    ;;
esac
