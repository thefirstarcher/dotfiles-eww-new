# Quick Start Guide

## 🎨 Change Theme

Use the theme switcher:

```bash
~/.config/eww/scripts/switch-theme.sh ayu    # Warm & rounded
~/.config/eww/scripts/switch-theme.sh cyber  # Cool & sharp
```

Changes apply instantly (eww auto-reloads)

## 📏 Adjust Bar Size

Edit the `defwindow bar` section in `eww.yuck`:

```lisp
(defwindow bar
  :geometry (geometry
    :width "98%"        ; Change to 100%, 90%, etc.
    :height "26px"      ; Change to 20px, 30px, etc.
    :y "20px")          ; Top offset
  ...
)
```

## 🔧 Quick Customizations

### Make workspace buttons bigger
In your active theme file (`styles/_theme-*.scss`):
```scss
$workspace-min-size: 28px;  // Change from 22px
```

### Change time display
In `config.yuck`, edit the defpoll sections:
```lisp
(defpoll time :interval "1s" "date '+%I:%M %p'")  ; 12-hour with AM/PM
(defpoll date_full :interval "10s" "date '+%a %d'")  ; Short format
```

### Hide/show widgets
In `config.yuck`:
```lisp
(defvar show-workspaces true)
(defvar show-time true)
(defvar show-date false)  ; Hide date, keep time
```

## 📁 File Overview

- **eww.yuck** → Bar position/size, widget structure
- **config.yuck** → Widget spacing, visibility, data sources
- **styles/theme-config.scss** → Pick theme
- **styles/_variables-*.scss** → Customize colors/sizes/effects

## 🔄 Workflow

1. Change setting in `config.yuck` or theme file
2. Save file
3. Run `eww reload`
4. See changes immediately

## 📖 Full Documentation

See `CONFIG-GUIDE.md` for complete reference
