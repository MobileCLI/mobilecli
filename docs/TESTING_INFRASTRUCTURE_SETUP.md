# MobileCLI Testing Infrastructure Setup Guide

## Quick Start: Apple Review Testing Server

This guide walks through setting up a production-ready MobileCLI testing server for Apple app review on a budget.

## Option 1: Hetzner Cloud Setup (Recommended)

### Step 1: Create Hetzner Account & Server

1. Sign up at https://console.hetzner.cloud
2. Create new project "MobileCLI Testing"
3. Create server:
   - **Type**: CPX21 (3 vCPU, 4GB RAM, 80GB NVMe)
   - **Location**: Ashburn (USA) or Falkenstein (EU)
   - **OS**: Ubuntu 22.04
   - **SSH Key**: Add your public key
   - **Name**: mobilecli-test-01

### Step 2: Initial Server Configuration

```bash
# Connect to server
ssh root@<your-server-ip>

# Update system
apt update && apt upgrade -y

# Install essential packages
apt install -y \
  build-essential \
  curl \
  git \
  ufw \
  fail2ban \
  htop \
  tmux \
  supervisor

# Configure firewall
ufw default deny incoming
ufw default allow outgoing
ufw allow ssh
ufw allow 9847/tcp  # MobileCLI WebSocket
ufw --force enable

# Create non-root user
adduser mobilecli
usermod -aG sudo mobilecli
su - mobilecli
```

### Step 3: Install MobileCLI

```bash
# Install Rust (as mobilecli user)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Clone and build MobileCLI
git clone https://github.com/MobileCLI/mobilecli.git
cd mobilecli/cli
cargo build --release

# Install binary
sudo cp target/release/mobilecli /usr/local/bin/
sudo chmod +x /usr/local/bin/mobilecli

# Create config directory
mkdir -p ~/.mobilecli
```

### Step 4: Configure MobileCLI Daemon

```bash
# Run initial setup
mobilecli setup

# Choose option 3 (Custom) and enter:
# ws://<your-server-ip>:9847

# This generates device ID and config
```

### Step 5: Setup Systemd Service

Create `/etc/systemd/system/mobilecli-daemon.service`:

```ini
[Unit]
Description=MobileCLI Daemon
After=network.target

[Service]
Type=simple
User=mobilecli
WorkingDirectory=/home/mobilecli
ExecStart=/usr/local/bin/mobilecli daemon
Restart=always
RestartSec=10
StandardOutput=append:/var/log/mobilecli/daemon.log
StandardError=append:/var/log/mobilecli/daemon.log

# Security settings
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/home/mobilecli/.mobilecli

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo mkdir -p /var/log/mobilecli
sudo chown mobilecli:mobilecli /var/log/mobilecli
sudo systemctl daemon-reload
sudo systemctl enable mobilecli-daemon
sudo systemctl start mobilecli-daemon
sudo systemctl status mobilecli-daemon
```

### Step 6: Install Demo AI Tools

```bash
# Install Node.js for Claude/Codex
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs

# Install Python for additional demos
sudo apt install -y python3-pip python3-venv

# Create demo directory
mkdir ~/demos
cd ~/demos

# Create simple demo scripts
cat > ai_demo.py << 'EOF'
#!/usr/bin/env python3
import time

print("🤖 AI Assistant Demo")
print("This simulates an AI coding assistant needing approval")
time.sleep(2)

# Simulate tool approval request
print("\n⚠️  Tool Approval Required:")
print("The AI wants to run: rm -rf /tmp/test")
print("Options:")
print("  1) Approve")
print("  2) Approve Always") 
print("  3) Deny")
print("\nYour choice: ", end="", flush=True)

choice = input()
if choice == "1":
    print("✅ Approved! Executing command...")
elif choice == "2":
    print("✅ Always approved for this session")
else:
    print("❌ Denied")
EOF

chmod +x ai_demo.py
```

### Step 7: Create Demo Sessions

```bash
# Start persistent demo sessions using tmux
tmux new-session -d -s demo1 'mobilecli -n "AI Assistant Demo" python3 ~/demos/ai_demo.py'
tmux new-session -d -s demo2 'mobilecli -n "Terminal Session" bash'
tmux new-session -d -s demo3 'mobilecli -n "Build Process" watch -n 1 "date; echo Building..."'
```

### Step 8: Generate QR Code for Testing

```bash
# Get connection info
mobilecli pair

# This displays QR code with format:
# mobilecli://<server-ip>:9847?device_id=xxx&device_name=xxx
```

## Option 2: Oracle Cloud Free Tier

### Advantages:
- Completely free (no credit card required after trial)
- ARM processor good for testing
- 100GB storage

### Setup Process:
```bash
# Similar to Hetzner but:
# 1. Choose ARM Ampere A1 instance
# 2. Select "Always Free Eligible" resources
# 3. Open port 9847 in security list
# 4. Follow same installation steps
```

## Option 3: Local Testing with ngrok

For quick testing without a VPS:

```bash
# Install ngrok
curl -s https://ngrok-agent.s3.amazonaws.com/ngrok.asc | sudo tee /etc/apt/trusted.gpg.d/ngrok.asc >/dev/null
echo "deb https://ngrok-agent.s3.amazonaws.com buster main" | sudo tee /etc/apt/sources.list.d/ngrok.list
sudo apt update && sudo apt install ngrok

# Run MobileCLI daemon locally
mobilecli daemon &

# Expose via ngrok
ngrok tcp 9847

# Use the ngrok URL for testing
# Example: tcp://2.tcp.ngrok.io:12345
```

## Testing Checklist

### Pre-Review Testing:
- [ ] Daemon auto-starts on reboot
- [ ] WebSocket accepts connections
- [ ] Demo sessions stay active
- [ ] Push notifications work
- [ ] Server handles multiple connections
- [ ] Logs are being collected

### Apple Review Preparation:

1. **App Store Connect Setup:**
   - Upload build via EAS/TestFlight
   - Add review notes with QR code
   - Include test instructions

2. **Review Notes Template:**
   ```
   To test MobileCLI:
   
   1. Open the app
   2. Tap "Add Device" 
   3. Scan this QR code: [Include QR image]
   4. You'll see 3 demo sessions
   5. Open "AI Assistant Demo"
   6. When prompted, tap "Approve"
   
   The app allows developers to control command-line tools from their phone.
   No account required - it's a direct connection to a computer.
   ```

3. **Demo Commands:**
   ```bash
   # Terminal session demos
   echo "Hello from MobileCLI"
   ls -la
   git status
   
   # Trigger approval prompt
   python3 ~/demos/ai_demo.py
   ```

## Monitoring & Debugging

### View Logs:
```bash
# Daemon logs
sudo journalctl -u mobilecli-daemon -f

# Connection logs
tail -f /var/log/mobilecli/daemon.log

# Active sessions
mobilecli status
```

### Debug Connection Issues:
```bash
# Test WebSocket locally
websocat ws://localhost:9847

# Check if port is open
sudo ss -tlnp | grep 9847

# Monitor connections
sudo tcpdump -i any port 9847
```

### Performance Monitoring:
```bash
# Install monitoring
sudo apt install -y nethogs iotop

# Monitor network usage
sudo nethogs

# Monitor CPU/Memory
htop
```

## Cost Optimization Tips

1. **Use Hetzner's snapshot feature** - Stop server between tests
2. **Set up monitoring alerts** - Catch issues early
3. **Use tmux/screen** for persistent sessions
4. **Automate with cron** for session recreation
5. **Consider Hetzner's hourly billing** for temporary needs

## Backup & Recovery

```bash
# Backup configuration
tar -czf mobilecli-backup.tar.gz ~/.mobilecli/

# Backup entire setup
sudo tar -czf server-backup.tar.gz \
  /home/mobilecli \
  /etc/systemd/system/mobilecli* \
  /usr/local/bin/mobilecli

# Quick restore script
cat > restore.sh << 'EOF'
#!/bin/bash
tar -xzf server-backup.tar.gz -C /
systemctl daemon-reload
systemctl restart mobilecli-daemon
EOF
```

## Security Hardening

```bash
# Install fail2ban rules
cat > /etc/fail2ban/jail.local << 'EOF'
[DEFAULT]
bantime = 3600
findtime = 600
maxretry = 5

[sshd]
enabled = true

[mobilecli]
enabled = true
port = 9847
logpath = /var/log/mobilecli/daemon.log
maxretry = 10
EOF

# Restart fail2ban
sudo systemctl restart fail2ban
```

## Troubleshooting

### Common Issues:

1. **Can't connect from mobile app**
   - Check firewall: `sudo ufw status`
   - Verify daemon running: `systemctl status mobilecli-daemon`
   - Test locally: `websocat ws://localhost:9847`

2. **Sessions disappear**
   - Use tmux/screen for persistence
   - Check daemon logs for crashes
   - Increase system limits if needed

3. **High latency**
   - Choose server location closer to reviewers
   - Use Tailscale for better routing
   - Check network congestion

## Success Metrics

- **Uptime**: 99.9% during review period
- **Response time**: <100ms WebSocket latency
- **Concurrent users**: Support 10+ reviewers
- **Demo reliability**: Zero-failure demo sessions

## Next Steps

1. Set up production monitoring (Uptime Robot)
2. Implement automatic session recovery
3. Create video walkthrough for reviewers
4. Prepare for scale if app gets featured

---

**Total Infrastructure Cost**: $6.30/month (Hetzner CPX21)
**Setup Time**: ~2 hours
**Maintenance**: ~1 hour/month
