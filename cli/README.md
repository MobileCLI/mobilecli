# MobileCLI - Stream Any Terminal to Your Phone

A simple CLI tool that streams any terminal session to your phone. No separate app window needed - just run `mobilecli` and your terminal is instantly accessible from your mobile device.

## Installation

```bash
# Build from source
cd cli
cargo build --release

# Install to your path
cp target/release/mobilecli ~/.local/bin/
```

## Quick Start

```bash
# First time setup (shows QR code for mobile pairing)
mobilecli setup

# Start your shell with mobile streaming
mobilecli
```

That's it! Your terminal is now accessible from your phone.

## Usage

```bash
# Name your session (shows up in mobile app)
mobilecli -n "Work Terminal"
mobilecli -n "AI Chat" claude

# Quiet mode (skip connection status)
mobilecli --quiet

# Show active sessions
mobilecli status

# Show pairing QR code again
mobilecli pair
```

## Commands

| Command | Description |
|---------|-------------|
| `mobilecli` | Start your default shell with streaming |
| `mobilecli setup` | Run setup wizard and show pairing QR code (`mobilecli --setup` alias) |
| `mobilecli status` | Show daemon status and active sessions |
| `mobilecli pair` | Show QR code for mobile pairing |
| `mobilecli stop` | Stop the background daemon |

## Options

| Option | Description |
|--------|-------------|
| `--setup` | Run setup wizard and show pairing QR code |
| `-n, --name <NAME>` | Name for this session (shown in mobile app) |
| `-q, --quiet` | Don't show connection status on startup |

Connection mode (Local/Tailscale/Custom) is configured via `mobilecli setup` (or `mobilecli --setup`).

## How It Works

1. **Setup**: Run `mobilecli setup` (or `mobilecli --setup`) to configure and optionally scan QR code with mobile app
2. **Daemon**: A background daemon starts automatically and manages all sessions
3. **Sessions**: Each `mobilecli` terminal registers with the daemon
4. **Mobile**: Connect once to see all active terminal sessions
5. **Streaming**: Terminal output streams to mobile, input flows back

```
Terminal 1 ──┐
Terminal 2 ──┼──► Daemon (port 9847) ◄──► Mobile App
Terminal 3 ──┘
```

## Mobile App

Scan the QR code with the MobileCLI mobile app during setup. If you cannot scan the QR code, use `mobilecli pair` and enter the full manual pairing details in app settings: WebSocket URL, credential id, server id, and pairing token. URL-only manual setup is not enough for auth-v2 daemons.

## Session Management

Active sessions are managed by the daemon. Use `mobilecli status` to see them:

```bash
$ mobilecli status
● Daemon running (PID: 12345, port: 9847)

Sessions: 2 active session(s):
  → claude - /bin/bash
  → Work Terminal - /bin/bash
```

## Security Model

MobileCLI uses auth-v2 QR pairing plus constrained network binds:

- **QR Pairing Credential**: `mobilecli setup` (or `mobilecli --setup`) creates a fresh mobile credential and embeds the daemon URL, device id/name, server id, credential id, and one-time pairing token in the QR code. The desktop config stores only a derived verifier.
- **Challenge-response auth**: mobile clients must send `auth_start`, answer `auth_challenge`, and prove possession of the pairing token before receiving `welcome`, sessions, terminal output, filesystem data, or push-token registration.
- **Credential management**: use `mobilecli credentials list`, `mobilecli credentials revoke <credential_id>`, or `mobilecli pair --rotate` to manage paired devices.
- **Constrained bind policy**: the daemon always binds loopback for desktop traffic, then binds the configured LAN or Tailscale address for mobile access. It does not silently bind `0.0.0.0`.
- **Network isolation**: auth is not a reason to expose the daemon directly to the public internet. For remote access, use Tailscale or a protected `wss://` endpoint you operate.

## Protocol

The WebSocket server uses a JSON protocol compatible with the MobileCLI mobile app:

### Client → Server

- `auth_start` - Begin auth-v2 challenge-response pairing proof
- `auth_response` - Complete auth-v2 proof
- `send_input` - Send keyboard input
- `pty_resize` - Resize terminal (cols, rows)
- `get_sessions` - List available sessions
- `rename_session` - Rename a session
- `spawn_session` - Start a new terminal session from mobile
- `ping` - Heartbeat

### Server → Client

- `auth_challenge` - Auth-v2 server challenge
- `welcome` - Connection established
- `session_info` - Session details
- `pty_bytes` - Terminal output (base64)
- `sessions` - List of sessions
- `session_ended` - Session terminated
- `session_renamed` - Rename confirmation
- `spawn_result` - Result of spawn_session request
- `waiting_for_input` - Tool approval or input prompt detected
- `pong` - Heartbeat response

## Troubleshooting

If the daemon fails to start, check the log file:

```bash
cat ~/.mobilecli/daemon.log
```

If desktop drag-select copy is not working while tmux runtime is active:

- On Linux, tmux mouse is now off by default to preserve terminal clipboard selection.
- To explicitly enable tmux mouse behavior (scroll/click/copy-mode), launch with:

```bash
MOBILECLI_TMUX_MOUSE=on mobilecli
```

- To force clipboard-first behavior on any OS, launch with:

```bash
MOBILECLI_TMUX_MOUSE=off mobilecli
```

## License

MIT
