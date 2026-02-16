# App Store Compliance Audit (2026-02-16)

## Scope

This audit covered:
- iOS app compliance-sensitive flows (permissions, purchases, privacy links, reviewability)
- App Review testability requirements (stable backend + deterministic reviewer steps)
- Current demo server readiness for App Review execution

## Sources Used

- Apple App Review Guidelines: https://developer.apple.com/app-store/review/guidelines/
- App Store Connect Help (In-App Purchase): https://developer.apple.com/help/app-store-connect/manage-subscriptions/
- App Store Connect Help (Submission): https://developer.apple.com/help/app-store-connect/manage-submissions-to-app-review/

## Findings (Ordered by Severity)

1. High: Demo review hostname not yet pointed to active review server
- `demo.mobilecli.app` still resolves to old IP (`89.167.6.36`), so TLS issuance failed.
- Impact: App Review cannot use reviewer URL/QR over `wss://` until DNS is fixed.
- Status: Pending user DNS update.

2. Medium: Yearly CTA wording implied trial availability for all users
- Previous CTA text could be interpreted as guaranteed trial availability.
- File: `mobile/app/paywall.tsx`
- Fix applied: CTA changed to neutral text (`Continue with Yearly`).
- Status: Fixed and pushed (`MobileCLI/mobile` commit `5b98d9f`).

3. Medium: Review relies on live backend behavior
- App Review will fail if demo daemon/QR flow is unavailable during review window.
- Status: Mitigated by persistent daemon + demo-session keeper on Hetzner.

4. Low: Submission metadata must be fully aligned with real behavior
- Ensure review notes, IAP listing, and test instructions exactly match current app behavior.
- Status: Requires App Store Connect data-entry checks before submit.

## Compliance Checks Completed (Pass)

- Terms and Privacy links available and returning HTTP 200:
  - `https://www.mobilecli.app/terms`
  - `https://www.mobilecli.app/privacy`
- Purchase UX has required controls:
  - Restore purchases button present (`mobile/app/paywall.tsx`)
  - Manage subscription entry point present (`mobile/app/paywall.tsx`, `mobile/app/(tabs)/settings.tsx`)
  - Pricing displayed for yearly/lifetime (`mobile/app/paywall.tsx`)
- Permission purpose strings exist for camera/photo/local network (`mobile/app.json`, `mobile/ios/MobileCLI/Info.plist`)
- Push notifications are opt-in by default (`mobile/hooks/useSettings.ts` notifications default false)
- Build number alignment check passed in source:
  - `mobile/ios/MobileCLI/Info.plist` `CFBundleVersion=70`
  - `mobile/ios/MobileCLI.xcodeproj/project.pbxproj` `CURRENT_PROJECT_VERSION=70`

## Review Environment Status

Provisioned and running on Hetzner `65.21.108.223`:
- `mobilecli-daemon.service`
- `xvfb-mobilecli.service`
- `mobilecli-demo-keeper.service` (persistent shell demo)
- nginx websocket proxy configured
- firewall configured (`22`, `80`, `443`)

Operational verification completed:
- Remote session spawn works
- Demo session remains available

## Go/No-Go for Submission

Current state: **No-Go until DNS cutover + TLS pass**

Go criteria:
1. `demo.mobilecli.app` resolves to `65.21.108.223`
2. TLS cert issued successfully and endpoint reachable over `https://` / `wss://`
3. Fresh iPhone test passes full reviewer flow:
   - Scan QR
   - Connect
   - Open existing session
   - Spawn new session
   - File browse/edit
   - Attachment upload
   - Purchase + restore

## Minimal Reviewer Flow (for Review Notes)

1. Open app and go to Settings.
2. Scan provided reviewer QR.
3. Return to Sessions tab.
4. Open "Shell Demo".
5. Tap + to spawn a new shell session.
6. Open Files and edit any test file.
7. Use attachment button to upload an image/file to the desktop session.
8. Open paywall and verify restore/manage controls are present.
