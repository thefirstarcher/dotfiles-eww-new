#!/bin/bash
# ============================================================================
# System Updates Check Script for EWW
# ============================================================================
# Returns JSON with update counts from official repos and AUR

# Function to count official repository updates
count_official_updates() {
  if command -v checkupdates &> /dev/null; then
    # Use checkupdates from pacman-contrib
    checkupdates 2>/dev/null | wc -l
  else
    # Fallback: parse pacman directly (slower but works)
    pacman -Qu 2>/dev/null | wc -l
  fi
}

# Function to count AUR updates
count_aur_updates() {
  if command -v paru &> /dev/null; then
    # Use paru to check AUR updates
    paru -Qua 2>/dev/null | wc -l
  elif command -v yay &> /dev/null; then
    # Fallback to yay if available
    yay -Qua 2>/dev/null | wc -l
  else
    echo 0
  fi
}

# Get counts
official=$(count_official_updates)
aur=$(count_aur_updates)
total=$((official + aur))

# Determine icon based on update count
if [ "$total" -eq 0 ]; then
  icon=$'\uF0ED'  #
elif [ "$total" -lt 10 ]; then
  icon=$'\uF0ED'  #
elif [ "$total" -lt 50 ]; then
  icon=$'\uF0ED'  #
else
  icon=$'\uF0ED'  #
fi

# Output JSON
echo "{\"official\": $official, \"aur\": $aur, \"total\": $total, \"icon\": \"$icon\"}"
