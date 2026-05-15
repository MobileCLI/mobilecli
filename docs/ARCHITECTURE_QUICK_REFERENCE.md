# MobileCLI Architecture Quick Reference

## Core Components

### Desktop Daemon (Rust)
- **Port**: 9847 (WebSocket)
- **Config**: `~/.mobilecli/config.json`
- **Binary**: `/usr/local/bin/mobilecli`
- **Process**: Manages PTY sessions, WebSocket server

### Mobile App (React Native)
- **Framework**: Expo SDK 54
- **Terminal**: xterm.js
- **Storage**: expo-secure-store (device links)
- **Notifications**: expo-notifications (push)

## Connection Flow

```
1. Desktop: mobilecli setup (or mobilecli --setup)
   ↓
2. Generate auth-v2 QR: mobilecli://ip:9847?...credential_id=xxx&server_id=yyy&auth_token=zzz
   ↓
3. Mobile: Scan QR → Store device and pairing secret
   ↓
4. WebSocket: Connect over configured LAN/Tailscale/custom path
   ↓
5. Auth: auth_start → auth_challenge → auth_response
   ↓
6. Stream: PTY bytes ↔ Terminal display
```

## Key Protocol Messages

### Client → Server
```typescript
{
  type: "auth_start",         // Initial mobile handshake
  type: "auth_response",      // Challenge-response proof
  type: "hello",              // Legacy/no-op after auth
  type: "get_sessions",       // List active terminals
  type: "subscribe",          // Subscribe to session
  type: "send_input",         // Send terminal input
  type: "pty_resize",         // Resize terminal
  type: "tool_approval",      // Approve/deny AI tools
  type: "register_push_token" // Register for notifications
}
```

### Server → Client
```typescript
{
  type: "auth_challenge",    // Auth-v2 server challenge
  type: "welcome",           // Handshake response
  type: "sessions",          // Session list
  type: "pty_bytes",         // Terminal output (base64)
  type: "waiting_for_input", // AI needs approval
  type: "session_ended",     // Terminal closed
}
```

## File Structure

### Desktop (Rust)
```
cli/
├── src/
│   ├── main.rs          # CLI entry point
│   ├── daemon.rs        # WebSocket server
│   ├── protocol.rs      # Message types
│   ├── detection.rs     # AI CLI detection
│   └── pty_wrapper.rs   # Terminal handling
```

### Mobile (React Native)
```
mobile/
├── app/                 # Expo Router screens
├── components/         
│   ├── Terminal.tsx     # xterm.js wrapper
│   └── QRScanner.tsx    # Device pairing
├── hooks/
│   ├── useSync.ts       # WebSocket management
│   └── useDevices.ts    # Multi-device support
```

## Security Model

- **Auth-v2 Pairing**: Setup creates a mobile credential. The QR code contains the daemon URL, device metadata, server id, credential id, and one-time pairing token. The desktop stores only a derived verifier.
- **Device IDs**: UUID per computer (for multi-device support / display)
- **Network Options**: 
  - Local WiFi (192.168.x.x)
  - Tailscale VPN (100.x.x.x)
  - Custom URL
- **Access Control**: Mobile clients must complete challenge-response auth before receiving sessions, terminal data, filesystem data, or push-token registration. Keep port 9847 on a trusted LAN, Tailnet, firewall allowlist, or protected custom endpoint.
- **Terminal Data**: Terminal streams are not sent through a MobileCLI relay. Push notifications use Expo's push service and include notification metadata.

## AI CLI Detection

Automatically detects and adapts UI for:
- **Claude Code**: Numbered options (1/2/3)
- **Codex**: Numbered options
- **Gemini**: Yes/No prompts
- **OpenCode**: Arrow navigation
- **Generic**: Standard terminal

## Testing Infrastructure

### Minimum Release Smoke
- **Desktop OS**: macOS Apple Silicon, macOS Intel, Linux x86_64, Linux ARM64, Windows x64
- **Mobile OS**: iOS release candidate, Android release candidate if Android is distributed
- **Networks**: Same LAN and Tailscale
- **Auth**: QR pair, manual pair, bad token rejection, revoked credential rejection
- **Filesystem**: allowed project roots work; MobileCLI config and common secret paths are denied
- **Installer**: archive checksum is verified before extraction

## Quick Commands

```bash
# Desktop
mobilecli setup             # Initial setup (mobilecli --setup is an alias)
mobilecli pair             # Show QR code
mobilecli daemon           # Start daemon
mobilecli status          # Check status
mobilecli                 # Start streaming session

# Server
systemctl status mobilecli-daemon
journalctl -u mobilecli-daemon -f
ss -tlnp | grep 9847

# Debug
# Expected: legacy hello gets auth_required and no sensitive data
websocat ws://localhost:9847
tcpdump -i any port 9847
```

## Apple Review Tips

1. **Stable QR**: Use server IP, not dynamic
2. **Demo Sessions**: Pre-start with tmux
3. **Review Notes**: Include QR and instructions
4. **Fallback**: Consider offline demo mode
5. **Monitor**: Watch logs during review

## Common Issues

| Problem | Solution |
|---------|----------|
| Can't connect | Check firewall, verify daemon |
| Sessions die | Use tmux/screen |
| High latency | Choose closer server location |
| No notifications | Check push token registration |

## Key Files

- **Config**: `~/.mobilecli/config.json`
- **Daemon PID**: `~/.mobilecli/daemon.pid`
- **Daemon Port**: `~/.mobilecli/daemon.port`
- **Logs**: `/var/log/mobilecli/daemon.log`
