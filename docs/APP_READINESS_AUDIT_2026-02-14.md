# App Readiness Audit - 2026-02-14

Scope: desktop daemon + mobile app critical paths for TestFlight readiness.

## 1. What Was Verified

## Desktop (Rust)
- `cargo test -q`: pass (`17 passed`).
- `cargo clippy --all-targets -- -D warnings`: pass.
- Websocket protocol smoke checks (live daemon on localhost):
  - PTY register handshake: pass.
  - Upload filename sanitization cases: pass.
    - traversal-style name -> sanitized filename.
    - Windows reserved names -> safe suffix.
    - empty filename -> fallback attachment name.
    - whitespace/invalid chars -> normalized underscores.
  - Oversize upload (>50MB raw): returns structured `file_too_large` (no websocket disconnect).

## Mobile (TypeScript)
- `npx tsc --noEmit`: pass after changes.
- Payment codepaths reviewed and updated in `hooks/usePremium.ts` and `app/paywall.tsx`.

## 2. Fixes Applied During Audit

## A. Upload robustness (desktop)
- File: `cli/src/daemon.rs`
- Fixes:
  - Websocket frame/message limits raised for large base64 payload headroom.
  - Upload filename sanitization hardened (invalid chars, reserved names, UTF-8 safe truncation).
  - Conservative filename budget introduced to avoid `ENAMETOOLONG` with atomic temp write suffixes.
  - Added/updated upload-related tests.

## B. Attachment + timeout robustness (mobile)
- File: `mobile/components/TerminalView.tsx`
  - Attachment pre-read size checks and clearer error handling.
- File: `mobile/hooks/useSync.ts`
  - Upload request timeout now scales by payload size.

## C. Payment flow hardening (mobile)
- File: `mobile/hooks/usePremium.ts`
  - Package discovery now searches across all offerings, not only `offerings.current`.
  - Purchase flow refreshes packages if missing before purchase attempt.
  - "already subscribed/already purchased" errors now:
    - trigger `syncPurchases` (if supported),
    - refresh entitlements,
    - succeed if entitlement is active,
    - otherwise show explicit entitlement-mapping guidance.
- File: `mobile/app/paywall.tsx`
  - Yearly/lifetime CTA buttons now reflect package availability and disable cleanly when unavailable.

## D. Session terminal UX cleanup (mobile)
- File: `mobile/app/session/[id].tsx`
  - removed unused callback wiring.
- File: `mobile/components/TerminalView.tsx`
  - retained single Esc affordance in toolbar path (no duplicate top-right control in session view).

## 3. Current Non-Blocking Warnings

## Expo doctor
- 2 checks failing:
  - Native config sync warning for project with committed `ios/` + `android/` and app.json native fields.
  - SDK package version drift (`expo`, `expo-router`, `expo-font`, `babel-preset-expo`, etc.).

## npm audit (prod deps)
- Moderate advisory chain:
  - `react-native-markdown-display` -> `markdown-it` uncontrolled resource consumption advisory.
  - No direct fix available from current dependency tree.

## 4. Store-Submission Checklist Before Next Build

1. Confirm RevenueCat offering has both package types visible in `current` offering on each platform:
   - annual package (subscription)
   - lifetime package (non-consumable)
2. Confirm entitlement mapping in RevenueCat:
   - entitlement id used by app (default `files`) must include both products.
3. Verify EAS env vars are set for correct public keys:
   - `EXPO_PUBLIC_REVENUECAT_IOS_API_KEY=appl_...`
   - `EXPO_PUBLIC_REVENUECAT_ANDROID_API_KEY=goog_...`
4. Run one on-device purchase matrix on sandbox users:
   - fresh user -> yearly purchase -> entitlement active
   - user with existing yearly -> yearly tap shows already-owned handling and remains unlocked
   - fresh user -> lifetime purchase -> entitlement active
   - reinstall + restore purchases -> entitlement active
5. Decide whether to pin/update Expo package versions now or defer until after this release.

