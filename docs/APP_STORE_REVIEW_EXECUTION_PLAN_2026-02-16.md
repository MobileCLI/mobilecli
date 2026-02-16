# App Store Review Execution Plan (2026-02-16)

## Current Code State

- Desktop repo (`MobileCLI/mobilecli`) is on `main` and pushed.
- Mobile repo (`MobileCLI/mobile`) is on `main` and pushed.
- Desktop autostart + spawn fixes are in `main`.
- Mobile iOS reconnect + stability fixes are in `main`.

## Reviewer Access Strategy (Recommended)

Use a **public demo daemon** reachable over `wss://` so App Review can test without your personal machine.

Why this is best:
- Reviewer can connect instantly from any network.
- No dependency on your home network uptime.
- No need to ask reviewer to install Tailscale/VNC.

### Important Recommendation

Do **not** require VNC for App Review as the primary path.
Provide a fully self-contained in-app flow:
1. Open app
2. Scan provided QR
3. See active sessions
4. Open session
5. Test file system + attachments + purchase flow

VNC can still exist as backup evidence only (for your own support).

## Demo Server Blueprint (Hetzner / Ubuntu 22.04)

1. Provision VM
- 2 vCPU / 4 GB RAM is enough.
- Static public IP.
- Domain: `review.mobilecli.app` (or similar).

2. Install desktop daemon binary
- Install `mobilecli` to `/usr/local/bin/mobilecli` or user `~/.local/bin/mobilecli`.
- Enable autostart:
  - `mobilecli autostart install`
  - Verify: `mobilecli autostart status`

3. TLS termination with nginx
- Proxy `wss://review.mobilecli.app` -> `ws://127.0.0.1:9847`.
- Use Let's Encrypt certificate.

4. Lock down host
- UFW allow `22`, `443`; deny rest.
- Fail2ban optional but recommended.
- Dedicated non-root user for daemon.

5. Pre-warm demo sessions
- Open visible demo sessions and keep them alive:
  - `mobilecli -n "Shell Demo" bash`
  - `mobilecli -n "Codex Demo" codex` (if installed)
- Ensure sessions appear from iOS app.

6. Pairing QR for review
- Generate QR that points to your `wss://` URL.
- Include that QR image in Review Notes instructions (or host image URL).
- Since your product no longer uses auth token flow, QR should only encode URL/device metadata.

## Apple App Review Checklist (Must Complete)

## 1) App version metadata
- Privacy policy URL valid.
- Support URL valid.
- Marketing text/features match actual behavior.
- Screenshots show current UI.
- Mention subscription/lifetime monetization clearly in description.

## 2) IAP/subscriptions
- Products present and in correct state:
  - `mobilecli_files_yearly`
  - `mobilecli_files_lifetime`
- Include both in app version review scope where required.
- Add clear Review Notes for IAP discoverability and unlock behavior.

## 3) Review information in App Store Connect
Populate App Review Information with:
- Contact name/email/phone.
- Exact test steps.
- QR image or QR payload + expected result.
- Statement that backend/demo daemon is active 24/7 during review.

## 4) Demo account / access
If reviewer needs any credentials for hosted environment, provide non-expiring reviewer credentials.
If no sign-in is required in app, explicitly say so.

## 5) Stability gate
Before submission day:
- Fresh install on iPhone.
- Scan QR and connect.
- Open new session from mobile (verify desktop terminal appears).
- File list/read/write/rename/delete on safe test directory.
- Attach image + file from phone and confirm upload path insertion.
- Purchase yearly + restore + lifetime behavior validation in sandbox/TestFlight.
- Background/foreground reconnect test (no stuck disconnected state).

## 6) Evidence package (recommended)
Keep these ready in case Apple asks follow-up:
- 60-90 second screen recording:
  - QR scan -> connected -> open session -> file action -> attachment upload.
- Short text snippet explaining daemon architecture and security assumptions.
- Fallback test URL/QR if primary demo host has outage.

## App Review Notes Template (Paste/Adapt)

"This app connects to a developer-hosted terminal daemon over WebSocket.

Testing steps:
1. Open Settings -> Scan QR.
2. Scan this QR: [attach image / include payload].
3. Return to Sessions tab.
4. Tap + and create a new session (Shell).
5. Open Files tab to browse/edit files.
6. Open paywall from Files features and test subscription unlock.

Backend status:
- Demo daemon is live 24/7 for review at: wss://review.mobilecli.app
- No additional account registration is required for app functionality in review mode.

If anything is unreachable, contact us immediately at: [email/phone]."

## Submission Order (Recommended)

1. Ensure iOS build with correct `CFBundleVersion` is finished in EAS.
2. Verify App Store Connect build processing complete.
3. Submit IAP/subscriptions if pending review state requires it.
4. Submit app version with complete Review Notes.
5. Monitor App Review messages every few hours.
6. If asked for clarification, respond with exact steps + video link quickly.

## Go / No-Go Gate

Go only if all are true:
- `mobilecli` daemon autostart verified on demo host.
- QR pairing works from clean install.
- New session spawn works repeatedly.
- File features + attachment upload verified.
- IAP unlock + restore verified.
- Review notes complete and specific.

If any fail, do not submit until fixed.
