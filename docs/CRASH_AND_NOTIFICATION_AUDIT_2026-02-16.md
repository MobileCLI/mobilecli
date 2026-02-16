# Crash + Notification Audit (2026-02-16)

## Crash Log Findings (Build 70)

Source logs:
- `/home/bigphoot/Pictures/t/crashlog.crash`
- `/home/bigphoot/Pictures/t/crashlog2.crash`

Observed signatures:
- `EXC_BAD_ACCESS (SIGSEGV)`
- Crashed thread in `hermes` runtime / microtask drain path
- App version: `1.0.0 (70)`
- Device/OS: iPhone17,4 / iOS 26.2.1

Interpretation:
- This is a JS runtime stability issue (Hermes + RN runtime path), not the prior `react-native-screens` snapshot crash.
- Build 70 is not stable enough for App Review submission.

## Stabilization Action Applied

In mobile repo:
- `app.json`: `expo.newArchEnabled = false`
- `ios/Podfile.properties.json`: `newArchEnabled = "false"`

Goal:
- Use the conservative architecture path for release builds to reduce iOS runtime crashes.

## Notification Trigger Reality Check

Current implementation does support push notifications, but only for **waiting-for-input** states detected from CLI output patterns.

Code path:
- Wait detection and broadcast: `cli/src/daemon.rs:657` onward
- Push send to Expo: `cli/src/daemon.rs:2474` onward
- Mobile token registration: `mobile/hooks/useSync.ts` and `mobile/hooks/usePushNotifications.ts`

What is supported now:
- Tool approval / plan approval / clarifying question / awaiting response

What is NOT explicitly implemented now:
- Generic "CLI finished outputting" notifications
- Reliable semantic completion detection for arbitrary CLIs

Recommendation:
- Keep current notification scope for this App Store submission.
- Defer "done outputting" detection to a post-approval release; it needs robust semantics and per-CLI tuning to avoid noisy/incorrect notifications.
