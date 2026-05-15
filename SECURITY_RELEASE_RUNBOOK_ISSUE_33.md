# Security Release Runbook - Issue #33

Date: 2026-05-15
Audience: MobileCLI maintainers running the release candidate on real desktop and mobile devices

## Purpose

This runbook turns the issue #33 remediation plan into an executable release checklist. The release is not ready until every supported host/mobile/network cell below has a recorded pass or an explicit product decision that the cell is out of scope for this release.

The reader should be able to take a desktop release candidate, a mobile build candidate, and real devices, then prove the new auth-v2, bind, filesystem, push, installer, and documentation behavior before shipping.

Use `SECURITY_RELEASE_EVIDENCE_TEMPLATE_ISSUE_33.md` to record the evidence produced while running this checklist.

## Release Candidates Under Test

Record the exact artifacts before testing starts.

| Component | Candidate | Required evidence |
| --- | --- | --- |
| Desktop CLI | Git SHA and release archive name | `mobilecli --version`, archive checksum, host OS |
| iOS app | TestFlight/App Store candidate | marketing version, build number, device model, iOS version |
| Android app | Internal/App Bundle candidate | marketing version, versionCode, device model, Android version |
| Website/docs | Deploy preview or commit SHA | URL or commit SHA |

Expected mobile build floor for this remediation:

- iOS build: `112` or higher.
- Android versionCode: `92` or higher.

## Cross-Repository Ownership

This checkout contains three separate release surfaces. The root repository release workflow does not automatically publish or gate the nested mobile and website workspaces.

| Workspace | Release owner | Required before stable desktop release |
| --- | --- | --- |
| Root/CLI | Desktop release owner | Root CI green, tag matches `cli/Cargo.toml`, release artifacts and checksums verified |
| Mobile | Mobile release owner | Mobile CI green, EAS/TestFlight/internal build available, physical smoke complete |
| Website | Website release owner | Website CI green, deploy preview/stable deploy uses matching installer and security docs |

Record the root commit/tag, mobile commit/build IDs, and website commit/deploy URL together. Do not ship the desktop auth-enforcing stable release before the compatible mobile build is installable by the intended testers/users.

## Transport Security Decision

Auth-v2 authenticates the paired app but does not make `ws://` confidential. Record one release stance before manual testing starts:

| Stance | Allowed release claim |
| --- | --- |
| LAN plus Tailscale, no message-layer encryption | Safe on trusted LANs and Tailnets; not safe against hostile same-LAN MITM |
| Tailscale-only for remote/untrusted networks | Remote access requires Tailnet membership; same-LAN remains trusted-network only |
| `wss://` or encrypted message layer added | May claim protection against network observers only after MITM tests pass |

For the current remediation, use Tailscale for untrusted networks and do not recommend public tunnels or untrusted WiFi LAN use.

## Mobile Compatibility Release Order

Run this before the stable desktop release:

1. Verify mobile build numbers are at or above the floor listed above.
2. Run mobile CI and local preflight.
3. Confirm EAS production environment values and signing credentials.
4. Produce iOS TestFlight/App Store candidate.
5. Produce Android internal candidate only if Android is in scope for this release; otherwise record Android as deferred/internal-only in the final sign-off.
6. Install the candidate on real devices and complete the pairing/auth smoke matrix below.
7. Only after compatible mobile builds pass, publish the desktop release that enforces auth-v2.

## Preflight Gates

Run these before manual device testing. Failures block release-candidate testing unless the maintainer records why the failed gate is unrelated.

Desktop CLI:

```bash
cd cli
cargo fmt --check
cargo clippy --all-targets -- -D warnings -A dead-code
cargo test
cargo check --target x86_64-unknown-linux-gnu
```

Mobile app:

```bash
cd mobile
npx tsc --noEmit
npm run test:security
npm run preflight:release
```

Android release candidates must not use `signingConfigs.debug`. If Android is in scope, run a signing report or EAS build evidence check and attach the result.

Website/docs:

```bash
cd website
npm run build
```

Installer parity:

```bash
bash -n install.sh
bash -n website/public/install.sh
diff -u install.sh website/public/install.sh
```

Workflow syntax:

```bash
python3 -c 'import pathlib, yaml; [yaml.safe_load(p.read_text()) for p in pathlib.Path(".github/workflows").glob("*.yml")]'
python3 -c 'import pathlib, yaml; [yaml.safe_load(p.read_text()) for p in pathlib.Path("mobile/.github/workflows").glob("*.yml")]'
python3 -c 'import pathlib, yaml; [yaml.safe_load(p.read_text()) for p in pathlib.Path("website/.github/workflows").glob("*.yml")]'
```

## Host And Device Matrix

Supported host cells:

| Host | Required? | Notes |
| --- | --- | --- |
| macOS Apple Silicon | Yes | Primary local/iMac path. |
| macOS Intel | Yes, if release archive is shipped | Can be CI plus manual smoke if no physical host is available. |
| Linux x86_64 | Yes | Include a machine without Tailscale and one with Tailscale where possible. |
| Linux ARM64 | Yes, if release archive is shipped | CI build plus at least install/start smoke on real hardware or VM. |
| Windows x86_64 | Yes | Must run in user session, not as a Windows service. |

Supported mobile cells:

| Mobile | Required? | Notes |
| --- | --- | --- |
| iOS candidate | Yes | Test LAN and Tailscale. |
| Android candidate | Yes if Android ships; otherwise document internal-only status | Must verify production `ws://` behavior. |

Network cells:

| Network | Required? | Expected security property |
| --- | --- | --- |
| Same LAN | Yes | App-layer auth blocks unauthenticated clients on reachable LAN. |
| Tailscale | Yes | Tailnet limits reachability, auth-v2 remains enforced. |

## Desktop Install And Setup

Run once per host OS.

1. Install the desktop candidate from the release archive or local build.
2. Confirm `mobilecli --version` reports the candidate version.
3. Run `mobilecli setup`.
4. Select the intended connection mode for the current test cell.
5. Confirm the daemon does not silently bind `0.0.0.0` in local or Tailscale mode.
6. Confirm setup creates a first mobile credential and shows an auth-v2 QR.
7. Run `mobilecli credentials list` and confirm the credential appears without a raw token.
8. Confirm the config file contains credentials/verifiers but no raw `auth_token`.
9. On Unix hosts, confirm the config file is owner-only readable/writable.
10. On Windows, confirm the config file is under the user profile and not readable by ordinary unrelated users.

Evidence to record:

| Host | Mode | Bound addresses | Credential ID | Config permission checked | Result |
| --- | --- | --- | --- | --- | --- |

## Pairing And Auth

Run for every host/mobile/network cell.

1. Start the daemon.
2. Pair by scanning the QR code.
3. Confirm the mobile app does not show connected until after the daemon sends authenticated welcome.
4. Confirm sessions are listed after auth succeeds.
5. Restart the mobile app and confirm it reconnects without rescanning.
6. Restart the daemon and confirm the mobile app reconnects without rescanning.
7. Pair manually using URL, credential ID, server ID, and token. Confirm URL-only manual pairing is rejected or impossible.
8. Attempt to connect with an altered token. Expected: no welcome, no sessions, no push registration, and a visible auth error.
9. Run `mobilecli credentials revoke <credential_id>` for the paired device, then attempt reconnect. Expected: revoked device cannot reconnect or receive further data.
10. Run `mobilecli pair --rotate`, pair again, and confirm the old credential remains revoked.

Evidence to record:

| Host | Mobile | Network | QR pair | Manual pair | App restart | Daemon restart | Bad token rejected | Revoked rejected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |

## Session And Terminal Controls

Run for every host/mobile/network cell where pairing passes.

1. Start an existing terminal session from desktop.
2. Confirm the session appears on mobile only after auth.
3. Subscribe from mobile and confirm PTY output appears.
4. Send input from mobile.
5. Resize/rotate the mobile terminal and confirm the host session remains usable.
6. Trigger a tool-approval or wait-state notification path and confirm the mobile prompt flow works.
7. Spawn each supported profile from mobile: Claude, Codex, Gemini, OpenCode where installed, and shell.
8. Confirm unsupported absolute paths and interpreter flags cannot be spawned from mobile.
9. Close a session from mobile and confirm the desktop state updates.
10. Confirm a second mobile device with a revoked or missing credential cannot receive PTY bytes for subscribed sessions.

Evidence to record:

| Host | Mobile | Network | Session list | Subscribe | Input | Resize | Approval | Spawn profiles | Unauthorized blocked | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |

## Push Notifications

Run on iOS and Android if that platform ships.

1. Grant push permission on the mobile app.
2. Confirm push token registration happens only after auth.
3. Trigger a waiting state and confirm a notification arrives.
4. Re-register after app restart and confirm duplicate tokens do not accumulate for the same installation.
5. Revoke the credential and trigger another waiting state. Expected: no push to the revoked installation.
6. Unregister/disable push from mobile and confirm future waiting states do not notify that device.

Evidence to record:

| Mobile | Host | Network | Register after auth | Notification received | No duplicate token | Revocation suppresses push | Unregister works | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |

## Filesystem Bridge

Run for at least one host per OS family, and for every mobile platform that ships.

1. Confirm fresh setup exposes only approved/project roots, not the entire home directory.
2. List an allowed root.
3. Read an allowed file.
4. Write a small allowed file.
5. Upload an attachment into the project upload cache.
6. Attempt to read MobileCLI config/auth files. Expected: denied.
7. Attempt to read common secret paths for that OS, such as SSH keys, shell history, cloud credentials, keychains, or Windows credential locations. Expected: denied.
8. Attempt search with an excessive result limit. Expected: server clamps to configured maximum.
9. Attempt copy with content over the configured limit. Expected: denied.
10. Confirm delete and rename are unavailable or denied by default.
11. Explicitly enable destructive operations in setup/config on a throwaway directory, then confirm delete/rename work only inside the approved root.
12. If whole-home access is enabled for testing, confirm the warning is shown and sensitive denylist paths remain blocked.
13. On Windows, test drive-prefix, case-insensitive, UNC/verbatim, junction, and reserved-name behavior where practical.

Evidence to record:

| Host | Mobile | Allowed read/write | Upload | Config denied | Secret denied | Search clamped | Copy capped | Delete default denied | Opt-in delete scoped | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |

## Network And Bind Behavior

Run once per host OS, then spot-check with mobile.

1. Local mode binds loopback plus the selected LAN address.
2. Local mode does not bind all interfaces unless explicitly configured as advanced opt-in.
3. Tailscale mode binds loopback plus the selected Tailscale address.
4. Tailscale mode fails closed if Tailscale is unavailable or disconnected.
5. Custom mode warns when the address is public or all-interface.
6. PTY registration is accepted from loopback and rejected from non-loopback.
7. A legacy client that sends `hello` first receives `auth_required` and no sensitive data.
8. An idle unauthenticated socket closes after the first-message timeout.
9. An oversized first message is rejected before auth.

Evidence to record:

| Host | Mode | Loopback | Selected bind | No implicit all-interface | Tailscale fail-closed | Legacy hello rejected | Idle socket closed | Oversized first message rejected | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |

## Installer And Release Integrity

Run against the actual draft GitHub Release before publishing.

1. Confirm `SHA256SUMS.txt` exists in the release.
2. Confirm every published archive appears exactly once in `SHA256SUMS.txt`.
3. Download the installer from the website path and root path if both are distributed.
4. Install a valid archive and confirm checksum verification passes before extraction.
5. Corrupt the archive and confirm install fails before extraction.
6. Remove or rename the archive checksum entry and confirm install fails.
7. Confirm Linux, macOS, and Windows archives unpack to the expected binary name.
8. Confirm no install path executes downloaded archive contents before hash verification.

Evidence to record:

| Archive | Checksum present | Valid install | Corrupt fails | Missing checksum fails | Binary smoke | Result |
| --- | --- | --- | --- | --- | --- | --- |

## Upgrade And Migration

Run with backups of old configs/devices.

1. Upgrade a desktop config that has no credentials.
2. Confirm network/device fields are preserved.
3. Confirm remote mobile bind is locked until `mobilecli pair` or `mobilecli setup` creates credentials.
4. Upgrade a mobile app with an existing linked device that has no auth-v2 fields.
5. Confirm the app does not send privileged queued messages to the auth-v2 daemon before re-pairing.
6. Re-pair and confirm old mobile settings are replaced with auth-v2 fields.
7. Confirm uninstall/reinstall behavior is acceptable for SecureStore and AsyncStorage fallback cases on both iOS and Android.

Evidence to record:

| Scenario | Desktop state preserved | Remote bind locked | Mobile queue gated | Re-pair succeeds | Result |
| --- | --- | --- | --- | --- | --- |

## Docs And Privacy Review

Before stable release:

1. Read root README, CLI README, architecture docs, Windows docs, website docs, website privacy/terms, and LLM artifacts.
2. Confirm no page claims that auth was already token-based before this release.
3. Confirm no page claims URL-only manual pairing is sufficient.
4. Confirm no page recommends public tunnels for normal use.
5. Confirm push documentation names Expo push while Expo remains in use.
6. Confirm filesystem docs say whole-home and destructive operations are opt-in.
7. Confirm installer docs say archives are checksum verified.

Evidence to record:

| Surface | Auth-v2 accurate | Tailscale/LAN accurate | Expo push disclosed | Filesystem accurate | Installer accurate | Result |
| --- | --- | --- | --- | --- | --- | --- |

## Final Sign-Off

Release may proceed only after this table is complete.

| Area | Owner | Evidence link/path | Status |
| --- | --- | --- | --- |
| Desktop CLI local checks |  |  |  |
| Desktop cross-platform build/test |  |  |  |
| iOS physical smoke |  |  |  |
| Android physical smoke |  |  |  |
| LAN auth/bind smoke |  |  |  |
| Tailscale auth/bind smoke |  |  |  |
| Filesystem bridge smoke |  |  |  |
| Push notification smoke |  |  |  |
| Installer checksum smoke |  |  |  |
| Website/docs truth sweep |  |  |  |

Any failed row must produce either a fix before release or an explicit release-scope decision recorded in the remediation plan.
