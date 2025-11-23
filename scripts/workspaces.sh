#!/bin/bash

# JQ filter to extract relevant workspace data
readonly WORKSPACE_FILTER='map({num: .num, name: .name, visible: .visible, focused: .focused, urgent: .urgent, output: .output})'

# Function to get and format workspace data
get_workspaces() {
    swaymsg -t get_workspaces | jq -c "$WORKSPACE_FILTER"
}

# Output initial workspace state
get_workspaces

# Subscribe to workspace changes and output on each event
swaymsg -t subscribe -m '["workspace"]' | while read -r _; do
    get_workspaces
done
