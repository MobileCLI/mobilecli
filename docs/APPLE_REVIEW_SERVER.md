# Apple Review "Always-On" Test Server

Goal: give App Review a stable desktop endpoint to connect to (WebSocket daemon) and a pre-warmed set of demo sessions.

## Recommended Approach

Use a VPS + `tmux`, and terminate TLS with `nginx` so the mobile app connects via `wss://`.

Notes:
- MobileCLI daemon speaks plain WebSocket (`ws://`) by default.
- For review, use `nginx` to provide TLS and proxy to `localhost:9847`.
- Pairing QR can include an `auth_token` for convenience, but direct URL entry is also supported.
- To make the QR encode `wss://`, set a Custom pairing URL in `~/.mobilecli/config.json` (or via setup wizard)
  so `mobilecli setup` prints a `wss=1` QR.

## Steps (Ubuntu 22.04)

1. Provision VPS with a stable public IP.
1. Install the daemon and run it as a service (systemd).
1. Put demo sessions under `tmux` so they survive restarts.
1. Configure `nginx` reverse proxy + Let's Encrypt cert.
1. In App Review notes, provide:
   - the QR code content (or a URL) and pairing steps
   - expected demo flow (what to tap / what you should see)

## Nginx Sketch

Terminate TLS and proxy WebSocket to the daemon:

```nginx
map $http_upgrade $connection_upgrade {
  default upgrade;
  ''      close;
}

server {
  listen 443 ssl;
  server_name demo.mobilecli.app;

  ssl_certificate     /etc/letsencrypt/live/demo.mobilecli.app/fullchain.pem;
  ssl_certificate_key /etc/letsencrypt/live/demo.mobilecli.app/privkey.pem;

  location / {
    proxy_pass http://127.0.0.1:9847;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection $connection_upgrade;
    proxy_set_header Host $host;
  }
}
```

## Demo Sessions

Start a few named sessions for review:

- Claude: `mobilecli -n "Claude Demo" claude`
- Shell: `mobilecli -n "Shell Demo" bash`

Keep them alive with `tmux` and set the service to auto-start on boot.
