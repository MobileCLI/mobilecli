# MobileCLI Project Context for Codex

## Project Overview

**MobileCLI** is a Rust-based terminal streaming application that allows users to mirror any desktop terminal session to their iOS device in real-time. It supports AI coding assistants like Claude Code, OpenAI Codex, and Gemini CLI.

### Core Components

1. **CLI Daemon** (`cli/` directory) - Rust-based WebSocket server that manages PTY sessions
2. **iOS App** (`mobile/` directory) - React Native app for viewing/interacting with terminals
3. **PTY Wrapper** - Handles terminal I/O streaming and session management

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           MOBILECLI ARCHITECTURE                        │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─────────────────────┐      WebSocket       ┌─────────────────────┐  │
│  │   iOS Device        │◄─────────────────────►│   Desktop Daemon    │  │
│  │   (React Native)    │    wss:// or ws://   │   (Rust + tokio)    │  │
│  │                     │                      │   Port 9847         │  │
│  │   - xterm.js UI     │                      │                     │  │
│  │   - Push notifs     │                      │   - PTY management  │  │
│  │   - File browser    │                      │   - Session spawn   │  │
│  │   - Code editor     │                      │   - AI CLI detect   │  │
│  └─────────────────────┘                      └─────────────────────┘  │
│                              ▲                                          │
│                              │                                          │
│                              │ Spawns visible windows                  │
│                              ▼                                          │
│                       ┌───────────────┐                                 │
│                       │  Terminal PTY │                                 │
│                       │  (bash/claude │                                 │
│                       │   /codex/etc) │                                 │
│                       └───────────────┘                                 │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Current Sprint: Windows Demo Server for Apple Review

### Goal
Set up a publicly accessible Windows PC that Apple reviewers can connect to via the MobileCLI iOS app to test the terminal streaming functionality without needing a local development environment.

### Why This Matters
- Apple App Store review requires a working demo environment
- Reviewers don't have local development setups
- The Windows PC must be accessible from the public internet
- Must demonstrate ALL supported CLI types (PowerShell, Claude, Codex, etc.)

---

## Infrastructure Details

### Windows Server PC ("Server PC 1")

| Property | Value |
|----------|-------|
| **Hostname** | DESKTOP-IGVNCBC |
| **Local IP** | 10.0.0.2 |
| **Tailscale IP** | 100.107.50.95 |
| **OS** | Windows (x86_64) |
| **Username** | "Server PC 1" (with space) |
| **Install Dir** | C:\mobilecli |

### Access Methods

#### 1. SSH via Tailscale (Internal)
```bash
# Connect via Tailscale (requires Tailscale on your machine)
ssh "Server PC 1@100.107.50.95"

# Or via local network
ssh "Server PC 1@10.0.0.2"
```

**SSH Key Location**: The Windows PC has your SSH key authorized for passwordless login.

#### 2. Public WebSocket Tunnel (For Apple Reviewers)
```
wss://flat-chairs-exist.loca.lt
```

This is a localtunnel URL that exposes the internal daemon port 9847 to the public internet.

### Hetzner Server (Linux Backup)
- **IP**: 65.21.108.223
- **Purpose**: Was used as SSH tunnel relay, now mostly unused
- **SSH Key**: ~/.ssh/id_hetzner

---

## Critical Technical Details

### Windows Session Isolation (SUPER IMPORTANT)

Windows has a security feature called "Session Isolation" that prevents services from interacting with the user desktop:

| Session | Description | Window Visibility |
|---------|-------------|-------------------|
| **Session 0** | Services/Background processes | HIDDEN (headless) |
| **Session 1+** | User desktop sessions | VISIBLE |
| **Console** | Interactive user session | VISIBLE |

#### The Problem We Solved
If the daemon runs as a Windows Service (Session 0), spawned terminal windows are **INVISIBLE** - they exist but can't be seen on the desktop. This breaks the entire user experience since MobileCLI is supposed to mirror **visible** terminals.

#### The Solution
Use **Windows Task Scheduler** with `ONLOGON` trigger instead of Windows Services:

```powershell
# Creates a task that runs on user logon in the CONSOLE session
schtasks /Create /TN MobileCLIDaemon /TR "C:

\\mobilecli\\mobilecli.exe daemon --port 9847" /SC ONCE /ST 23:59 /RU "DESKTOP-IGVNCBC\Server PC 1" /F
```

This ensures:
1. Daemon runs in Session 2 (Console - user desktop)
2. Spawned terminals are VISIBLE
3. User can interact with both desktop and mobile simultaneously

### Process Architecture on Windows

```
┌─────────────────────────────────────────────────────────────────┐
│ Session 2 - Console (User Desktop)                             │
│ ┌───────────────────────┐                                       │
│ │ mobilecli.exe (daemon)│ ← Parent process, runs Task Scheduler│
│ │ PID: 12784            │   Handles WebSocket connections       │
│ └───────────┬───────────┘                                       │
│             │ Spawns via CREATE_NEW_CONSOLE flag                │
│             ▼                                                   │
│ ┌───────────────────────┐     ┌───────────────────────┐        │
│ │ powershell.exe        │     │ codex.exe             │        │
│ │ (Visible Window)      │     │ (Visible Window)      │        │
│ │ Session: Console      │     │ Session: Console      │        │
│ └───────────────────────┘     └───────────────────────┘        │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ Session 0 - Services (DO NOT USE)                              │
│ ┌───────────────────────┐                                       │
│ │ mobilecli.exe (BAD)   │ ← Creates HIDDEN windows              │
│ │ Windows Service       │   Users can't see terminals!          │
│ └───────────────────────┘                                       │
└─────────────────────────────────────────────────────────────────┘
```

---

## Recent Fixes and Changes

### 1. Terminal Window Visibility (Session 0 → Console)
**Problem**: Spawned terminals were invisible on Windows desktop  
**Root Cause**: Daemon running as Windows Service in Session 0  
**Solution**: 
- Switched to Task Scheduler with user session
- Added `CREATE_NEW_CONSOLE` flag (0x00000010) to process creation
- Updated `spawn_session_windows()` in `daemon.rs`

### 2. Screen Jumping / Content in Middle
**Problem**: Text appeared in middle of mobile screen instead of top  
**Root Cause**: `println!("📱 Connected!...")` was outputting before PTY content  
**Solution**: Added `--quiet` flag to all spawned sessions:
```rust
// In daemon.rs - Windows spawn
cmd.arg("--quiet");  // Suppresses "Connected!" message

// In daemon.rs - Unix spawn (build_wrap_shell_command)
tokens.push("--quiet".to_string());
```

### 3. PowerShell Support
**Problem**: `powershell` command not in allowed list  
**Solution**: Added to `is_allowed_command()`:
```rust
const ALLOWED_COMMANDS: &[&str] = &[
    "claude", "codex", "gemini", "opencode", "bash", "zsh", "sh", 
    "fish", "nu", "pwsh", "powershell",  // ← Added powershell
    "python", "python3", "node", "ruby",
];
```

### 4. Codex Installation
**Problem**: Codex CLI wasn't installed on Windows  
**Solution**: 
```powershell
npm install -g @openai/codex
```
Now Codex spawns work from mobile and show the welcome screen.

---

## Current System State

### Running Processes on Windows PC
```
mobilecli.exe      PID 12784   Console     Session 2   (Daemon)
node.exe           PID 2284    Services    Session 0   (Tunnel)
powershell.exe     PID varies  Console     Session 2   (Spawned sessions)
codex.exe          PID varies  Console     Session 2   (Spawned sessions)
```

### Active Sessions
**Tunnel URL**: `wss://flat-chairs-exist.loca.lt`

**Current Sessions** (check with):
```bash
wscat -c wss://flat-chairs-exist.loca.lt -x '{"type":"list_sessions"}'
```

### Autostart Configuration
- **Task Name**: `MobileCLIDaemon`
- **Trigger**: ONLOGON (runs when user logs in)
- **User**: DESKTOP-IGVNCBC\Server PC 1
- **Session**: Console (Session 2)

Check status:
```powershell
schtasks /Query /TN MobileCLIDaemon /V
```

---

## Development Workflow

### Building for Windows

```bash
# From project root
cd cli/

# Cross-compile for Windows (requires mingw-w64)
cargo build --release --target x86_64-pc-windows-gnu

# Binary location
target/x86_64-pc-windows-gnu/release/mobilecli.exe
```

### Deploying to Windows PC

```bash
# Stop existing daemon
ssh "Server PC 1@10.0.0.2" 'taskkill /F /IM mobilecli.exe'

# Copy new binary
scp target/x86_64-pc-windows-gnu/release/mobilecli.exe \
  "Server PC 1@10.0.0.2":C:/mobilecli/

# Restart via Task Scheduler
ssh "Server PC 1@10.0.0.2" 'schtasks /Run /TN MobileCLIDaemon'
```

### Testing Spawns

```bash
# From Linux/Mac terminal
wscat -c wss://flat-chairs-exist.loca.lt \
  -x '{"type":"spawn_session","command":"powershell"}'

# Check if window is visible on Windows
ssh "Server PC 1@10.0.0.2" 'tasklist /V /FI "ImageName eq powershell.exe"'
```

---

## GitHub Repository Structure

```
MobileCLI/
├── cli/                      # Rust daemon and CLI
│   ├── src/
│   │   ├── main.rs          # CLI entry point, command parsing
│   │   ├── daemon.rs        # WebSocket server, session management
│   │   ├── pty_wrapper.rs   # PTY allocation, I/O streaming
│   │   ├── shell_hook.rs    # Shell autolaunch integration
│   │   ├── autostart.rs     # systemd/launchd/Task Scheduler
│   │   └── platform.rs      # Cross-platform utilities
│   └── Cargo.toml
├── mobile/                   # React Native iOS app (separate repo)
├── WINDOWS_SETUP.md          # Comprehensive Windows guide
├── CODEX_CONTEXT.md          # This file
└── README.md
```

### Key Branches
- `main` - Production code
- All Windows fixes have been merged to main

---

## Testing Checklist for Apple Review

### ✅ Working Features
- [x] PowerShell spawn (visible window, no jumping)
- [x] Codex spawn (visible window, auth screen shows)
- [x] Claude spawn (if installed)
- [x] Daemon runs in Console session (not Services)
- [x] Autostart via Task Scheduler
- [x] Public tunnel accessible
- [x] WebSocket connections stable
- [x] PTY bytes streaming to mobile

### 📱 For Apple Reviewers
1. Download MobileCLI from TestFlight
2. Enter server URL: `wss://flat-chairs-exist.loca.lt`
3. Tap Connect
4. Spawn any session type from the Sessions tab
5. **Verify**: Terminal window appears on Windows desktop
6. **Verify**: Mobile shows terminal content starting from top (not middle)

---

## Known Limitations

1. **Tunnel URL Changes**: localtunnel URLs change when the tunnel restarts. Current URL is `wss://flat-chairs-exist.loca.lt` but this may change.

2. **Session 0 Warning**: If daemon ever runs in Session 0 (Windows Services), windows will be invisible. Always use Task Scheduler method.

3. **SSH Username**: The Windows username "Server PC 1" has a space, requiring quotes in SSH commands: `ssh "Server PC 1@10.0.0.2"`

4. **Windows Defender**: May occasionally flag the binary - need to add exclusion for C:\mobilecli

---

## Troubleshooting Guide

### Windows Not Visible
```powershell
# Check which session daemon is in
tasklist /V /FI "ImageName eq mobilecli.exe"

# If Session# is 0, kill and restart in Console session
taskkill /F /IM mobilecli.exe
schtasks /Run /TN MobileCLIDaemon
```

### Screen Jumping
- Already fixed with `--quiet` flag
- If still happening, verify binary is latest version

### Tunnel Not Working
```powershell
# Restart tunnel
ssh "Server PC 1@10.0.0.2" 'taskkill /F /IM node.exe'
ssh "Server PC 1@10.0.0.2" 'wmic process call create "cmd /c cd /d C:\mobilecli && lt --port 9847"'

# Check new URL
ssh "Server PC 1@10.0.0.2" 'type C:\mobilecli\tunnel.log'
```

### Codex Not Found
```powershell
# Install Codex
npm install -g @openai/codex

# Verify
where codex
codex --version
```

---

## Architecture Decision Records

### ADR 1: Task Scheduler vs Windows Service
**Decision**: Use Task Scheduler instead of Windows Service  
**Rationale**: Windows Services run in Session 0 which cannot create visible windows on the desktop. Task Scheduler can run in user session (Console).  
**Impact**: Autostart works differently than Linux/macOS, requires documenting in WINDOWS_SETUP.md.

### ADR 2: --quiet Flag for Spawns
**Decision**: Always use --quiet for mobile-spawned sessions  
**Rationale**: The "Connected!" message was appearing before PTY content, pushing actual terminal content down. Mobile users expect content to start at top.  
**Impact**: Users won't see the "Session visible on phone" confirmation, but that's acceptable tradeoff.

---

## Commands Reference

### Windows PC Management
```bash
# SSH to Windows
ssh "Server PC 1@10.0.0.2"

# Check daemon status
tasklist /V /FI "ImageName eq mobilecli.exe"

# Restart daemon
schtasks /End /TN MobileCLIDaemon
schtasks /Run /TN MobileCLIDaemon

# View daemon logs
type "%APPDATA%\mobilecli\daemon.log"

# Check autostart status
mobilecli.exe autostart status
```

### Mobile CLI Commands
```bash
# Spawn from command line
mobilecli --name "My Session" powershell

# With quiet mode (what daemon uses)
mobilecli --name "My Session" --quiet powershell

# Setup autostart
mobilecli autostart install

# Setup shell hook
mobilecli autolaunch install
```

### WebSocket Testing
```bash
# List sessions
wscat -c wss://flat-chairs-exist.loca.lt -x '{"type":"list_sessions"}'

# Spawn session
wscat -c wss://flat-chairs-exist.loca.lt \
  -x '{"type":"spawn_session","command":"codex","name":"AI Session"}'

# Subscribe to session output
wscat -c wss://flat-chairs-exist.loca.lt \
  -x '{"type":"subscribe","session_id":"SESSION_ID_HERE"}'
```

---

## Contact & Resources

- **GitHub**: https://github.com/MobileCLI/mobilecli
- **Windows Setup Guide**: `WINDOWS_SETUP.md`
- **Current Tunnel**: `wss://flat-chairs-exist.loca.lt`
- **Windows PC**: DESKTOP-IGVNCBC (10.0.0.2, 100.107.50.95)

---

## Summary for Codex

We've successfully set up a Windows demo server for Apple App Store review. The key challenges were:

1. **Windows Session Isolation**: Solved by using Task Scheduler instead of Windows Services
2. **Terminal Visibility**: Ensured all spawned windows appear in Console session
3. **Screen Jumping**: Fixed by suppressing the "Connected!" message with `--quiet` flag
4. **Codex Support**: Installed the Codex CLI globally on Windows

The system is now stable and ready for Apple reviewers to test. The public tunnel provides access without requiring local setup.

**Next Steps** (if needed):
- Monitor tunnel stability
- Update tunnel URL in Apple Review notes if it changes
- Consider setting up a persistent subdomain with ngrok or Cloudflare Tunnel
- Add more AI CLI tools if requested (currently have: claude, codex, gemini, opencode)
