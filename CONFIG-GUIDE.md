# EWW Configuration Guide

**A highly configurable, DRY-compliant eww setup following Occam's Razor principles**

## 🎯 Philosophy

This configuration follows three core principles:

1. **DRY (Don't Repeat Yourself)** - Single source of truth for all values
2. **Abstraction/Encapsulation** - Components are isolated and composable
3. **Occam's Razor** - Simplest solution that works, no over-engineering

## 📁 File Structure

```
eww/
├── config.yuck              # Central configuration (sizes, spacing, behavior)
├── eww.yuck                 # Widget definitions (what to show)
├── eww.scss                 # Style imports (how it looks)
└── styles/
    ├── theme-config.scss    # Theme selector (pick your theme)
    ├── _variables-*.scss    # Theme files (colors, shadows, etc.)
    ├── _reset.scss          # Global resets
    ├── _layouts.scss        # Bar layout containers
    ├── _workspace.scss      # Workspace component
    ├── _button.scss         # Button component
    ├── _card.scss           # Card component
    ├── _text.scss           # Text variants
    ├── _icon.scss           # Icon styling
    ├── _badge.scss          # Badge/notification component
    ├── _time.scss           # Time/date widget
    └── _modifiers.scss      # Utility classes
```

## 🎨 How to Configure

### 1. Basic Bar Configuration

#### Bar Position/Size (`eww.yuck`)

Due to eww limitations, geometry must be configured directly in the window definition:

```lisp
(defwindow bar
  :monitor 0              ; Which monitor (0, 1, 2...)
  :geometry (geometry
    :x "0%"              ; Horizontal position
    :y "20px"            ; Vertical offset from anchor
    :width "98%"         ; Bar width
    :height "26px"       ; Bar height
    :anchor "top center") ; Anchor position
  :stacking "fg"         ; Layer (fg = foreground)
  :exclusive true        ; Reserve space
  (bar_layout))
```

**Common adjustments:**

- **Slim bar**: `:height "20px"`, `:y "10px"`
- **Bottom bar**: `:anchor "bottom center"`
- **Full width**: `:width "100%"`, `:x "0px"`
- **Second monitor**: `:monitor 1`

#### Widget Configuration (`config.yuck`)

```lisp
;; Spacing
(defvar bar-section-spacing 8)   ; Space between sections
(defvar bar-widget-spacing 4)    ; Space between widgets

;; Widget visibility
(defvar show-workspaces true)
(defvar show-time true)
(defvar show-date true)
```

### 2. Theme Selection

#### Using the Theme Switcher Script (Recommended)

```bash
# Switch to a theme
~/.config/eww/scripts/switch-theme.sh ayu    # Warm & rounded
~/.config/eww/scripts/switch-theme.sh cyber  # Cool & sharp

# Show current theme
~/.config/eww/scripts/switch-theme.sh
```

The script uses symlinks to switch themes instantly. Eww auto-reloads on file changes.

#### Manual Theme Switching

The `theme-config.scss` file is a symlink. Change it manually:

```bash
cd ~/.config/eww/styles
rm theme-config.scss
ln -s _theme-ayu-dark.scss theme-config.scss          # Ayu theme
# OR
ln -s _theme-cyber-blue-sharp.scss theme-config.scss  # Cyber theme
```

### 3. Creating Custom Themes

Copy an existing theme file:

```bash
cd ~/.config/eww/styles
cp _theme-ayu-dark.scss _theme-my-custom.scss
```

Edit your new theme file - all values are clearly labeled:

```scss
// Colors
$bg-primary: #0A0E14;
$accent-primary: #E6B450;

// Borders
$radius-md: 8px;  // Rounded corners
$border-width-thin: 1px;

// Component-specific
$workspace-min-size: 22px;
$workspace-padding: 2px;
```

Activate your theme:

```bash
rm theme-config.scss
ln -s _theme-my-custom.scss theme-config.scss
```

### 4. Component Size Presets

Use size modifiers in widgets (defined in SCSS variables):

```lisp
;; In eww.yuck
(button :class "workspace size-sm" "1")  ; Small
(button :class "workspace size-md" "2")  ; Medium (default)
(button :class "workspace size-lg" "3")  ; Large
```

Sizes are configured in the theme file:

```scss
// In _variables-*.scss
$workspace-min-size: 22px;   // Default size
$workspace-padding: 2px;     // Internal spacing

// Size variants in _modifiers.scss automatically scale
```

## 🔧 Advanced Configuration

### Adding New Widgets

1. **Create the widget in `eww.yuck`:**

```lisp
(defwidget my-widget []
  (box
    :class "my-widget"
    :orientation "h"
    (label :text "Hello")
  )
)
```

2. **Add to a section:**

```lisp
;; In bar_layout, add to any section
(section :halign "end" :class "right_layout"
  (my-widget))
```

3. **Style it (create `styles/_my-widget.scss`):**

```scss
.my-widget {
  background: $bg-elevated;
  padding: $space-sm;
  border-radius: $radius-md;
  color: $fg-primary;
}
```

4. **Import in `eww.scss`:**

```scss
@import "styles/my-widget";
```

### Creating Multiple Bars

In `config.yuck`, duplicate and modify bar variables:

```lisp
;; Top bar
(defvar bar-top-height "26px")
(defvar bar-top-y "20px")

;; Bottom bar
(defvar bar-bottom-height "30px")
(defvar bar-bottom-y "20px")
```

In `eww.yuck`, create multiple windows:

```lisp
(defwindow bar-top
  :geometry (geometry :y bar-top-y :height bar-top-height ...)
  (bar_layout))

(defwindow bar-bottom
  :geometry (geometry :y bar-bottom-y :height bar-bottom-height ...)
  (bottom_layout))
```

### Conditional Widgets

Use the `conditional-widget` wrapper:

```lisp
;; Only show when condition is true
(conditional-widget :condition show-workspaces
  (workspaces))
```

Toggle in `config.yuck`:

```lisp
(defvar show-workspaces true)   ; Visible
(defvar show-workspaces false)  ; Hidden
```

## 🎨 Styling Guidelines

### Using Theme Variables

**Always use variables, never hardcode values:**

```scss
/* ❌ DON'T */
.my-widget {
  background: #0A0E14;
  padding: 8px;
  border-radius: 12px;
}

/* ✅ DO */
.my-widget {
  background: $bg-primary;
  padding: $space-sm;
  border-radius: $radius-lg;
}
```

### Component States

Each component handles its own states:

```scss
.workspace {
  // Base styles
  background: $workspace-bg-default;

  // State modifiers
  &.active { ... }
  &.occupied { ... }
  &.urgent { ... }
  &:hover { ... }
}
```

### Utility Classes

Apply modifiers for quick adjustments:

```lisp
;; Spacing
(box :class "my-widget p-md m-sm" ...)

;; Colors
(label :class "text fg-blue" ...)

;; Effects
(box :class "card glow-primary" ...)

;; Sizes
(button :class "button size-lg" ...)
```

## 📏 Common Customizations

### Workspace Buttons

In your theme file (`_variables-*.scss`):

```scss
// Size
$workspace-min-size: 28px;        // Larger buttons
$workspace-padding: 4px;          // More padding

// Colors
$workspace-text-active: #000;     // Black text when active
$workspace-bg-active: $gradient-primary;  // Gradient background

// Effects
$workspace-shadow-active: $glow-size-lg $glow-primary;  // Glow
$workspace-radius: 0px;           // Sharp corners (or $radius-full for circles)
```

### Time Widget

```scss
// In _variables-*.scss
$time-text-size: 16px;           // Bigger time
$time-text-color: $accent-primary-bright;  // Cyan color
$date-text-size: 10px;           // Smaller date
$time-padding-x: $space-xl;      // More horizontal padding
```

### Bar Appearance

```scss
$bar-bg: $gradient-bg-primary;   // Background gradient
$bar-border: $accent-primary;    // Accent border
$bar-radius: $radius-none;       // Sharp or rounded
$bar-shadow: $shadow-xl;         // Shadow depth
$bar-padding-x: $space-lg;       // Horizontal padding
```

## 🔄 Workflow

1. **Configuration changes** → Edit `config.yuck`
2. **Theme/color changes** → Edit theme file or switch in `theme-config.scss`
3. **Widget behavior** → Edit `eww.yuck`
4. **New components** → Create new `.scss` file, import in `eww.scss`
5. **Reload** → `eww reload` or `eww open bar` (auto-reloads on file change)

## 🎯 Design Principles Applied

### DRY (Don't Repeat Yourself)
- All colors/sizes defined once in theme files
- Reusable widgets (`section`, `conditional-widget`)
- Shared utility classes (`.m-sm`, `.fg-blue`, etc.)

### Abstraction/Encapsulation
- Components are self-contained (workspace, button, time)
- Clear separation: config.yuck (behavior) ↔ scss (appearance)
- Widget composition: small widgets build bigger layouts

### Occam's Razor
- Minimal file structure - only essential files
- No over-engineering - straightforward CSS without tricks
- Clear naming - no cryptic abbreviations
- Direct configuration - change values, not code

## 🚀 Quick Start Examples

### Example 1: Minimal Bar
```lisp
// config.yuck
(defvar bar-height "20px")
(defvar show-workspaces true)
(defvar show-time true)
(defvar show-date false)
```

### Example 2: Gaming Theme (Cyber Blue Sharp)
```scss
// theme-config.scss
// @import "variables-ayu-dark";
@import "variables-cyber-blue-sharp";
```

### Example 3: Multiple Monitors
```lisp
// In eww.yuck
(defwindow bar-left :monitor 0 ...)
(defwindow bar-right :monitor 1 ...)
```

## 📚 Reference

### Available Variables (in theme files)

**Colors:** `$bg-primary`, `$fg-primary`, `$accent-primary`, `$accent-blue`, `$accent-green`, etc.
**Spacing:** `$space-xs`, `$space-sm`, `$space-md`, `$space-lg`, `$space-xl`
**Borders:** `$radius-sm`, `$radius-md`, `$radius-lg`, `$border-width-thin`
**Shadows:** `$shadow-sm`, `$shadow-lg`, `$glow-size-md`, `$glow-primary`

### Available Utility Classes

**Spacing:** `.m-xs` `.m-sm` `.p-md` `.p-lg`
**Colors:** `.fg-primary` `.fg-blue` `.fg-green` `.fg-red`
**Effects:** `.glow-primary` `.glow-blue` `.glow-red`
**Sizes:** `.size-sm` `.size-md` `.size-lg`
**Gradients:** `.gradient-primary` `.gradient-multi`

---

**Questions?** All files are commented. Read the inline documentation for more details!
