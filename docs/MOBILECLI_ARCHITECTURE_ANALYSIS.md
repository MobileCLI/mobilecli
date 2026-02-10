# MobileCLI Architecture Analysis & Testing Infrastructure Plan

## Executive Summary

MobileCLI is a self-hosted solution that enables users to control AI coding assistants from their phone. It consists of a Rust daemon running on the desktop and a React Native mobile app that connects via WebSocket. This document analyzes the architecture and proposes a cost-effective testing infrastructure for Apple review within a $1K budget.

## 1. Technical Architecture Overview

### Components

```
┌─────────────────────┐     WebSocket      ┌─────────────────────┐
│                     │◄──────────────────►│                     │
│  Desktop Daemon     │    Port 9847       │    Mobile App       │
│  (Rust CLI)         │                    │  (React Native)     │
│                     │                    │                     │
└────────┬────────────┘                    └─────────────────────┘
         │
         │ PTY (Pseudo-Terminal)
         ▼
┌─────────────────────┐
│                     │
│  AI CLI Tool        │
│  (Claude/Codex/etc) │
│                     │
└─────────────────────┘
```

### Key Technologies
- **Desktop**: Rust CLI daemon with WebSocket server
- **Mobile**: React Native (Expo) with xterm.js terminal emulator
- **Communication**: WebSocket protocol over local network or Tailscale VPN
- **Security**: Device-based pairing with unique device IDs

## 2. Mobile-Desktop Connection Process

### 2.1 Initial Setup & QR Code Pairing

1. **Desktop Setup**
   - User runs `mobilecli setup` (or `mobilecli --setup`) on their computer
   - Daemon generates unique `device_id` (UUID)
   - Gets local IP and/or Tailscale IP
   - Displays QR code containing connection info

2. **QR Code Format**
   ```
   mobilecli://192.168.1.100:9847?device_id=UUID&device_name=MacBook-Pro
   ```
   - Contains WebSocket URL
   - Includes device ID for multi-device support
   - Device name for user-friendly display

3. **Mobile Pairing**
   - User scans QR code with mobile app
   - App stores device info in secure storage
   - Creates persistent device link
   - Can pair multiple computers

### 2.2 Connection Protocol

1. **WebSocket Handshake**
   ```json
   // Mobile → Desktop
   {
     "type": "hello",
     "client_version": "0.1.0"
   }
   
   // Desktop → Mobile
   {
     "type": "welcome",
     "server_version": "0.1.0",
     "device_id": "uuid-string",
     "device_name": "MacBook-Pro"
   }
   ```

2. **Session Management**
   - Mobile requests active sessions: `get_sessions`
   - Desktop streams terminal output via `pty_bytes` (base64 encoded)
   - Mobile sends input via `send_input`
   - Real-time PTY resize with `pty_resize`

3. **Push Notifications**
   - Mobile registers token (Expo/APNS/FCM)
   - Desktop sends notifications when CLI needs approval
   - Background support for tool approval requests

### 2.3 Security & Authentication

- **Device-based**: Each computer has unique UUID
- **Network-level**: Relies on local network or VPN security
- **No cloud relay**: Direct peer-to-peer connection
- **Optional encryption**: Support for encrypted WebSocket (wss://)

## 3. Testing Infrastructure Requirements

### 3.1 Apple Review Process Requirements

1. **Demo Environment**
   - Running desktop daemon accessible from reviewer's network
   - Pre-configured AI CLI sessions
   - Stable WebSocket connection
   - Example commands that trigger approval flows

2. **TestFlight Requirements**
   - iOS build uploaded via Xcode/EAS
   - App Store Connect metadata
   - Screenshots showing key features
   - Review notes explaining connection process

3. **Technical Requirements**
   - Server must be accessible 24/7 during review
   - Support for multiple concurrent connections
   - Logging for debugging review issues
   - Fallback demo mode if connection fails

### 3.2 Infrastructure Components Needed

1. **Desktop Host Server**
   - Linux/macOS VPS or dedicated server
   - Rust toolchain for building daemon
   - AI CLI tools installed (Claude, Codex, etc.)
   - Stable public IP or domain

2. **Network Configuration**
   - Open port 9847 for WebSocket
   - Optional: Tailscale for VPN access
   - SSL certificate for secure WebSocket (recommended)
   - Reverse proxy (nginx) for multiple instances

3. **Monitoring & Logging**
   - Daemon process monitoring (systemd/pm2)
   - WebSocket connection logs
   - Error tracking for debugging
   - Uptime monitoring

## 4. $1K Budget Testing Infrastructure

### 4.1 Recommended Setup

**Primary Server: Hetzner Cloud VPS**
- **Type**: CPX21 (3 vCPU, 4GB RAM)
- **Cost**: €5.83/month (~$6.30/month)
- **Storage**: 80GB NVMe
- **Location**: Choose US or EU based on reviewers
- **OS**: Ubuntu 22.04 LTS

**Why This Works:**
- Sufficient resources for daemon + AI CLIs
- 24/7 availability for Apple review
- Public IPv4 included
- Excellent price/performance ratio

**12-Month Cost**: ~$76

### 4.2 Alternative Budget Options

1. **Oracle Cloud Free Tier**
   - **Specs**: 1 OCPU, 1GB RAM, 100GB storage
   - **Cost**: $0 (free tier)
   - **Pros**: Completely free, ARM-based
   - **Cons**: Limited resources, potential throttling

2. **Vultr VPS**
   - **Type**: Regular Cloud Compute
   - **Cost**: $6/month (1 vCPU, 1GB RAM)
   - **Pros**: Multiple locations, good network
   - **Cons**: Less storage than Hetzner

3. **DigitalOcean Droplet**
   - **Type**: Basic droplet
   - **Cost**: $6/month (1 vCPU, 1GB RAM)
   - **Pros**: Developer-friendly, good docs
   - **Cons**: Standard pricing

### 4.3 Setup Process

```bash
# 1. Provision VPS
# 2. SSH into server
ssh root@your-server-ip

# 3. Install dependencies
apt update && apt upgrade -y
apt install -y build-essential curl git

# 4. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 5. Clone and build MobileCLI
git clone https://github.com/MobileCLI/mobilecli.git
cd mobilecli/cli
cargo build --release

# 6. Install as system service
sudo cp target/release/mobilecli /usr/local/bin/
```

### 4.4 Cost Breakdown (Annual)

| Component | Cost | Purpose |
|-----------|------|---------|
| Hetzner VPS | $76 | Primary testing server |
| Domain (optional) | $12 | demo.mobilecli.app |
| SSL cert | $0 | Let's Encrypt |
| Backup storage | $12 | 10GB Hetzner backup |
| **Total** | **$100** | Full infrastructure |

**Remaining Budget**: $900 for additional servers, development tools, or scaling

### 4.5 Demo Configuration

1. **Pre-configured Sessions**
   ```bash
   # Start demo sessions
   mobilecli -n "Claude Code Demo" claude
   mobilecli -n "Codex Demo" codex
   mobilecli -n "Terminal Demo" bash
   ```

2. **Review Instructions**
   - Provide QR code in App Store review notes
   - Include test commands that trigger approvals
   - Document expected behavior

3. **Fallback Demo Mode**
   - Implement offline demo if connection fails
   - Pre-recorded terminal sessions
   - Simulated approval flows

## 5. Implementation Timeline

### Phase 1: Infrastructure Setup (Week 1)
- [ ] Provision Hetzner VPS
- [ ] Install and configure MobileCLI daemon
- [ ] Setup systemd service for auto-start
- [ ] Configure firewall rules

### Phase 2: Testing Environment (Week 2)
- [ ] Install AI CLI tools
- [ ] Create demo scripts
- [ ] Test WebSocket connections
- [ ] Setup monitoring

### Phase 3: Apple Review Prep (Week 3)
- [ ] Generate stable QR codes
- [ ] Document review process
- [ ] Create video demonstrations
- [ ] Submit to TestFlight

### Phase 4: Review Support (Week 4+)
- [ ] Monitor server during review
- [ ] Quick response to reviewer questions
- [ ] Debug any connection issues
- [ ] Iterate based on feedback

## 6. Recommendations

1. **Start with Hetzner VPS** - Best value within budget
2. **Use Tailscale** for backup connectivity option
3. **Implement connection retry logic** in mobile app
4. **Create detailed review guide** with screenshots
5. **Consider demo video** showing full workflow
6. **Monitor server closely** during review period
7. **Have backup server** ready if primary fails

## 7. Security Considerations

- **Firewall**: Restrict to port 9847 only
- **Rate limiting**: Prevent connection spam
- **Auth tokens**: Implement for production
- **VPN fallback**: Tailscale as secure option
- **Logging**: Audit connection attempts

## 8. Conclusion

MobileCLI's architecture is well-designed for self-hosted deployment. The WebSocket-based communication and QR code pairing make it user-friendly while maintaining security. For Apple review testing, a simple Hetzner VPS ($6/month) provides more than adequate resources within the $1K budget, leaving significant funds for scaling or additional development resources.

The key to successful Apple review is having a stable, always-on testing environment with pre-configured demo sessions that showcase the app's core functionality.
