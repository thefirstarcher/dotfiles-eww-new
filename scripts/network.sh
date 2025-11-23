#!/bin/bash
# ============================================================================
# Network Status Script for EWW
# ============================================================================
# Returns JSON with network type, icon, and name

# Check for WiFi connection
wifi_status=$(nmcli -t -f TYPE,STATE device | grep '^wifi:connected$')
if [ -n "$wifi_status" ]; then
  # Get WiFi SSID
  ssid=$(nmcli -t -f active,ssid dev wifi | grep '^yes' | cut -d':' -f2)
  # Get signal strength for icon
  signal=$(nmcli -t -f active,signal dev wifi | grep '^yes' | cut -d':' -f2)

  if [ "$signal" -ge 80 ]; then
    icon="󰤨"
  elif [ "$signal" -ge 60 ]; then
    icon="󰤥"
  elif [ "$signal" -ge 40 ]; then
    icon="󰤢"
  elif [ "$signal" -ge 20 ]; then
    icon="󰤟"
  else
    icon="󰤯"
  fi

  echo "{\"type\": \"wifi\", \"icon\": \"$icon\", \"name\": \"$ssid\", \"percent\": $signal}"
  exit 0
fi

# Check for Ethernet connection
ethernet_status=$(nmcli -t -f TYPE,STATE device | grep '^ethernet:connected$')
if [ -n "$ethernet_status" ]; then
  echo "{\"type\": \"ethernet\", \"icon\": \"󰈀\", \"name\": \"Ethernet\", \"percent\": 100}"
  exit 0
fi

# Disconnected
echo "{\"type\": \"disconnected\", \"icon\": \"󰤭\", \"name\": \"Disconnected\", \"percent\": 0}"
