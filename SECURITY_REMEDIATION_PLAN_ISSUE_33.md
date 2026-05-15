# Security Remediation Plan - Issue #33

Date: 2026-05-15
Status: local remediation implemented in this checkout; release/manual-device gates still required before shipping
Audience: MobileCLI maintainers and implementation agents

## Summary

Issue #33 was substantially true against the pre-remediation code. The desktop daemon treated network reachability as the security boundary, while public docs described application-layer token authentication that did not exist. The fix needed to be coordinated across the desktop daemon, protocol, mobile app, installer/release pipeline, and public docs.

This is not a daemon-only change. A new mobile build is required because the bundled app code controls QR parsing, token storage, WebSocket handshake order, queued message flushing, push-token registration, and reconnect behavior. OTA updates are not available in the current native app configuration, so the mobile compatibility release must ship before the desktop daemon enforces fail-closed auth.

The implemented local end state is protocol v2 challenge-response auth, not a raw bearer token sent over plaintext `ws://`. Tailscale remains recommended transport isolation, but Tailscale is not the only auth boundary.

## Local Implementation Status

Implemented locally on 2026-05-15:

- Desktop auth-v2 protocol messages, credential generation, verifier storage, fail-closed mobile handshake, and post-auth welcome/session flow.
- Authenticated local PTY registration for desktop wrapper sessions so arbitrary loopback WebSocket clients cannot register fake sessions or poison project roots.
- First-message timeout and first-message size cap for unauthenticated WebSockets.
- `mobilecli pair`, `mobilecli pair --rotate`, `mobilecli credentials list`, and `mobilecli credentials revoke <credential_id>`.
- Mobile QR parsing for auth-v2 fields, SecureStore-only per-device pairing-token storage, metadata-only AsyncStorage fallback, challenge-response proof generation, authenticated connection state, queued-message gating, and push registration after auth.
- Durable mobile push opt-out state so offline unregister requests survive app termination and drain after the next authenticated reconnect.
- Loopback plus configured LAN/Tailscale bind policy; no default all-interface bind.
- Active-credential filtering on sensitive broadcasts, PTY output, and file-watch sends so revoked credentials stop receiving data after revocation.
- Session-control messages now require both the `session:control` scope and an active subscription to the target session.
- Filesystem hardening defaults, expanded sensitive-path denylist including MobileCLI auth config, safe project-root inclusion for active sessions, search clamp, copy-size enforcement, destructive-operation config enforcement with delete/rename off by default, explicit empty-root deny-all preservation, and mobile spawn profile normalization.
- Push-token ownership, per-credential caps, and token format validation.
- Unix private config writes and Windows ACL tightening for config files/directories; Windows save now fails visibly if ACL hardening fails.
- Windows filesystem pattern matching now normalizes verbatim paths, separators, and case before denied/read-only glob checks.
- Auth material no longer falls back to a project-local `./.mobilecli` directory when the home directory is unavailable; the CLI uses a platform config directory or fails closed.
- Android release builds no longer use the debug signing config; mobile release preflight fails if that pattern returns.
- Linux Tailscale setup now gives manual installation instructions instead of running `curl | sh`.
- Installer checksum verification in both root and website installer scripts.
- Root release workflow gates for CLI fmt/clippy/test, root installer syntax, tag/version sync, native runner tests on release builders, and checksum completeness across expected Linux/macOS/Windows archives.
- Installer checksum helper now has a local shell test covering valid, star-prefixed, missing, invalid, wrong, and empty checksum manifests.
- Per-workspace CI gates: root Linux/macOS/Windows desktop compile checks; mobile TypeScript/security/preflight checks; website build and public installer syntax.
- Public docs/website/LLM artifacts updated from temporary network-only language to the implemented auth-v2 model.
- Mobile native build numbers synchronized to iOS build 112 and Android versionCode 92.

Still required before shipping:

- Physical-device smoke tests on iOS and Android for LAN and Tailscale, including app restart, daemon restart, bad token, revoked credential, push registration, attachments, filesystem allowed/denied roots, and production Android cleartext behavior.
- Cross-platform desktop test/build matrix on Linux, macOS Apple Silicon/Intel, and Windows x64.
- Release-artifact verification against the actual GitHub Release `SHA256SUMS.txt`.
- Transport-confidentiality release decision: auth-v2 authenticates the mobile client but does not encrypt `ws://` terminal/filesystem frames. Same-LAN use must be limited to trusted networks, Tailscale must be required for untrusted networks, or an encrypted/authenticated message layer must be added before claiming protection against LAN MITM.
- Dedicated full hook/integration tests for mobile WebSocket behavior and SecureStore migration are still recommended; the current local security test script covers QR parsing, auth-v2 proof fixtures, redundant storage fallback semantics, and pure auth state-machine gates used by `useSync`.
- Execute the release-candidate runbook in `SECURITY_RELEASE_RUNBOOK_ISSUE_33.md` and record pass/fail evidence for every supported host/mobile/network cell.
- Record release-candidate evidence in `SECURITY_RELEASE_EVIDENCE_TEMPLATE_ISSUE_33.md`.
- Use `SECURITY_COMPLETION_AUDIT_ISSUE_33.md` as the prompt-to-artifact checklist before marking the remediation complete.

## Pre-Remediation Truths Confirmed

- Desktop WebSocket auth is missing. The daemon accepts mobile clients, registers them, and sends `welcome`, session lists, and waiting states before any credential check.
- QR pairing currently contains connection metadata only. It does not contain a secret.
- `Config` stores device/network fields only. It does not store an auth token hash or verifier.
- Mobile already has partial `authToken` plumbing, but it marks the raw socket as connected and sends `hello`, `get_sessions`, push registration, and queued commands before auth is proven.
- Mobile parses `auth_token`, not Claude's proposed `token`. The daemon should emit `auth_token`; the app should accept both `auth_token` and `token` for migration tolerance.
- Push notifications use Expo push service. Docs claiming direct APNs or no third-party service are false while Expo remains the push path.
- The mobile app, website, and root CLI are separate workspaces in this checkout. Root GitHub CI currently gates only the CLI repository surface.

## Target Architecture

### Auth Model

Implement protocol v2 challenge-response authentication for all non-PTY mobile WebSocket clients.

Pairing creates a credential:

- `server_id`: stable desktop daemon identity.
- `credential_id`: stable ID for one paired mobile installation.
- `auth_token`: 32 bytes from OS RNG, base64url encoded, shown only in the QR/manual pairing flow.
- `verifier`: derived from `auth_token` and stored on desktop. Treat this verifier as sensitive because it is pass-the-hash material for challenge-response verification.
- `scopes`: explicit allowed capabilities for that credential.
- `created_at`, `last_used_at`, `revoked_at`, and display name.

Protocol flow:

1. Client opens WebSocket.
2. Client sends `auth_start` with `auth_version: 2`, `credential_id`, `client_nonce`, `mobile_installation_id`, `sender_id`, `client_version`, and `client_capabilities`.
3. Server replies with `auth_challenge` containing `server_nonce`, `server_id`, and `credential_id`.
4. Client sends `auth_response` with `proof = HMAC-SHA256(verifier, transcript)`.
5. Server constant-time compares the proof, updates credential last-used metadata, registers the mobile client, and only then sends `welcome`, session list, waiting states, and any queued authorized data.

Fail closed:

- No `welcome`, sessions, waiting states, filesystem watcher, push registration, PTY output, or client registration before auth succeeds.
- Legacy first-message `hello` without v2 auth gets `auth_required` and a policy-violation close.
- Wrong proof gets `auth_invalid` and a policy-violation close.
- Missing local credential storage or tokenless migrated config locks remote mobile bind until the user runs `mobilecli setup` or `mobilecli pair`.

Compatibility:

- The new mobile app should support legacy daemons and auth-v2 daemons.
- The new desktop daemon should not support unauthenticated mobile clients.
- If an emergency server-only bridge is needed, `hello.auth_token` can be considered as a temporary auth-v1 path, but it should not be the final design because it sends a reusable bearer token over `ws://`.

### Transport Confidentiality

Auth-v2 proves that a paired mobile installation knows the pairing secret, but `ws://` is still cleartext. A hostile same-LAN intermediary could read or tamper with post-auth terminal and filesystem frames unless the network itself is trusted or protected by Tailscale/WireGuard.

Release posture:

- Same-LAN mode is acceptable only as a trusted-network convenience, not as protection against a LAN MITM.
- Tailscale is the recommended remote/untrusted-network transport because it gives confidentiality and peer authentication below the WebSocket.
- Public tunnels and untrusted WiFi must not be recommended unless the connection is `wss://` with a trusted endpoint or MobileCLI adds its own encrypted/authenticated message layer after pairing.
- Android cleartext support must remain a deliberate compatibility decision for LAN/Tailscale `ws://`, not an accidental blanket security claim.

### Desktop Config

Extend the desktop config format:

- Add `config_version`.
- Add `server_id`.
- Add `auth_version`.
- Add `credentials`.
- Add `filesystem.allowed_roots` and destructive-operation policy.
- Preserve existing `device_id`, `device_name`, `connection_mode`, `tailscale_ip`, and `local_ip`.

Persistence requirements:

- Atomic writes.
- Unix config files written with owner-only permissions.
- Best-effort private ACLs on Windows.
- No raw `auth_token` stored on desktop.
- `mobilecli pair` creates a fresh credential every time because old raw tokens cannot be redisplayed safely.

New CLI operations:

- `mobilecli pair`: create and show a new credential QR.
- `mobilecli pair --rotate`: revoke old mobile credentials for this desktop and create a new one.
- `mobilecli credentials list`: show paired devices without secrets.
- `mobilecli credentials revoke <credential_id>`: revoke one paired mobile device.
- `mobilecli setup`: migrate tokenless config, select connection mode, create first credential, and show QR.

### Bind Policy

Replace unconditional `0.0.0.0:<port>` mobile exposure with explicit bind selection.

Required behavior:

- PTY registration remains loopback-only.
- The daemon always has a loopback listener for desktop wrapper/link traffic.
- Local mode binds the selected LAN IP, not every interface.
- Tailscale mode binds the Tailscale IP and fails closed if Tailscale is unavailable.
- Custom mode binds only according to explicit user configuration and warns if public exposure is possible.
- `0.0.0.0` becomes explicit advanced opt-in, not the default.

Cross-platform considerations:

- Linux/macOS/Windows must all support loopback plus selected mobile bind.
- IP changes require a clear restart/re-pair message.
- Windows users must keep the daemon in the user session, not a Windows service, so spawned terminals remain visible.
- Firewall docs should say exactly which address and port are bound.

## Implementation Slices

### Slice 0 - Release And Branch Hygiene

- Freeze feature work touching daemon protocol, mobile sync, QR pairing, filesystem bridge, installer, and docs until the remediation lands.
- Treat current docs as unsafe until updated.
- Keep `SECURITY_AUDIT_ISSUE_33.md` as evidence and use this file as the execution plan.
- Decide release numbers before code starts. Assuming iOS build `111` has already been used, the next mobile build should be at least `112`; Android versionCode should be at least `92`. Use the next marketing version that matches App Store / Play Console state.

### Slice 1 - Mobile Compatibility Release

Goal: ship an app that can talk to both legacy daemons and auth-v2 daemons before the desktop daemon starts enforcing auth.

Mobile changes:

- Clean up QR parsing so special `mobilecli://relay`, `mobilecli://tailscale`, and `mobilecli://direct` formats are not swallowed by the generic compact parser.
- Parse `auth=v2`, `credential_id`, `server_id`, `auth_token`, `token`, `wss=1`, and `ws_url`.
- Avoid logging full QR payloads or auth tokens, even in dev logs.
- Store pairing secrets with existing `expo-secure-store` keychain service. Do not enable biometric `requireAuthentication`; reconnect/background behavior depends on silent access.
- Add a SecureStore-backed `mobile_installation_id` that survives app restarts and is distinct from the desktop `device_id`.
- Add HMAC-SHA256 support for auth-v2, likely with a small audited JS dependency such as `@noble/hashes` unless the native stack already provides a reliable HMAC API.
- Change connection state from `socket open == connected` to `authenticated welcome == connected`.
- On WebSocket open, send auth-v2 messages when the active device has v2 fields.
- Wait for auth success / `welcome` before setting `isConnected`, requesting sessions, registering/unregistering push tokens, sending filesystem requests, or flushing queued messages.
- Clear queued privileged messages on auth failure.
- Handle `auth_required`, `auth_invalid`, `auth_revoked`, JSON error messages, and close-code-only failures without retry loops.
- Add a visible manual token entry field in Settings or make manual URL pairing explicitly QR-only. Recommended: add manual fields for URL, credential ID, and token.
- Register push tokens only after auth succeeds and include `mobile_installation_id`.
- Fix the current TypeScript blocker: `TMUX_SWIPE_COOLDOWN_MS` undefined.
- Fix native version/build sync before any build: app config, iOS project, iOS Info.plist, and Android Gradle must agree.
- Test Android production `ws://` behavior. Debug allows cleartext; production must be verified for LAN/Tailscale WebSockets.

Mobile acceptance tests:

- QR parser unit tests for JSON, compact URL, `auth_token`, `token`, `auth=v2`, `wss=1`, relay/tailscale/direct, IPv6, and malformed values.
- Mock WebSocket tests proving auth is sent first, `isConnected` stays false until `welcome`, push registration waits for auth, queued messages flush only after auth, and auth failure clears privileged queues.
- SecureStore migration tests for existing linked devices without tokens and rescans with new credentials.
- Physical-device smoke tests for iOS LAN, iOS Tailscale, Android production LAN, Android production Tailscale, app restart, daemon restart, and push opt-in.

Release order:

- Build locally on the iMac for smoke testing if useful.
- Use the same signing/env/version inputs as production EAS.
- EAS remains the release path unless the team explicitly switches release process.
- Ship TestFlight/internal build first; only enforce desktop auth after the compatible mobile build is available.

### Slice 2 - Desktop Auth And Pairing

Goal: the daemon must reject unauthenticated mobile clients before any sensitive data leaves the desktop.

Desktop changes:

- Add auth-v2 protocol messages and server messages.
- Refactor `handle_connection` so mobile auth completes before calling the current mobile client loop.
- Do not create filesystem watcher subscriptions, broadcast receivers, mobile client map entries, sender IDs, or capability records until auth succeeds.
- Reduce unauthenticated WebSocket message/frame limits. The current 96 MB cap should apply only after auth or be replaced by authenticated chunked upload.
- Add token generation, verifier derivation, credential storage, credential list/revoke/rotate, config migration, and secure config persistence.
- `mobilecli setup` must create credentials before daemon state is initialized, or restart/reload the daemon after setup so the running daemon has the new auth state.
- Existing tokenless configs preserve network/device fields but lock remote mobile bind until pairing creates credentials.
- `mobilecli pair` should create a new credential and show a QR once; it should not attempt to redisplay old secrets.
- Never log full QR URLs, raw tokens, HMAC proofs, or verifiers.
- Return stable auth errors: `auth_required`, `auth_invalid`, `auth_revoked`, `auth_unsupported`, and `auth_locked`.

Desktop auth tests:

- No-token client gets no `welcome`, no sessions, no waiting states, and closes.
- Bad proof closes without registering a mobile client.
- Good proof gets `welcome` and can request sessions.
- Legacy `hello` gets `auth_required` and closes.
- Revoked credential cannot reconnect.
- Config migration locks remote bind until pairing.
- Config writes contain no raw token and use expected permissions.
- PTY `register_pty` remains loopback-only.

### Slice 3 - Authorization And Capability Scopes

Goal: after auth, the daemon still enforces least privilege by credential/scope instead of trusting arbitrary session IDs.

Scopes:

- `session:read`
- `session:control`
- `session:spawn`
- `fs:read`
- `fs:write`
- `fs:delete`
- `fs:watch`
- `fs:upload`
- `push:register`

Session controls:

- Gate `Subscribe`, `GetSessionHistory`, `SendInput`, `ToolApproval`, `PtyResize`, `TmuxViewport`, `RenameSession`, and `CloseSession`.
- Do not use `owner_connection_id` as the primary authorization model. It breaks reconnects and multi-device use. Use authenticated `credential_id`, scopes, and session ACLs where needed.
- For input/resize/viewport, require the connection to be subscribed or explicitly controlling the session.
- Ensure old/no-capability clients do not receive `PtyBytes` for sessions they did not subscribe to.

Push tokens:

- Bind tokens to `credential_id` and `mobile_installation_id`.
- Validate `token_type` and token format.
- Cap tokens per credential.
- Re-registration replaces the same mobile installation's token.
- Unregister only removes tokens owned by the authenticated credential.
- Credential revocation purges related push tokens.
- Rate-limit by credential and IP.
- Keep docs honest that Expo push is used unless the implementation changes.

### Slice 4 - Filesystem Hardening

Goal: the filesystem bridge should be useful without defaulting to whole-home access.

Desktop changes:

- Change default roots from whole home to active session project roots plus explicit setup-approved roots.
- Make whole-home access an explicit opt-in with a clear warning.
- Add per-root permissions: read, write, delete, watch, upload.
- Keep an expanded denylist as defense-in-depth, not as the primary security boundary.
- Block known secret locations across Linux, macOS, and Windows by default, including SSH, GPG, AWS, kube, Docker, gcloud, shell histories, netrc, Git credentials, Vault, Terraform state, browser credential stores, keychains, and password-manager config.
- Clamp client-provided search result limits to the configured maximum.
- Enforce write-size limits for copy operations too.
- Validate mobile `working_dir` for spawned sessions against approved roots or session project roots.
- Harden Windows paths: case-insensitive matching, drive prefixes, UNC/verbatim paths, junctions/reparse points, reserved names, and symlink race behavior.

Mobile changes:

- Treat denied roots as normal UX, not connection failure.
- Show explicit root/permission state in the file browser where needed.
- Do not offer destructive file actions when the authenticated credential lacks `fs:delete`.

Filesystem tests:

- Fresh config cannot read arbitrary home files.
- Session project roots are readable where intended.
- Whole-home opt-in works only after explicit setup approval.
- Sensitive paths are denied even under allowed roots.
- Copy honors size limits.
- Search clamps `max_results`.
- Windows junction/reparse/UNC cases are denied or handled as specified.

### Slice 5 - Command Spawn Hardening

Goal: mobile spawn should start supported tools, not arbitrary interpreter payloads.

Desktop changes:

- Replace free-form mobile `command,args` with server-defined spawn profiles: `claude`, `codex`, `gemini`, `opencode`, and `shell`.
- Existing mobile requests with `command` and empty `args` may be translated to profiles for compatibility.
- Reject absolute command paths from mobile clients.
- Reject non-empty args by default unless a profile explicitly allows them.
- For the `shell` profile, spawn the user's default shell without accepting client-provided flags.
- If advanced arbitrary command spawning is kept, put it behind an explicit config flag and a separate high-risk scope.
- Fix basename allowlist bypasses such as `/tmp/bash`.
- Do not rely on `is_shell_safe` for security. Escaping is still useful, but interpreter flags are the real risk.

Tests:

- Allowed profile spawns work on Linux, macOS, and Windows.
- `/tmp/bash`, `bash -c`, `python -c`, `node -e`, `powershell -Command`, and `powershell -EncodedCommand` are rejected.
- Working directories outside approved roots are rejected.
- Existing mobile UI spawn buttons continue to work.

### Slice 6 - Bind, Setup, Autostart, And Cross-Platform UX

Goal: safe defaults work on Linux, macOS, and Windows.

Desktop changes:

- Implement pure bind-address selection logic and test it independently.
- Local mode binds selected LAN IP.
- Tailscale mode binds selected Tailscale IP and fails closed if disconnected.
- Custom mode warns and requires explicit confirmation for public or all-interface exposure.
- Never silently fall back from Tailscale to LAN when Tailscale is selected.
- Setup should default to Tailscale as the recommended remote mode and explain LAN risk plainly.
- Auto-launch shell hook should remain opt-in and default to `No` during setup.
- Tailscale Linux install helper should give manual package-manager/download instructions and not run `curl | sh`.
- Audit autostart outputs while touching setup: macOS launchd plist, Windows Task Scheduler command, Linux systemd user service.

Tests:

- Bind selection: loopback always available, LAN binds selected local IP, Tailscale binds selected `100.x` address, no implicit `0.0.0.0`.
- Tailscale disconnected in Tailscale mode refuses pairing instead of showing LAN QR.
- Windows setup docs and behavior keep daemon in the user session.
- Shell hook install/uninstall remains reversible.

### Slice 7 - Installer And Release Integrity

Goal: downloads are verified before execution or privileged install.

Changes:

- Update root `install.sh` and `website/public/install.sh` together.
- Use the existing `SHA256SUMS.txt` from GitHub Releases or switch release workflow to per-archive `.sha256`; pick one and test it. Recommended: use existing `SHA256SUMS.txt` to minimize release workflow churn.
- Download checksum file before extraction.
- Verify the selected archive hash before `tar`/`unzip` and before `sudo`.
- Fail closed if checksum is missing or verification fails.
- Prefer fail-closed if no checksum tool exists; if a bypass flag is added, make it explicit and noisy.
- Keep the sudo warning, but the main supply-chain fix is checksum verification.
- Add release workflow checks that every archive appears in checksum output.
- Add signing/notarization as a follow-up hardening track: cosign/Sigstore for artifacts, macOS codesign/notarization if distributing outside Homebrew, Windows Authenticode if distributing `.exe`.

Tests:

- Valid checksum installs.
- Corrupted archive fails.
- Missing checksum fails.
- Wrong archive name fails.
- Website installer copy matches root installer.

### Slice 8 - Docs, Website, Privacy, And Disclosure

Goal: docs must describe the implemented security model, not the intended one.

Before code ships:

- Replace current auth-token claims with a temporary warning: current released daemon relies on network isolation and should be used only on trusted LANs or locked-down Tailnets.
- Remove public tunnel recommendations until app-layer auth is released.
- State that QR currently contains connection metadata only.
- State that the daemon exposes high-privilege terminal and filesystem capabilities to reachable clients.
- State that push notifications use Expo push service.
- State that installer checksum verification is being added if not yet shipped.

After fixes ship:

- Add a canonical Security and Privacy Model section reused across root README, CLI README, docs, and website.
- Document auth-v2 pairing, token storage, QR/manual entry, rotation, revocation, failed-auth logs, and re-pairing.
- Document LAN, Tailscale, and custom modes honestly.
- Keep Expo in privacy/docs unless push implementation changes.
- Document filesystem root approval and whole-home opt-in.
- Document release checksum verification.

Sweep areas:

- `README.md`
- `cli/README.md`
- `docs/ARCHITECTURE_QUICK_REFERENCE.md`
- `docs/WINDOWS_SETUP.md`
- `website/src/pages/index.astro`
- `website/src/pages/docs/*`
- `website/src/pages/features.astro`
- `website/src/pages/pricing.astro`
- `website/src/pages/privacy.astro`
- `website/src/pages/terms.astro`
- `website/src/lib/constants.ts`
- `website/public/llms.txt`
- `website/public/llms-full.txt`
- blog and comparison pages with durable no-cloud/no-third-party/no-shell claims
- `website/README.md`

## Verification Gates

### CLI Gates

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --all-targets -- -D warnings -A dead-code`
- `cargo test`
- `cargo check --target x86_64-unknown-linux-gnu`
- `cargo check --target x86_64-pc-windows-gnu` when the target is available locally.
- Cross-platform build/test on Linux, macOS, and Windows.
- Tmux-dependent tests skip cleanly or run in an environment where tmux sockets are permitted.

Local status:

- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings -A dead-code`, and `cargo test` passed in this checkout on 2026-05-15.
- `cargo check --target x86_64-unknown-linux-gnu` and `cargo check --target x86_64-pc-windows-gnu` passed locally on 2026-05-15.
- Desktop auth parsing/verification now has pure unit coverage for legacy first-message rejection, unsupported auth versions, unknown/revoked credentials, transcript mismatch, invalid proof, and a valid bound proof.
- Local PTY registration proof, daemon-side PTY registration validation, and safe session-root filtering have focused unit coverage; explicit empty filesystem roots stay deny-all.
- Windows-style filesystem match normalization and config-directory fallback behavior have focused unit coverage.
- Root PR CI now includes `cargo check` on `ubuntu-latest`, `macos-latest`, and `windows-latest`.
- Full cross-platform Linux/macOS/Windows release build/test still needs to run outside this sandbox because macOS and Windows MSVC runners are not available here.

### Mobile Gates

- `npx tsc --noEmit`
- `npm run preflight:release`
- `npm run test:security` for QR parser, auth-v2 proof fixtures, redundant storage fallback semantics, and pure auth state-machine gates.
- WebSocket auth state machine tests.
- SecureStore/AsyncStorage fallback tests.
- Physical iOS and Android smoke tests.

Local status:

- `TMUX_SWIPE_COOLDOWN_MS` is defined.
- Native versions/build numbers are synchronized to iOS build 112 and Android versionCode 92.
- `npx tsc --noEmit`, `npm run test:security`, and `npm run preflight:release` passed locally.
- Pairing tokens are stored per device in SecureStore only; AsyncStorage stores device metadata without `authToken`/`auth_token`. Old inline token JSON is migrated and scrubbed.
- Pending push-token unregister state is persisted so notification opt-out can be drained after app restart and the next authenticated reconnect.
- Full mock WebSocket hook tests and SecureStore migration integration tests are not yet automated, but pure state gates, token-free metadata serialization, and redundant storage behavior are covered by `npm run test:security`.
- EAS production env var checks still warn if EAS auth/network is unavailable.
- Mobile workspace PR CI now runs dependency install, TypeScript check, `npm run test:security`, and release preflight.

### Website Gates

- `npm run build`
- No false security/privacy claims after the docs sweep.
- LLM artifacts regenerated or updated if they are generated manually.
- Website install script matches root install script.

Local status:

- `npm run build` passed in `website/`.
- Root and website installer scripts are byte-for-byte identical and pass `bash -n`.
- Targeted stale-claim scan found no remaining network-only auth claims or URL-only manual-pairing instructions in root docs, CLI docs, website source, or LLM artifacts.
- Website workspace PR CI now checks public installer syntax and website build before merge.
- Cross-repository installer parity still requires local/release coordination because root, website, and mobile are separate Git workspaces in this checkout.

### Manual E2E Matrix

The executable release checklist lives in `SECURITY_RELEASE_RUNBOOK_ISSUE_33.md`. The matrix below is the release-blocking coverage summary; use the runbook for exact commands, scenarios, and evidence tables.

Host operating systems:

- macOS Apple Silicon
- macOS Intel
- Linux x86_64
- Linux ARM64
- Windows x86_64

Mobile clients:

- iOS TestFlight/App Store candidate
- Android internal candidate if Android is still in scope

Networks:

- Same LAN
- Tailscale

Per-cell scenarios:

- Fresh setup
- QR scan
- token persistence
- app restart reconnect
- daemon restart reconnect
- bad token rejection
- revoked credential rejection
- session list
- subscribe to existing session
- spawn Claude/Codex/Gemini/shell profile
- send input
- approval flow
- close session
- push register/unregister
- file list/read/write in allowed root
- denied sensitive path
- whole-home opt-in if enabled
- desktop upgrade from tokenless config
- mobile upgrade from existing pairing

Release is blocked unless every supported cell passes or the unsupported cell is explicitly documented.

## Rollout Order

1. Patch public docs with temporary truth if we need to reduce user risk before code ships.
2. Build and release the mobile compatibility app first. Use at least the next unused iOS build number after `111` and the next unused Android versionCode after `91`.
3. Ship a desktop prerelease with auth-v2 enforced, safe migration, credential pairing, bind policy, command spawn hardening, filesystem root changes, push-token ownership, and installer checksum verification.
4. Run the full CLI, mobile, website, and manual E2E gates.
5. Update docs from temporary warnings to the final auth-v2 security model.
6. Publish stable CLI release.
7. Monitor support channels for auth failures, pairing confusion, Tailscale bind failures, mobile reconnect loops, and installer verification failures.

## Explicit Non-Goals For The First Remediation Release

- Do not implement a cloud relay.
- Do not claim end-to-end encryption beyond the chosen transport and auth properties.
- Do not claim “no third-party services” while Expo push, App Store, RevenueCat, or other processors remain in use.
- Do not implement arbitrary mobile command execution as a default feature.
- Do not expose public tunnels or reverse proxies as recommended setup paths.
- Do not silently preserve unauthenticated legacy mobile access for convenience.

## Open Decisions To Confirm Before Release

- Whether to ship an emergency desktop-only `hello.auth_token` hotfix before the full auth-v2 rollout. Recommended default: skip unless there is an immediate release pressure.
- Whether Android is part of this security release or remains internal only. If it is included, production cleartext WebSocket behavior must be verified.
- Whether whole-home filesystem access should remain available as an advanced opt-in. Recommended default: yes, but off by default with a strong warning.
- Whether artifact signing is required in the same release as checksum verification. Recommended default: checksum verification now, signing as the next hardening milestone.
