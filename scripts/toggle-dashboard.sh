#!/bin/bash

# Check if dashboard window is actually open in Eww
if eww active-windows | grep -q "dashboard"; then
    eww close dashboard
    eww update show-dashboard=false
else
    eww open dashboard
    eww update show-dashboard=true
fi
