# PingZilla

> *When your latency needs a monster to keep it in check*

A lightweight macOS menu bar application that monitors your network latency in real-time. Built with Tauri 2, React, and Rust.

## Features

- **Live Menu Bar Display** - Current ping displayed right in your menu bar
- **Real-Time Graph** - Visual history of the last 2 minutes of ping data
- **Smart Notifications** - Get alerted when latency exceeds your threshold (default: 400ms)
- **Configurable Target** - Ping any host (default: 8.8.8.8)
- **Persistent History** - Stores up to 24 hours of ping data locally
- **Smooth Animations** - Buttery smooth number transitions
- **Native Performance** - Rust backend with minimal resource usage
- **No Root Required** - Uses system ping command

## Screenshot

*[Coming soon - add screenshot here]*

## Installation

### From Release

Download the latest `.dmg` from the [Releases](https://github.com/PiXeL16/Pingzilla/releases) page.

### Build from Source

#### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/)
- Xcode Command Line Tools

#### Steps

```bash
# Clone the repo
git clone https://github.com/PiXeL16/Pingzilla.git
cd Pingzilla

# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev

# Build for production
pnpm tauri build
```

The built app will be in `src-tauri/target/release/bundle/macos/`.

## Usage

1. **Launch** - PingZilla appears in your menu bar showing the current ping
2. **Click** the menu bar icon to open the detailed view with graph
3. **Right-click** to access the quit menu
4. **Settings** - Click "Settings" in the popup to configure:
   - **Target**: The host to ping
   - **Alert threshold**: Latency (in ms) that triggers notifications

## Tech Stack

- **Frontend**: React 19, TypeScript, Recharts
- **Backend**: Rust, Tauri 2
- **Build**: Vite, pnpm

## Color Indicators

| Color  | Latency    | Status    |
|--------|------------|-----------|
| Green  | < 50ms     | Excellent |
| Yellow | 50-150ms   | Good      |
| Red    | > 150ms    | Poor      |
| Gray   | Timeout    | No response |

## License

MIT

## Authors

Chriszilla & Claudio

---

*PingZilla: Stomping latency since 2024*
