# EWW Configuration

Modern EWW configuration with Rust applets for better performance and native API access.

## Quick Start

```bash
# Start EWW
go-task restart

# Check daemon status
go-task status

# Development mode (auto-reload on file changes)
go-task dev
```

## Task Commands

### EWW Management
- `go-task reload` - Reload config (rebuilds Rust applets first)
- `go-task restart` - Full restart (kill + rebuild + start)
- `go-task open` - Open EWW bar
- `go-task close` - Close all windows
- `go-task kill` - Kill EWW and all daemons
- `go-task status` - Show daemon status
- `go-task logs` - Show EWW logs

### Build
- `go-task build` - Build all Rust applets
- `go-task build:battery` - Build specific applet
- `go-task clean` - Clean build artifacts

### Development
- `go-task dev` - Watch mode (auto-reload)
- `go-task check` - Check all Rust code
- `go-task fmt` - Format Rust code

### Testing
- `go-task test:battery` - Test battery monitor
- `go-task test:pomodoro` - Test pomodoro timer
- `go-task test:workspaces` - Test workspace monitor

## Architecture

### Rust Applets (Native Performance)
- **eww-battery** - Direct sysfs, auto-updates via `deflisten`
- **eww-workspaces** - Swayipc real-time events
- **eww-window-title** - Swayipc real-time events
- **eww-pomodoro** - Auto-starting daemon with Unix socket
- **eww-volume-mixer** - PulseAudio native bindings
- **eww-microphone-mixer** - PulseAudio native bindings
- **eww-music-daemon** - MPRIS D-Bus integration

### Auto-Starting Daemons
Daemons start automatically when EWW launches them:
- Pomodoro daemon auto-forks when first accessed
- No startup scripts needed!

## Directory Structure
```
.config/eww/
├── eww.yuck          # Main widgets
├── config.yuck       # Data sources
├── Taskfile.yml      # Task runner
├── styles/           # SCSS themes
├── rust-applets/     # Rust programs
└── scripts/          # Legacy scripts
```

## Benefits
- **Native APIs** - Direct sysfs, swayipc, PulseAudio
- **Event-Driven** - Real-time updates, no polling
- **Auto-Management** - Daemons start themselves
- **Type Safe** - Rust compile-time guarantees
- **Performance** - Compiled vs interpreted

## Troubleshooting

### Daemon not starting
```bash
go-task kill
go-task restart
```

### Build errors
```bash
go-task clean
go-task build
```

Use `go-task --list` to see all available commands.
