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

1. **Setup**: Run `mobilecli setup` (or `mobilecli --setup`) to configure and scan QR code with mobile app
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

Scan the QR code with the MobileCLI mobile app during setup. The app connects to the daemon and shows all active terminal sessions.

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

MobileCLI uses a pairing token + network-level access control:

- **Pairing Token**: `mobilecli setup` (or `mobilecli --setup`) generates an `auth_token` and embeds it in the QR code. The mobile app sends it in the initial `hello` message.
- **Local Network**: Only devices on the same WiFi can connect
- **Tailscale**: Only authenticated Tailscale network members can connect

The daemon binds to all interfaces (0.0.0.0) intentionally so mobile devices can connect. For remote access, use Tailscale or terminate TLS (`wss://`) with a reverse proxy.

## Protocol

The WebSocket server uses a JSON protocol compatible with the MobileCLI mobile app:

### Client → Server

- `send_input` - Send keyboard input
- `pty_resize` - Resize terminal (cols, rows)
- `get_sessions` - List available sessions
- `rename_session` - Rename a session
- `spawn_session` - Start a new terminal session from mobile
- `ping` - Heartbeat

### Server → Client

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

## License

MIT
