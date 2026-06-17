<div align="center">

# >_ MobileCLI

### Your desktop terminal, in your pocket.

Stream Claude Code, Codex, Gemini CLI, and any terminal session to your phone in real time.
Approve tool calls from the couch. Browse and edit files on your dev machine from anywhere.

[![GitHub stars](https://img.shields.io/github/stars/MobileCLI/mobilecli?style=flat&color=0A84FF)](https://github.com/MobileCLI/mobilecli)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?color=0A84FF)](LICENSE)
[![Rust](https://img.shields.io/badge/daemon-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey.svg)]()
[![iOS](https://img.shields.io/badge/iOS-App_Store-0A84FF.svg)](https://apps.apple.com/us/app/mobilecli/id6757689455)
[![Twitter](https://img.shields.io/twitter/follow/mobilecli?style=flat&color=0A84FF)](https://x.com/mobilecli)

[Website](https://mobilecli.app) · [Download CLI](https://github.com/MobileCLI/mobilecli/releases) · [iOS App](https://apps.apple.com/us/app/mobilecli/id6757689455)

</div>

<br/>

<div align="center">
<table>
<tr>
<td align="center"><img src="docs/screenshot-terminal.png" width="200"/><br/><sub>Live Terminal</sub></td>
<td align="center"><img src="docs/screenshot-sessions.png" width="200"/><br/><sub>Session Management</sub></td>
<td align="center"><img src="docs/screenshot-new-session.png" width="200"/><br/><sub>Spawn from Phone</sub></td>
</tr>
<tr>
<td align="center"><img src="docs/screenshot-files.png" width="200"/><br/><sub>Remote File Browser</sub></td>
<td align="center"><img src="docs/screenshot-editor.png" width="200"/><br/><sub>Code Editor</sub></td>
<td align="center"><img src="docs/screenshot-config.png" width="200"/><br/><sub>Configuration</sub></td>
</tr>
</table>
</div>

> 📺 **[Watch the 60-second demo ](https://x.com/AlexanderKnigge/status/2054322896760357124)**

<br/>

## Why MobileCLI exists

You kick off Claude Code on a large refactor. You go make coffee. You come back 20 minutes later and discover it's been blocked on a tool approval since minute two.

This happens constantly with AI coding assistants. They're powerful but need a human in the loop. That human doesn't need to be chained to a desk.

**MobileCLI streams your terminal to your phone over a network path you control.** When your AI assistant asks a question, requests tool access, or finishes a task, you get a push notification. Tap it, read the context, approve or deny, and go back to what you were doing.

The terminal stream is served by a local daemon over WebSocket. Mobile clients pair with auth-v2 QR credentials and then prove possession with a challenge-response handshake before the daemon sends sessions, terminal output, filesystem data, or push-token registration. There is no MobileCLI terminal relay or account system, but push notifications are delivered through Expo's push service, and the daemon should still only be reachable from a trusted LAN, Tailscale network, or protected custom endpoint.

<br/>

## Getting started

### 1. Install the daemon

```bash
curl -fsSL https://mobilecli.app/install.sh | bash
```

This macOS/Linux installer downloads the matching GitHub Release archive, verifies it against that release's `SHA256SUMS.txt` manifest before extraction, and puts the binary on your PATH. Windows users should install from the GitHub Releases `.zip` or with Cargo. The checksum protects against a corrupted or tampered archive relative to the published release manifest; for stronger supply-chain assurance, inspect the source or build from source with Cargo.

<details>
<summary>Other install methods</summary>

```bash
# From crates.io
cargo install mobilecli

# From source
git clone https://github.com/MobileCLI/mobilecli.git
cd mobilecli/cli && cargo install --path .

# Pre-built binaries (Linux x86_64/aarch64, macOS x86_64/arm64)
# → https://github.com/MobileCLI/mobilecli/releases
```
</details>

### 2. Pair your phone

```bash
mobilecli setup
```

This starts the daemon, saves your connection mode, creates a fresh mobile credential, and displays a QR code. Open the MobileCLI iOS app, tap **Scan QR Code**, and you're connected. The QR encodes the `ws://` or `wss://` URL, device id/name metadata, credential id, server id, and one-time pairing token. The desktop stores only a derived verifier, not the raw token.

### 3. Start a session

```bash
mobilecli claude                    # Claude Code
mobilecli codex                     # OpenAI Codex
mobilecli -n "DB Migration" claude  # Named session
mobilecli bash                      # Plain shell
mobilecli                           # Your default $SHELL
```

Your phone now shows the terminal output in real time. Walk away.

<br/>

## How it works

MobileCLI has two components: a **Rust daemon** that runs on your dev machine, and a **React Native app** on your phone.

```
Your Machine                                     Your Phone
┌─────────────────────────────────┐              ┌───────────────────────────┐
│  mobilecli daemon (Rust)        │  WebSocket   │  MobileCLI App            │
│  ┌────────────────────────────┐ │◄────────────►│                           │
│  │ PTY Manager                │ │   LAN or     │  ┌─ Sessions tab          │
│  │  session 1: claude code    │ │   Tailscale  │  │   Live xterm.js        │
│  │  session 2: codex          │ │              │  │   Touch keyboard       │
│  │  session 3: bash           │ │              │  │   Push notifications   │
│  └────────────────────────────┘ │              │  │                         │
│  ┌────────────────────────────┐ │              │  ├─ Files tab (Pro)       │
│  │ File System Bridge         │ │              │  │   Browse, edit, search │
│  │  read / write / search     │ │              │  │   Create, delete, copy │
│  │  git status integration    │ │              │  │                         │
│  └────────────────────────────┘ │              │  └─ Config tab            │
│  ┌────────────────────────────┐ │              │     Theme, notifications  │
│  │ CLI Detection Engine       │ │              │     Connection settings   │
│  │  claude, codex, gemini,    │ │              │                           │
│  │  opencode — auto-detected  │ │              └───────────────────────────┘
│  └────────────────────────────┘ │
└─────────────────────────────────┘
          │
    Port 9847 (default)
    Trusted network required
    Protect from untrusted clients
```

The daemon allocates a PTY (pseudo-terminal) for each session, streams the byte output over WebSocket, and relays keyboard input from your phone back to the PTY. The mobile app renders the stream using a bundled xterm.js instance — full ANSI color, cursor positioning, and alternate screen buffer support.

<br/>

## Core features

### Multi-session management

Run multiple AI assistants simultaneously. The Sessions tab shows all active and historical sessions with live status indicators. Long-press to rename or close sessions. Tap the **+** button to spawn a new CLI directly from your phone — Claude Code, OpenAI Codex, or a plain shell.

Sessions persist across daemon restarts. If you close the app and come back, your sessions are still there with full scrollback history.

### Smart CLI detection

The daemon automatically identifies which AI assistant is running in each session and parses its output to detect wait states — tool approval prompts, plan reviews, questions.

| CLI | Wait-state Detection | What Gets Detected |
|-----|---------------------|--------------------|
| **Claude Code** | ANSI output parsing | Tool calls, plan reviews, questions, completion |
| **OpenAI Codex** | Output pattern matching | Approval prompts, completion signals |
| **Gemini CLI** | Prompt detection | Yes/No prompts, input requests |
| **Shell** | Generic | Full terminal with manual interaction |

When a wait state is detected, the daemon fires a push notification to your phone. You don't need to keep the app open or watch the session — you'll be alerted the moment your attention is needed.

### Real-time terminal

The terminal view is a full xterm.js 5.3 instance running inside a WebView:

- **256-color ANSI** rendering with correct cursor positioning
- **Scrollable history** — scroll up through output, auto-scroll follows new content at the bottom
- **Touch keyboard** with a toolbar providing Esc, arrow keys (for CLI history), paste, and file attachment buttons
- **Responsive resize** — terminal dimensions adapt to your phone/tablet screen and send the new size to the PTY so output reflows correctly
- **Desktop-safe by default** — mobile resizing changes the child PTY dimensions without forcing your desktop terminal window to physically resize (`MOBILECLI_DESKTOP_RESIZE_POLICY=mirror` restores legacy mirroring)
- **Low latency** — WebSocket streaming over LAN is typically sub-10ms

### File browser and editor *(Pro)*

The Files tab gives you direct access to your dev machine's filesystem:

- **Browse** directories with breadcrumb navigation, file sizes, and modification times
- **Search** files by name across your entire project tree
- **Edit** files with a built-in editor featuring Save/Undo/Redo, Markdown formatting shortcuts (Bold, Italic, Code, H1, List, Link), and syntax awareness
- **Create** new files and folders from your phone
- **Destructive actions stay opt-in** — delete and rename are disabled by default in the daemon config and must be explicitly enabled during setup or config review
- **Upload** photos, files, or camera captures from your phone to your dev machine — the daemon saves them and returns the desktop path so you can paste it into your terminal
- **Git integration** — file listings show git status indicators

### Push notifications

Notifications are delivered through Expo's push notification service for the current iOS app. The daemon sends a push when:

- An AI CLI enters a wait state (tool approval, plan review, question)
- A session finishes or exits
- A long-running command completes

The push token is registered over the WebSocket connection and then used by the daemon to call Expo's push API. Notification payloads include the notification title/body and session id, not the full terminal stream.

<br/>

## Privacy and security

MobileCLI keeps the terminal streaming path self-hosted, but the current iOS push-notification path uses Expo's cloud push service.

- **No MobileCLI terminal relay.** Your terminal output is served by the daemon over your configured network path.
- **No accounts.** No sign-up, no email, no OAuth.
- **No telemetry.** The daemon collects nothing.
- **Auth-v2 pairing.** Each mobile app stores a pairing token in SecureStore and authenticates with a challenge-response proof before receiving sessions or terminal data. Use `mobilecli pair --rotate` or `mobilecli credentials revoke <credential_id>` to replace or revoke mobile access.
- **Network isolation still matters.** Keep port `9847` on a trusted LAN, Tailnet, firewall allowlist, or protected custom endpoint. Do not expose it directly to the public internet.
- **Bounded resources.** The daemon limits concurrent connections, channel buffer sizes, and session counts to prevent resource exhaustion.

Your terminal stream does not go through MobileCLI-operated servers. If push notifications are enabled, Expo receives the notification title/body and session id. If you configure Tailscale or a custom remote URL, traffic follows that network provider or endpoint.

<br/>

## Connection modes

| Mode | How it works | Setup |
|------|-------------|-------|
| **LAN** | WebSocket over your trusted WiFi/ethernet. Fastest and simplest. | Auto-detected during `mobilecli setup` |
| **Tailscale** | WireGuard-based mesh VPN. Access from your Tailnet without opening the daemon to the public internet. | `mobilecli setup` → select your Tailscale IP |
| **Custom URL** | Your own protected `ws://` or `wss://` endpoint, such as a private reverse proxy or TLS terminator. | Provide the URL during setup |

For most users, LAN mode is all you need. Open a terminal, scan the QR, done.

<br/>

## CLI reference

```
mobilecli [OPTIONS] [COMMAND]

Session commands:
  mobilecli                           Start default shell with streaming
  mobilecli <command>                 Run any command with streaming
  mobilecli -n "Name" <command>       Name the session for easy identification
  mobilecli link [session-id]         Attach to an existing session (tmux-like)

Setup and management:
  mobilecli setup                     Interactive setup wizard (generates QR code)
  mobilecli pair                      Show QR code for pairing additional devices
  mobilecli pair --rotate             Revoke existing mobile credentials and pair again
  mobilecli credentials list          List paired mobile credentials without secrets
  mobilecli credentials revoke <id>   Revoke one paired mobile credential
  mobilecli status                    Show daemon status, active sessions, connections
  mobilecli stop                      Stop the daemon
  mobilecli uninstall                 Remove MobileCLI completely (daemon, autostart, hook, config, binary)

Daemon lifecycle:
  mobilecli daemon [--port PORT]      Start daemon manually (default port: 9847)
  mobilecli autostart install         Auto-start daemon on login
  mobilecli autostart uninstall       Remove auto-start
  mobilecli autostart status          Check auto-start status

Shell integration:
  mobilecli shell-hook install        Auto-launch mobilecli in every new terminal
  mobilecli shell-hook uninstall      Remove the shell hook
  mobilecli shell-hook status         Check shell hook status
```

### Daemon autostart

The daemon can register itself to start automatically when you log in:

| Platform | Mechanism | Command |
|----------|-----------|---------|
| **Linux** | systemd user service | `mobilecli autostart install` |
| **macOS** | launchd agent | `mobilecli autostart install` |
| **Windows** | Task Scheduler | `mobilecli autostart install` |

> **Windows Note:** See [docs/WINDOWS_SETUP.md](docs/WINDOWS_SETUP.md) for important details about running in user session for visible terminal windows.

### Shell hook

To automatically wrap every new terminal session:

```bash
mobilecli shell-hook install
```

This adds a one-liner to your `.bashrc`, `.zshrc`, `config.fish`, or PowerShell `$PROFILE`. Every new shell you open will be streamed to your phone automatically. Bypass it temporarily:

```bash
MOBILECLI_NO_AUTO_LAUNCH=1 bash
```

### Uninstall

To remove MobileCLI completely, run the built-in uninstaller:

```bash
mobilecli uninstall
```

This stops the daemon, removes daemon autostart (systemd / launchd / Task Scheduler), strips the shell auto-launch hook from your shell config, deletes the config directory (`~/.mobilecli`, including paired credentials), and removes the binary. Use `--keep-config` to preserve `~/.mobilecli`, `--keep-binary` to leave the binary in place, and `-y`/`--yes` to skip the confirmation prompt.

If the binary is missing or broken, you can run the standalone uninstall script instead, which delegates to `mobilecli uninstall` when available and otherwise performs the same cleanup manually:

```bash
curl -fsSL https://raw.githubusercontent.com/MobileCLI/mobilecli/main/uninstall.sh | bash
```

<br/>

## Platform support

### CLI daemon

| Platform | Architecture | Status |
|----------|-------------|--------|
| **Linux** | x86_64, aarch64 | Fully supported |
| **macOS** | Intel, Apple Silicon | Fully supported |
| **Windows** | x86_64 | Fully supported |

### Mobile app

| Platform | Status |
|----------|--------|
| **iOS** (iPhone + iPad) | Available on the App Store — [Download MobileCLI](https://apps.apple.com/us/app/mobilecli/id6757689455) |
| **Android** | In development |

<br/>

## Pricing

The CLI daemon is **open source and free forever** (MIT license).

The mobile app has a free tier and an optional Pro upgrade:

| | Free | Pro |
|---|---|---|
| Live terminal streaming | Unlimited sessions | Unlimited sessions |
| Push notifications | Included | Included |
| Multi-session management | Included | Included |
| Spawn sessions from phone | Included | Included |
| Rename / close sessions | Included | Included |
| Multiple themes | Included | Included |
| File browser & editor | — | Included |
| Full-text file search | — | Included |
| Photo / file upload to desktop | — | Included |
| | **Free** | **$19.99/yr** or **$29.99 lifetime** |

<br/>

## Configuration

All config lives in `~/.mobilecli/`:

| File | Purpose |
|------|---------|
| `config.json` | Device identity and connection mode/URL |
| `sessions.json` | Persisted session metadata (names, history) |
| `daemon.pid` | Running daemon's process ID |
| `daemon.port` | Active WebSocket port (default: `9847`) |
| `daemon.log` | Debug log output |

<br/>

## Development

### CLI (Rust)

```bash
cd cli
cargo build                     # Debug build
cargo run -- setup              # Run setup wizard
RUST_LOG=debug cargo run        # Verbose logging
cargo test                      # Run tests
cargo clippy                    # Lint
```

The daemon is ~7,000 lines of async Rust built on `tokio`. Key modules:

| Module | Lines | Role |
|--------|-------|------|
| `daemon.rs` | 2,700 | WebSocket server, session lifecycle, file system bridge |
| `protocol.rs` | 550 | All client/server message types (serde JSON) |
| `shell_hook.rs` | 530 | Cross-platform shell integration (bash/zsh/fish/PowerShell) |
| `autostart.rs` | 560 | systemd / launchd / Task Scheduler registration |
| `pty_wrapper.rs` | 490 | PTY allocation, I/O streaming, signal handling |
| `detection.rs` | 390 | AI CLI fingerprinting and wait-state parsing |
| `setup.rs` | 570 | Interactive wizard, QR generation, network detection |
| `main.rs` | 450 | CLI argument parsing and command dispatch |

### Mobile app (React Native / Expo)

The mobile app is in a [separate repository](https://github.com/MobileCLI/mobile):

```bash
cd mobile
npm install
npx expo start                  # Dev server (press 'i' for iOS simulator)
npx eas-cli build --platform ios --profile production  # Production build
```

Key technologies: Expo Router (navigation), xterm.js 5.3 (terminal rendering in WebView), expo-secure-store (credential storage), RevenueCat (subscriptions).

### Project layout

```
MobileCLI/
├── cli/                        # Rust daemon + CLI wrapper
│   └── src/
│       ├── main.rs             # Entry point, clap argument parsing
│       ├── daemon.rs           # WebSocket server, PTY management, filesystem ops
│       ├── protocol.rs         # Client ↔ Server message types
│       ├── pty_wrapper.rs      # PTY spawning and byte-level I/O
│       ├── detection.rs        # AI CLI detection + wait-state parsing
│       ├── setup.rs            # Interactive setup wizard + QR code
│       ├── shell_hook.rs       # Shell auto-launch integration
│       ├── autostart.rs        # OS-level daemon autostart
│       ├── uninstall.rs        # Native uninstaller (reverses install + setup)
│       ├── link.rs             # Session attachment (tmux-style)
│       ├── session.rs          # Session metadata structures
│       ├── platform.rs         # Cross-platform utilities
│       └── qr.rs               # QR code rendering
├── mobile/                     # React Native app (separate git repo)
├── website/                    # Marketing site (Astro + Tailwind)
├── install.sh                  # One-line installer script
├── uninstall.sh                # One-line uninstaller script
├── .github/workflows/          # CI, release packaging, Claude Code review
└── docs/                       # Architecture docs + screenshots
```

<br/>

## Troubleshooting

<details>
<summary><b>Can't connect from mobile app</b></summary>

1. **Same network?** Your phone and machine must be on the same WiFi/LAN, or both on Tailscale.
2. **Daemon running?** Run `mobilecli status` to check. If not running, `mobilecli daemon` starts it.
3. **Firewall?** Ensure port `9847` (or whatever `~/.mobilecli/daemon.port` says) allows inbound TCP.
4. **Re-pair:** Run `mobilecli pair` to show a fresh QR code and scan it again.
5. **Check logs:** `~/.mobilecli/daemon.log` will show connection attempts and connection errors.
</details>

<details>
<summary><b>No push notifications</b></summary>

1. Verify notifications are enabled for MobileCLI in iOS Settings.
2. The push token registers automatically after the WebSocket auth-v2 handshake completes — check the Config tab shows "connected" status.
3. Notifications require the daemon to be running. If you restart your machine, make sure the daemon is back up (`mobilecli autostart install` handles this automatically).
</details>

<details>
<summary><b>Terminal display issues</b></summary>

1. MobileCLI uses xterm.js with full ANSI 256-color support. Ensure your CLI sets `TERM=xterm-256color` (this is the default).
2. The terminal auto-resizes to fit your phone screen. TUI applications (like `htop` or `vim`) should adapt automatically.
3. Desktop terminal geometry is preserved by default. If you explicitly want mirrored desktop window resizing, launch with `MOBILECLI_DESKTOP_RESIZE_POLICY=mirror`.
4. On Linux, tmux mouse mode is disabled by default so desktop terminals like Konsole keep normal drag-select clipboard behavior. Re-enable tmux mouse features with `MOBILECLI_TMUX_MOUSE=on mobilecli`.
5. If a session looks garbled after switching tabs, tap the session to re-enter it — the terminal refits on activation.
</details>

<details>
<summary><b>Session not appearing on phone</b></summary>

1. Sessions only appear when the daemon is running and your phone is connected.
2. Check `mobilecli status` to see active sessions and connected clients.
3. If you started a command without `mobilecli` wrapping it, it won't appear. Use `mobilecli <command>` or install the shell hook.
</details>

<br/>

## Contributing

Contributions are welcome. The CLI daemon is open source under the MIT license.

```bash
# Fork, clone, and create a feature branch
git clone https://github.com/YOUR_USERNAME/mobilecli.git
cd mobilecli/cli

# Build and test
cargo build
cargo test
cargo clippy -- -D warnings

# Open a PR against main
```

Claude Code review is enabled on this repository — your PR will receive automated feedback.

<br/>

## License

MIT — see [LICENSE](LICENSE) for details.

<br/>

<div align="center">

---

**Stop babysitting your AI assistant.** Start it, walk away, and get a push when it needs you.

[Get the CLI](https://mobilecli.app) · [Download on the App Store](https://apps.apple.com/us/app/mobilecli/id6757689455) · [GitHub](https://github.com/MobileCLI/mobilecli)

</div>
