#!/bin/bash
# ============================================================================
# Weather Script for EWW
# ============================================================================
# Returns JSON with weather data from wttr.in

# Get weather from wttr.in (change location if needed)
# Format: %t = temperature, %C = condition, %c = icon
weather_data=$(curl -s "wttr.in/?format=%t|%C|%c" 2>/dev/null)

if [ -z "$weather_data" ] || [ "$weather_data" = "Unknown location; please try ~" ]; then
  # Failed to get weather
  echo '{"temp": "", "condition": "No data", "icon": "󰖐"}'
  exit 0
fi

# Parse the data
temp=$(echo "$weather_data" | cut -d'|' -f1 | tr -d ' ')
condition=$(echo "$weather_data" | cut -d'|' -f2)
icon_raw=$(echo "$weather_data" | cut -d'|' -f3 | tr -d ' ')

# Map wttr.in emoji/text to nerd font icons
case "$icon_raw" in
  *"☀"*|*"Sunny"*|*"Clear"*)
    icon="󰖙"
    ;;
  *"⛅"*|*"Partly"*|*"cloudy"*)
    icon="󰖕"
    ;;
  *"☁"*|*"Cloudy"*|*"Overcast"*)
    icon="󰖐"
    ;;
  *"🌧"*|*"Rain"*|*"Drizzle"*)
    icon="󰖗"
    ;;
  *"⛈"*|*"Thunder"*|*"storm"*)
    icon="󰙾"
    ;;
  *"🌨"*|*"Snow"*)
    icon="󰖘"
    ;;
  *"🌫"*|*"Fog"*|*"Mist"*)
    icon="󰖑"
    ;;
  *)
    icon="󰖐"
    ;;
esac

echo "{\"temp\": \"$temp\", \"condition\": \"$condition\", \"icon\": \"$icon\"}"
