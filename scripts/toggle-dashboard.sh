#!/bin/bash

# Check if dashboard is currently open
if eww active-windows | grep -q "dashboard"; then
    eww close dashboard
    eww update show-dashboard=false
else
    eww open dashboard
    eww update show-dashboard=true
fi
