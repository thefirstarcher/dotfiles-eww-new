#!/bin/bash
# ============================================================================
# Bluetooth Status Script for EWW
# ============================================================================
# Returns JSON with bluetooth enabled status, connected status, and device name

# Check if bluetooth is powered on
powered=$(bluetoothctl show | grep "Powered" | awk '{print $2}')

if [ "$powered" = "yes" ]; then
  enabled="true"

  # Check for connected devices
  connected_device=$(bluetoothctl devices Connected | head -1)

  if [ -n "$connected_device" ]; then
    # Extract device name (everything after MAC address)
    device_name=$(echo "$connected_device" | sed 's/^Device [0-9A-F:]* //')
    echo "{\"enabled\": true, \"connected\": true, \"device\": \"$device_name\"}"
  else
    echo "{\"enabled\": true, \"connected\": false, \"device\": \"\"}"
  fi
else
  echo "{\"enabled\": false, \"connected\": false, \"device\": \"\"}"
fi
