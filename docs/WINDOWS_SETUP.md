# MobileCLI Windows Setup Guide

## Overview

MobileCLI on Windows requires special configuration to ensure terminal windows are **visible** on the desktop. Unlike Unix systems, Windows has session isolation that prevents services from creating visible windows.

## Key Principle: User Session Only

**NEVER run the daemon as a Windows Service** - this forces it into Session 0 (service isolation), making all terminal windows **headless/hidden**.

Instead, the daemon **MUST** run in the **user session** (Console) for windows to be visible.

## Installation Steps

### 1. Install MobileCLI

```powershell
# Copy mobilecli.exe to C:\mobilecli\
# Add to PATH if desired
```

### 2. Enable Autostart (User Session)

```powershell
cd C:\mobilecli
mobilecli.exe autostart install
```

This creates a **Windows Task Scheduler** task that:
- Runs on user logon
- Executes in the **user session** (Console)
- Allows **visible** terminal windows

### 3. Verify Autostart

```powershell
mobilecli.exe autostart status
```

Should show:
```
● Daemon: running
✓ Autostart: installed (Task Scheduler)
  Task: MobileCLIDaemon
```

### 4. Enable Shell Hook (Optional)

For automatic linking when opening terminals:

```powershell
mobilecli.exe autolaunch install
```

This works for:
- PowerShell (via $PROFILE)
- CMD (manual registry - see output)

## Architecture

### Session Model

```
┌─────────────────────────────────────────────────────────────┐
│  User Session (Console)                                     │
│  ┌──────────────────┐                                       │
│  │ MobileCLI Daemon │ ← Must run here!                       │
│  │ (PID in Console) │                                       │
│  └────────┬─────────┘                                       │
│           │ Spawns visible windows                           │
│           ▼                                                  │
│  ┌──────────────────┐     ┌──────────────────┐              │
│  │ PowerShell       │     │ CMD              │              │
│  │ (Visible Window) │     │ (Visible Window) │              │
│  └──────────────────┘     └──────────────────┘              │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  Session 0 (Services) - DO NOT USE                          │
│  ┌──────────────────┐                                       │
│  │ Windows Service  │ ← Creates hidden windows!             │
│  │ (Invisible)      │                                       │
│  └──────────────────┘                                       │
└─────────────────────────────────────────────────────────────┘
```

### How It Works

1. **Task Scheduler** runs daemon on user logon
2. **User session** allows `CREATE_NEW_CONSOLE` to show windows
3. **Mobile spawn** → Daemon spawns mobilecli in new console
4. **Visible window** appears on desktop
5. **WebSocket** streams to phone

## Troubleshooting

### Windows Not Visible

**Problem**: Spawned terminals don't appear on desktop

**Cause**: Daemon running in Session 0 (Services) instead of Console

**Solution**:
```powershell
# Kill service-mode daemon
taskkill /F /IM mobilecli.exe

# Start in user mode via Task Scheduler
schtasks /Run /TN MobileCLIDaemon

# Or manually
cd C:\mobilecli
start /B mobilecli.exe daemon --port 9847
```

### Autostart Not Working

**Problem**: Daemon doesn't start on login

**Solution**:
```powershell
# Reinstall autostart
mobilecli.exe autostart uninstall
mobilecli.exe autostart install

# Check Task Scheduler
schtasks /Query /TN MobileCLIDaemon /V
```

### Shell Hook Not Working

**Problem**: Opening terminal doesn't auto-link

**Solution**:
```powershell
# Check if hook is installed
mobilecli.exe autolaunch status

# Reinstall
mobilecli.exe autolaunch install

# For CMD, manually add registry:
reg add "HKCU\Software\Microsoft\Command Processor" /v AutoRun /t REG_SZ /d "mobilecli" /f
```

## Public Tunnel for Testing

For Apple Review or remote access:

```powershell
# Install localtunnel
npm install -g localtunnel

# Start tunnel (in separate window)
lt --port 9847
```

This gives a public URL like `https://xxx.loca.lt` for mobile connection.

## Commands Reference

| Command | Purpose |
|---------|---------|
| `mobilecli daemon --port 9847` | Start daemon (manual) |
| `mobilecli autostart install` | Enable login autostart |
| `mobilecli autostart uninstall` | Disable login autostart |
| `mobilecli autostart status` | Check autostart status |
| `mobilecli autolaunch install` | Enable shell auto-link |
| `mobilecli autolaunch uninstall` | Disable shell auto-link |
| `mobilecli autolaunch status` | Check shell hook status |

## Design Philosophy

MobileCLI mirrors terminals - it doesn't create hidden sessions. When you spawn a terminal from your phone:

1. A **real window** opens on your desktop
2. You can **see and interact** with it locally
3. Your phone **mirrors** the same session
4. Both views are **synchronized**

This is intentional - the terminal is real and visible, not headless.
