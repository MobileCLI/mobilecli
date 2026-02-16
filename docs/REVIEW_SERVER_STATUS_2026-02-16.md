# Review Server Status (2026-02-16)

## Provisioned Host

- Provider: Hetzner
- Server IP: `65.21.108.223`
- Hostname: `apple-tester`

## What Is Already Configured

- `mobilecli` binary installed at `/usr/local/bin/mobilecli`
- Daemon service running:
  - `mobilecli-daemon.service` (port `9847`)
- Virtual X display running for GUI terminal spawn support:
  - `xvfb-mobilecli.service` (`DISPLAY=:1`)
- Nginx reverse proxy configured:
  - `demo.mobilecli.app` (HTTP now)
  - WebSocket proxy to `127.0.0.1:9847`
- Firewall enabled (`ufw`):
  - allow `22`, `80`, `443`
- Persistent demo session keeper enabled:
  - `mobilecli-demo-keeper.service`
  - keeps tmux session `mobilecli-shell` alive
  - always maintains at least one `Shell Demo` session

## Functional Verification Completed

- Direct daemon health:
  - `mobilecli status` shows daemon running
- Session availability:
  - `Shell Demo` is visible/active
- Remote spawn behavior:
  - `spawn_session` succeeds and registers sessions

## Blocker (TLS / WSS)

`demo.mobilecli.app` DNS is still pointing to old infrastructure, not this server.

Current resolution observed:
- `demo.mobilecli.app -> 89.167.6.36`

Let's Encrypt failed for this reason.

## Required DNS Changes

In DNS provider for `mobilecli.app`:

1. Set `A` record:
- Host: `demo`
- Value: `65.21.108.223`
- TTL: 60 or 300

2. Remove stale `AAAA` record for `demo` (if present), unless you have IPv6 configured on this server.

3. If using Cloudflare proxy, set record to **DNS only** (not proxied) for certificate issuance.

## Finalize TLS After DNS Propagates

Run on server:

```bash
certbot --nginx -d demo.mobilecli.app --non-interactive --agree-tos -m kingthecole2002@gmail.com --redirect
systemctl reload nginx
```

Validate:

```bash
curl -I https://demo.mobilecli.app
```

## Pairing URL / QR

Server config has been set so `mobilecli pair` emits QR for:
- `wss://demo.mobilecli.app`

Once TLS is live, this is the URL to use for App Review pairing.

## Useful Ops Commands

```bash
systemctl status mobilecli-daemon.service
systemctl status xvfb-mobilecli.service
systemctl status mobilecli-demo-keeper.service
mobilecli status
journalctl -u mobilecli-daemon.service -n 200 --no-pager
```
