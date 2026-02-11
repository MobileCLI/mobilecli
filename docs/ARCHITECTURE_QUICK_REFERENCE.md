# MobileCLI Architecture Quick Reference

## Core Components

### Desktop Daemon (Rust)
- **Port**: 9847 (WebSocket)
- **Config**: `~/.mobilecli/config.json`
- **Binary**: `/usr/local/bin/mobilecli`
- **Process**: Manages PTY sessions, WebSocket server

### Mobile App (React Native)
- **Framework**: Expo SDK 52
- **Terminal**: xterm.js
- **Storage**: expo-secure-store (device links)
- **Notifications**: expo-notifications (push)

## Connection Flow

```
1. Desktop: mobilecli setup (or mobilecli --setup)
   ↓
2. Generate QR: mobilecli://ip:9847?device_id=xxx
   ↓
3. Mobile: Scan QR → Store device
   ↓
4. WebSocket: Connect & authenticate
   ↓
5. Stream: PTY bytes ↔ Terminal display
```

## Key Protocol Messages

### Client → Server
```typescript
{
  type: "hello",              // Initial handshake
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

- **Pairing Token (Optional)**: A per-device `auth_token` is generated during setup and embedded in the pairing QR code for convenience.
- **Device IDs**: UUID per computer (for multi-device support / display)
- **Network Options**: 
  - Local WiFi (192.168.x.x)
  - Tailscale VPN (100.x.x.x)
  - Custom URL
- **Data**: Never leaves your network

## AI CLI Detection

Automatically detects and adapts UI for:
- **Claude Code**: Numbered options (1/2/3)
- **Codex**: Numbered options
- **Gemini**: Yes/No prompts
- **OpenCode**: Arrow navigation
- **Generic**: Standard terminal

## Testing Infrastructure

### Minimum Requirements
- **VPS**: 1 vCPU, 1GB RAM, 20GB storage
- **OS**: Ubuntu 22.04 LTS
- **Ports**: 22 (SSH), 9847 (WebSocket)
- **Cost**: ~$6/month

### Recommended Stack
- **Server**: Hetzner CPX21 (€5.83/month)
- **Process Manager**: systemd
- **Monitoring**: journalctl logs
- **Sessions**: tmux for persistence

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
websocat ws://localhost:9847
tcpdump -i any port 9847
```

## Budget Breakdown

| Service | Monthly Cost | Purpose |
|---------|--------------|---------|
| Hetzner VPS | $6.30 | Test server |
| Domain (optional) | $1 | Custom URL |
| SSL (Let's Encrypt) | $0 | HTTPS |
| **Total** | **$7.30** | Complete setup |

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
