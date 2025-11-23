#!/bin/bash
# ============================================================================
# Battery Status Script for EWW
# ============================================================================
# Returns comprehensive JSON with battery information

BATTERY_PATH="/sys/class/power_supply/BAT0"

# Check if battery exists
if [ ! -d "$BATTERY_PATH" ]; then
  # Try BAT1
  BATTERY_PATH="/sys/class/power_supply/BAT1"
  if [ ! -d "$BATTERY_PATH" ]; then
    # No battery found, return default with all fields
    echo '{"present":false,"percent":0,"status":"N/A","time":"","power":0,"icon":"","health":0,"cycles":0,"voltage":"0.0V","temp":0,"design_capacity":"N/A","current_capacity":"N/A"}'
    exit 0
  fi
fi

# Read battery info
present=true
percent=$(cat "$BATTERY_PATH/capacity" 2>/dev/null || echo "0")
status=$(cat "$BATTERY_PATH/status" 2>/dev/null || echo "Unknown")

# Power in watts (microwatts to watts)
power_now=$(cat "$BATTERY_PATH/power_now" 2>/dev/null || echo "0")
power=$(awk "BEGIN {printf \"%.1f\", $power_now / 1000000}")

# Voltage
voltage_now=$(cat "$BATTERY_PATH/voltage_now" 2>/dev/null || echo "0")
voltage=$(awk "BEGIN {printf \"%.1fV\", $voltage_now / 1000000}")

# Temperature (millidegrees to degrees)
temp_raw=$(cat "$BATTERY_PATH/temp" 2>/dev/null || echo "0")
temp=$(awk "BEGIN {printf \"%.0f\", $temp_raw / 10}")

# Cycle count
cycles=$(cat "$BATTERY_PATH/cycle_count" 2>/dev/null || echo "0")

# Capacity (microampere-hours to milliampere-hours)
design_capacity_raw=$(cat "$BATTERY_PATH/charge_full_design" 2>/dev/null || cat "$BATTERY_PATH/energy_full_design" 2>/dev/null || echo "0")
current_capacity_raw=$(cat "$BATTERY_PATH/charge_full" 2>/dev/null || cat "$BATTERY_PATH/energy_full" 2>/dev/null || echo "0")

design_capacity=$(awk "BEGIN {printf \"%.0f mAh\", $design_capacity_raw / 1000}")
current_capacity=$(awk "BEGIN {printf \"%.0f mAh\", $current_capacity_raw / 1000}")

# Calculate health percentage
if [ "$design_capacity_raw" -gt 0 ]; then
  health=$(awk "BEGIN {printf \"%.0f\", ($current_capacity_raw / $design_capacity_raw) * 100}")
else
  health=0
fi

# Calculate time remaining (if discharging) or time to full (if charging)
time=""
current_now=$(cat "$BATTERY_PATH/current_now" 2>/dev/null || cat "$BATTERY_PATH/power_now" 2>/dev/null || echo "0")
if [ "$current_now" -gt 0 ]; then
  if [ "$status" = "Discharging" ]; then
    hours=$(awk "BEGIN {printf \"%.0f\", ($current_capacity_raw / $current_now)}")
    minutes=$(awk "BEGIN {printf \"%.0f\", (($current_capacity_raw / $current_now) - $hours) * 60}")
    time="${hours}h ${minutes}m"
  elif [ "$status" = "Charging" ]; then
    remaining=$(awk "BEGIN {print $design_capacity_raw - $current_capacity_raw}")
    hours=$(awk "BEGIN {printf \"%.0f\", ($remaining / $current_now)}")
    minutes=$(awk "BEGIN {printf \"%.0f\", (($remaining / $current_now) - $hours) * 60}")
    time="${hours}h ${minutes}m"
  fi
fi

# Determine icon based on status and percentage
local icon=""
if [ "$status" = "Charging" ]; then
    icon="󰂄"
elif [ "$status" = "Full" ]; then
    icon="󰁹"
elif [ "$percent" -ge 90 ]; then
    icon="󰁹"
elif [ "$percent" -ge 70 ]; then
    icon="󰂀"
elif [ "$percent" -ge 50 ]; then
    icon="󰁾"
elif [ "$percent" -ge 30 ]; then
    icon="󰁼"
elif [ "$percent" -ge 10 ]; then
    icon="󰁺"
else
    icon="󰂎"
fi

# Output JSON with all fields
echo "{\"present\":$present,\"percent\":$percent,\"status\":\"$status\",\"time\":\"$time\",\"power\":$power,\"icon\":\"$icon\",\"health\":$health,\"cycles\":$cycles,\"voltage\":\"$voltage\",\"temp\":$temp,\"design_capacity\":\"$design_capacity\",\"current_capacity\":\"$current_capacity\"}"
