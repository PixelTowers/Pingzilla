# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Development (runs Vite on :1420 + Rust backend)
pnpm tauri dev

# Production build (outputs to src-tauri/target/release/bundle/macos/)
pnpm tauri build

# Frontend only (Vite dev server)
pnpm dev

# TypeScript check + Vite build
pnpm build
```

Note: Rust/Cargo must be in PATH. If builds fail with "cargo not found", run:
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

## Architecture

PingZilla is a macOS menu bar app built with Tauri 2 (Rust backend + React frontend).

### Communication Flow

```
React (src/App.tsx)
    │
    ├─→ invoke("command_name", {args})  →  Rust Tauri commands (src-tauri/src/lib.rs)
    │
    └─← listen("ping-update")  ←──────────  Rust emits events every 2 seconds
```

### Backend (Rust)

**`src-tauri/src/lib.rs`** - Core application logic:
- `AppState` - Shared state wrapped in `Arc<Mutex>` for thread-safe access
- `start_ping_service()` - Background task that pings every 2 seconds, updates tray title, emits events
- `do_ping()` - Executes system `ping -c 1 -W 2000` (no root required)
- `position_window_at_tray()` - Positions popup window below tray icon using logical coordinates
- Tauri commands: `get_current_ping`, `get_ping_history`, `set_ping_target`, `set_notification_threshold`, `get_settings`

**`src-tauri/capabilities/default.json`** - Permission scopes for tray, window, events, notifications, autostart

### Frontend (React)

**`src/App.tsx`** - Single-file React app:
- State: `currentPing`, `history` (last 60 samples), `target`, `threshold`, `launchAtLogin`
- `AnimatedNumber` component for smooth ping value transitions
- Recharts `LineChart` for 2-minute history visualization
- Settings panel with target input, threshold input, launch-at-login toggle

### Data Persistence

- History stored in `~/Library/Application Support/pingzilla/history.json` (macOS)
- Saves every 30 pings (~1 minute)
- Loads last 24 hours on startup

## Key Technical Decisions

- **System ping command** instead of raw sockets - avoids requiring root/capabilities
- **LaunchAgent** for autostart - uses `tauri-plugin-autostart` with `MacosLauncher::LaunchAgent`
- **Template icon** - `icon_as_template(true)` allows macOS to adapt icon color to menu bar theme
- **Logical coordinates** - Window positioning must convert physical to logical coordinates for proper display
- **No config-based tray** - Programmatic `TrayIconBuilder` only (config-based conflicts with click handlers)

## Latency Thresholds

| Latency | Color | Status |
|---------|-------|--------|
| < 50ms | Green (#22c55e) | Excellent |
| 50-150ms | Yellow (#eab308) | Good |
| > 150ms | Red (#ef4444) | Poor |
| Timeout | Gray (#888) | No response |

## Plugins

- `tauri-plugin-notification` - System notifications for high latency alerts
- `tauri-plugin-autostart` - Launch at login via macOS LaunchAgent
- `tauri-plugin-opener` - External URL handling
