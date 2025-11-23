#!/bin/bash

# ============================================================================
# EWW THEME SWITCHER
# ============================================================================
# Switches between different eww themes using symlinks (DRY principle)
#
# Usage: ./switch-theme.sh [ayu|cyber]
#        ./switch-theme.sh        (shows current theme)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STYLES_DIR="$(cd "$SCRIPT_DIR/../styles" && pwd)"
THEME_LINK="$STYLES_DIR/theme-config.scss"

# Available themes
THEME_AYU="_theme-ayu-dark.scss"
THEME_AYU_MINI="_theme-ayu-mini.scss"
THEME_CYBER="_theme-cyber-blue-sharp.scss"
THEME_CYBER_MINI="_theme-cyber-mini.scss"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Function to get current theme
get_current_theme() {
    if [[ -L "$THEME_LINK" ]]; then
        local target=$(readlink "$THEME_LINK")
        local theme_name=$(basename "$target" | sed 's/_theme-//' | sed 's/.scss//')
        echo "$theme_name"
    else
        echo "unknown"
    fi
}

# Function to show current theme
show_current() {
    local current=$(get_current_theme)
    local display_name=""

    case "$current" in
        ayu-dark)
            display_name="Ayu Dark (warm, rounded, bubble style)"
            ;;
        ayu-mini)
            display_name="Ayu Mini (warm, compact, flat style with separators)"
            ;;
        cyber-blue-sharp)
            display_name="Cyber Blue Sharp (cool, angular, bubble style)"
            ;;
        cyber-mini)
            display_name="Cyber Mini (cool, compact, flat style with separators)"
            ;;
        *)
            display_name="Unknown"
            ;;
    esac

    echo -e "${CYAN}Current theme:${NC} ${GREEN}$display_name${NC}"

    if [[ -L "$THEME_LINK" ]]; then
        local target=$(readlink "$THEME_LINK")
        echo -e "${CYAN}Symlinked to:${NC} $target"
    fi
}

# Function to switch theme
switch_theme() {
    local theme=$1
    local theme_file=""
    local theme_display=""

    case "$theme" in
        ayu|ayu-dark)
            theme_file="$THEME_AYU"
            theme_display="Ayu Dark"
            ;;
        ayu-mini)
            theme_file="$THEME_AYU_MINI"
            theme_display="Ayu Dark Mini"
            ;;
        cyber-mini)
            theme_file="$THEME_CYBER_MINI"
            theme_display="Cyber Blue Sharp Mini"
            ;;
        cyber|cyber-blue|futuristic)
            theme_file="$THEME_CYBER"
            theme_display="Cyber Blue Sharp"
            ;;
        *)
            echo -e "${RED}Error: Unknown theme '$theme'${NC}"
            echo ""
            echo "Available themes:"
            echo -e "  ${GREEN}ayu${NC}         - Ayu Dark (full, bubble style)"
            echo -e "  ${GREEN}ayu-mini${NC}    - Ayu Mini (compact, flat style)"
            echo -e "  ${BLUE}cyber${NC}       - Cyber Blue Sharp (full, bubble style)"
            echo -e "  ${BLUE}cyber-mini${NC}  - Cyber Mini (compact, flat style)"
            return 1
            ;;
    esac

    # Check if theme file exists
    local full_path="$STYLES_DIR/$theme_file"
    if [[ ! -f "$full_path" ]]; then
        echo -e "${RED}Error: Theme file not found: $full_path${NC}"
        return 1
    fi

    # Remove existing symlink
    if [[ -L "$THEME_LINK" ]]; then
        rm "$THEME_LINK"
    fi

    # Create symlink to theme file
    ln -s "$theme_file" "$THEME_LINK"

    echo -e "${GREEN}✓${NC} Switched to ${YELLOW}$theme_display${NC} theme"
    echo -e "${CYAN}→${NC} $full_path"

    # Note: Separators are always rendered but styled to be invisible in full themes
    if [[ "$theme" == *"mini"* ]]; then
        echo -e "${CYAN}→${NC} Compact style (flat, with visible separators)"
    else
        echo -e "${CYAN}→${NC} Full style (bubble widgets, no separators)"
    fi

    # Touch the main eww.scss file to trigger reload
    # (symlink changes don't trigger file watchers)
    touch "$STYLES_DIR/../eww.scss"
}

# Function to list available themes
list_themes() {
    echo -e "${CYAN}Available themes:${NC}"
    echo ""

    if [[ -f "$STYLES_DIR/$THEME_AYU" ]]; then
        echo -e "  ${GREEN}ayu${NC}          - Ayu Dark (full)"
        echo "                 Rounded corners, warm orange accent"
        echo "                 Soft glows, bubble widgets, modern feel"
        echo ""
    fi

    if [[ -f "$STYLES_DIR/$THEME_AYU_MINI" ]]; then
        echo -e "  ${GREEN}ayu-mini${NC}     - Ayu Mini (compact)"
        echo "                 Same colors, minimal padding"
        echo "                 Flat style with thin separators"
        echo ""
    fi

    if [[ -f "$STYLES_DIR/$THEME_CYBER" ]]; then
        echo -e "  ${BLUE}cyber${NC}        - Cyber Blue Sharp (full)"
        echo "                 Sharp edges, bright cyan accent"
        echo "                 Strong neon glows, bubble widgets, futuristic"
        echo ""
    fi

    if [[ -f "$STYLES_DIR/$THEME_CYBER_MINI" ]]; then
        echo -e "  ${BLUE}cyber-mini${NC}   - Cyber Mini (compact)"
        echo "                 Same colors, minimal padding"
        echo "                 Flat style with thin separators"
        echo ""
    fi

    echo -e "${CYAN}Usage:${NC}"
    echo "  $0 ayu          - Switch to Ayu Dark theme (full)"
    echo "  $0 ayu-mini     - Switch to Ayu Mini theme (compact)"
    echo "  $0 cyber        - Switch to Cyber Blue Sharp theme (full)"
    echo "  $0 cyber-mini   - Switch to Cyber Mini theme (compact)"
    echo "  $0              - Show current theme"
}

# Main script
main() {
    if [[ $# -eq 0 ]]; then
        # No arguments - show current theme and list available
        show_current
        echo ""
        list_themes
    elif [[ "$1" == "list" ]] || [[ "$1" == "-l" ]] || [[ "$1" == "--list" ]]; then
        list_themes
    elif [[ "$1" == "current" ]] || [[ "$1" == "-c" ]] || [[ "$1" == "--current" ]]; then
        show_current
    else
        # Switch theme
        switch_theme "$1"
    fi
}

main "$@"
