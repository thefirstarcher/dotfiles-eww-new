# EWW Configuration

**DRY, abstraction-focused eww config following Occam's Razor principles**

## 🚀 Quick Start

```bash
# Switch theme
./scripts/switch-theme.sh ayu     # Warm & rounded
./scripts/switch-theme.sh cyber   # Cool & sharp

# Reload eww
eww reload
```

## 📖 Documentation

- **QUICK-START.md** - Common tasks (3 min read)
- **CONFIG-GUIDE.md** - Complete reference (15 min read)
- **REFACTORING-SUMMARY.md** - What was improved

## 📁 Structure

```
eww/
├── README.md              # This file
├── eww.yuck               # Widget definitions
├── config.yuck            # Configuration variables
├── eww.scss               # Style imports
├── scripts/
│   ├── switch-theme.sh    # Theme switcher
│   └── workspaces.sh      # Workspace data
└── styles/
    ├── theme-config.scss  # → Active theme (symlink)
    ├── _theme-*.scss      # Theme files
    ├── _*.scss            # Component styles
    └── _modifiers.scss    # Utility classes
```

## 🎨 Themes

- **Ayu Dark** - Warm orange accent, rounded corners, soft glows
- **Cyber Blue Sharp** - Cool cyan accent, sharp edges, neon glows

Switch with: `./scripts/switch-theme.sh [ayu|cyber]`

## 🔧 Customization

### Bar Size/Position
Edit `eww.yuck` → `defwindow bar` section

### Widget Visibility
Edit `config.yuck` → Change `show-*` variables

### Colors/Sizes
Edit `styles/_theme-*.scss` → Modify values

### Add Widget
1. Create widget in `eww.yuck`
2. Create style in `styles/_widget.scss`
3. Import in `eww.scss`

## 🎯 Principles

✓ **DRY** - Single source of truth for all values
✓ **Abstraction** - Reusable, composable components
✓ **Occam's Razor** - Minimal complexity, no over-engineering

## 📚 More Info

See **CONFIG-GUIDE.md** for detailed documentation
