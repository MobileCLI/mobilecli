# MobileCLI

**Control AI coding assistants from your phone.** Stream Claude Code, Codex, Gemini CLI, and any terminal session to your mobile device. Approve tool calls, answer questions, and monitor progress from anywhere.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![Expo](https://img.shields.io/badge/Expo-SDK_52-black.svg)](https://expo.dev/)

**Website:** https://mobilecli.app
**Releases:** https://github.com/MobileCLI/mobilecli/releases

---

## Why MobileCLI?

AI coding assistants like Claude Code run long tasks that need periodic human approval. Without MobileCLI, you're stuck at your desk waiting for prompts. With MobileCLI:

- **Walk away from your desk** - Get push notifications when Claude needs you
- **Approve from anywhere** - One-tap approve/deny for tool calls and plans
- **Monitor progress** - Full terminal output streamed to your phone
- **Stay in control** - Type responses, send escape sequences, interact fully

## Features

### Smart CLI Detection
MobileCLI automatically detects which AI assistant you're running and adapts:
- **Claude Code** - Numbered option approval (1/2/3)
- **Codex** - Numbered option approval
- **Gemini CLI** - Yes/No prompts
- **OpenCode** - Arrow key navigation

### Push Notifications
Get notified instantly when your CLI needs attention:
- Tool approval requests
- Plan review prompts
- Questions requiring your input
- Generic "awaiting response" states

### Real-time Terminal Streaming
- Full ANSI color and formatting support
- Scroll through output history
- Responsive resizing for mobile screens
- Low-latency WebSocket connection

### Privacy-First Architecture
- **100% self-hosted** - No cloud relay, no third-party servers
- **Direct connection** - Phone connects to your machine over LAN or Tailscale
- **No data collection** - Your terminal output never leaves your network

## Quick Start

### 1. Install the CLI

**Quick Install (recommended):**
```bash
curl -fsSL https://raw.githubusercontent.com/MobileCLI/mobilecli/main/install.sh | bash
```

**Alternative methods:**
```bash
# From crates.io
cargo install mobilecli

# From source
git clone https://github.com/MobileCLI/mobilecli.git
cd mobilecli/cli
cargo install --path .
```

**Manual download:** [GitHub Releases](https://github.com/MobileCLI/mobilecli/releases)

### 2. Run Setup

```bash
mobilecli setup
```

This starts the background daemon and displays a QR code for pairing. (`mobilecli --setup` is an alias.)

### 3. Install the Mobile App

- **iOS**: [App Store](https://apps.apple.com/app/mobilecli) (coming soon)
- **Android**: [Google Play](https://play.google.com/store/apps/details?id=app.mobilecli) (coming soon)

Scan the QR code to connect.

### 4. Start Streaming

```bash
# Stream your default shell
mobilecli

# Stream a specific command
mobilecli claude

# Name your session
mobilecli -n "Backend refactor" claude
```

## Usage Examples

### Claude Code
```bash
mobilecli claude
# Your phone shows the terminal output
# When Claude asks for tool approval, you get a notification
# Tap Approve/Always/Deny on your phone
```

### Codex
```bash
mobilecli codex
# Same workflow - notifications and one-tap approval
```

### Any Command
```bash
# Long-running builds
mobilecli -n "Build" npm run build

# Interactive scripts
mobilecli python script.py

# Remote sessions
mobilecli ssh server
```

## Architecture

```
┌─────────────────┐     WebSocket      ┌─────────────────┐
│                 │◄──────────────────►│                 │
│   CLI Daemon    │                    │   Mobile App    │
│   (port 9847)   │                    │                 │
│                 │                    │                 │
└────────┬────────┘                    └─────────────────┘
         │
         │ PTY
         ▼
┌─────────────────┐
│                 │
│  claude/codex   │
│  gemini/etc     │
│                 │
└─────────────────┘
```

**Components:**
- **Daemon** - Background WebSocket server managing PTY sessions
- **Wrapper** - Spawns commands in a PTY, streams to daemon
- **Mobile App** - React Native/Expo app with xterm.js terminal

## CLI Reference

```
mobilecli                    # Start your shell with streaming
mobilecli <command>          # Run command with streaming
mobilecli -n "Name" <cmd>    # Name the session
mobilecli setup              # Run setup wizard, show QR code
mobilecli status             # Show daemon and session status
mobilecli pair               # Show QR code for pairing
mobilecli link               # Link to an existing session (like screen -x)
mobilecli link <session>     # Link to specific session by name/ID
mobilecli daemon             # Start daemon manually
mobilecli stop               # Stop the daemon
```

## Configuration

Config stored in `~/.mobilecli/config.json`:

```json
{
  "device_id": "uuid-string",      // Unique device identifier
  "device_name": "MacBook-Pro",    // Display name (hostname by default)
  "connection_mode": "local",      // "local" or "tailscale"
  "local_ip": "192.168.1.100",
  "tailscale_ip": null,
  "auth_token": "uuid-string"      // Optional QR pairing token
}
```

**Connection Modes:**
- **Local** - Connect over your WiFi network (same LAN required)
- **Tailscale** - Connect over Tailscale VPN (works from anywhere)

**Multi-Device Support:**
The mobile app supports linking multiple computers. Each computer gets a unique `device_id` on first setup. Scanning a new QR code adds that device to your linked devices list.

## Mobile App Features

- **Session List** - See all active terminal sessions
- **Live Terminal** - Full terminal emulation with touch keyboard
- **Quick Actions** - Approve/Deny buttons for tool calls
- **Push Notifications** - Background alerts when CLI needs attention
- **Dark Theme** - Easy on the eyes, matches terminal aesthetic

## Troubleshooting

### Can't connect from mobile app

1. **Same network?** Ensure phone and computer are on same WiFi
2. **Firewall?** Allow port 9847 (or check `~/.mobilecli/daemon.port`)
3. **Daemon running?** Run `mobilecli status` to check
4. **Pairing metadata (optional):** Re-scan the QR code (`mobilecli pair`) to refresh saved device details

### No push notifications

1. **Permissions?** Check notification permissions in iOS/Android settings
2. **Token registered?** App should auto-register on connect
3. **Daemon logs?** Check `~/.mobilecli/daemon.log`

### Terminal looks wrong

1. **Font?** Mobile uses system monospace font
2. **Size?** Terminal adapts to phone screen, some content may wrap
3. **Colors?** Full ANSI 256-color support enabled

## Development

### CLI (Rust)
```bash
cd cli
cargo build
cargo run -- --setup
RUST_LOG=debug cargo run -- claude  # With debug logging
```

### Mobile (React Native/Expo)
```bash
cd mobile
npm install
npx expo start
# Press 'i' for iOS simulator, 'a' for Android emulator
```

### Project Structure
```
MobileCLI/
├── cli/                 # Rust CLI and daemon
│   ├── src/
│   │   ├── main.rs      # Entry point, CLI args
│   │   ├── daemon.rs    # WebSocket server, PTY management
│   │   ├── detection.rs # CLI type detection, wait state parsing
│   │   ├── protocol.rs  # WebSocket message types
│   │   ├── pty_wrapper.rs
│   │   ├── session.rs
│   │   └── setup.rs
│   └── Cargo.toml
├── mobile/              # React Native app (Expo)
│   ├── app/             # Expo Router screens
│   ├── components/      # UI components
│   ├── hooks/           # useSync, useSettings, etc.
│   └── package.json
├── docs/                # Documentation
└── website/             # Marketing site (Astro)
```

## Contributing

Contributions welcome! Please:

1. Fork the repo
2. Create a feature branch
3. Make your changes
4. Run `cargo test` and `cargo clippy`
5. Submit a PR

## License

MIT License - see [LICENSE](LICENSE) for details.

---

**Built for developers who use AI coding assistants and want freedom from their desk.**
