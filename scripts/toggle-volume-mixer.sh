#!/bin/bash
# Toggle volume mixer window

if eww windows | grep -q "volume-mixer"; then
    eww close volume-mixer
    eww update show-volume-mixer=false
else
    eww open volume-mixer
    eww update show-volume-mixer=true
fi
