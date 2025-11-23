#!/bin/bash
# ============================================================================
# User Info Script for EWW Dashboard
# ============================================================================
# Returns JSON with username and uptime breakdown

# Get username
username=$(whoami)

# Parse uptime into components
uptime_seconds=$(cat /proc/uptime | cut -d' ' -f1 | cut -d'.' -f1)
days=$((uptime_seconds / 86400))
hours=$(((uptime_seconds % 86400) / 3600))
minutes=$(((uptime_seconds % 3600) / 60))

# Output JSON
jq -n \
  --arg username "$username" \
  --arg days "$days days" \
  --arg hours "$hours hours" \
  --arg minutes "$minutes minutes" \
  '{username: $username, uptime_days: $days, uptime_hours: $hours, uptime_minutes: $minutes}'
