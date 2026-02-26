# Windows Demo Server Setup Guide

This guide sets up a Windows machine on your network as the MobileCLI demo server. This avoids all the headless/Xvfb rendering issues.

## Advantages Over Headless Server

- ✅ Real Windows Terminal (no Xvfb needed)
- ✅ Proper font rendering
- ✅ No display/geometry issues
- ✅ Better performance for TUI apps (Claude, Codex)
- ✅ Easier to debug and monitor

## Prerequisites

- Windows 10/11 machine on your network
- Python 3 (for QR code generation)
- Optional: Windows Terminal (from Microsoft Store)
- Optional: Git for Windows

## Step 1: Download Windows Binary

The Windows binary is at:
```
cli/target/x86_64-pc-windows-gnu/release/mobilecli.exe
```

Copy this to your Windows machine (e.g., `C:\mobilecli\mobilecli.exe`)

## Step 2: Install Prerequisites on Windows

### Option A: Using Windows Terminal (Recommended)
1. Install Windows Terminal from Microsoft Store
2. It will be auto-detected by mobilecli

### Option B: Using Git Bash
1. Install Git for Windows
2. mobilecli will use Git Bash terminal

## Step 3: Start the Daemon

Open PowerShell as Administrator and run:

```powershell
# Create directory
mkdir C:\mobilecli
cd C:\mobilecli

# Start daemon
.\mobilecli.exe daemon --port 9847
```

You should see:
```
▶ Starting daemon on port 9847...
INFO Daemon WebSocket server on port 9847
```

## Step 4: Expose to Internet (Choose One)

### Option A: Ngrok (Easiest)

1. Download ngrok from https://ngrok.com/download
2. Sign up and get authtoken
3. Run:
```powershell
ngrok authtoken YOUR_TOKEN
ngrok tcp 9847
```

4. Note the URL (e.g., `tcp://0.tcp.ngrok.io:12345`)
5. In mobile app, connect to: `wss://0.tcp.ngrok.io:12345`

### Option B: Tailscale (Most Secure)

1. Install Tailscale on Windows machine
2. Install Tailscale on iPhone/iPad
3. Both devices on same Tailscale network
4. Use Windows machine's Tailscale IP (e.g., `100.x.x.x`)
5. In mobile app, connect to: `wss://100.x.x.x:9847`

### Option C: Port Forwarding (If you have public IP)

1. Forward port 9847 on your router to Windows machine
2. Use your public IP or DDNS hostname

## Step 5: Generate QR Code

On Windows, run:
```powershell
.\mobilecli.exe pair
```

Or use Python to generate QR:
```python
import qrcode

# For ngrok
url = "wss://0.tcp.ngrok.io:12345"

# For Tailscale
# url = "wss://100.x.x.x:9847"

qr = qrcode.QRCode(version=1, box_size=10, border=5)
qr.add_data(url)
qr.make(fit=True)
img = qr.make_image(fill_color="black", back_color="white")
img.save("mobilecli-qr.png")
```

## Step 6: Test

1. Scan QR code with mobile app
2. Spawn a session (Claude, Codex, or Shell)
3. Should open in Windows Terminal

## Making it Persistent

### Run as Windows Service

Create a service using NSSM (Non-Sucking Service Manager):

1. Download NSSM from https://nssm.cc/download
2. Run as Administrator:
```powershell
nssm install MobileCLIDaemon
```

3. Set:
   - Path: `C:\mobilecli\mobilecli.exe`
   - Arguments: `daemon --port 9847`
   - Working directory: `C:\mobilecli`

4. Start service:
```powershell
nssm start MobileCLIDaemon
```

## Troubleshooting

### Terminal not detected
- Install Windows Terminal from Microsoft Store
- Or install Git for Windows

### Port in use
- Change port: `mobilecli.exe daemon --port 9848`
- Update ngrok/port forwarding accordingly

### Connection refused
- Check Windows Firewall
- Allow port 9847 through firewall:
```powershell
New-NetFirewallRule -DisplayName "MobileCLI" -Direction Inbound -Protocol TCP -LocalPort 9847 -Action Allow
```

### ngrok URL changes
- Use ngrok with a reserved domain (requires paid plan)
- Or re-scan QR code when URL changes

## Alternative: No Internet Exposure

If you don't want to expose to internet:

1. Connect iPhone/iPad to same WiFi network
2. Use Windows machine's local IP (e.g., `192.168.1.xxx`)
3. In mobile app: `wss://192.168.1.xxx:9847`

Note: This only works when phone is on same network as Windows machine.

## Monitoring

View logs:
```powershell
# If running directly
# Logs appear in terminal

# If running as service
Get-EventLog -LogName Application -Source MobileCLIDaemon -Newest 50
```

Check running sessions:
```powershell
.\mobilecli.exe status
```

## Stopping

If running directly: Press Ctrl+C

If running as service:
```powershell
nssm stop MobileCLIDaemon
nssm remove MobileCLIDaemon
```
