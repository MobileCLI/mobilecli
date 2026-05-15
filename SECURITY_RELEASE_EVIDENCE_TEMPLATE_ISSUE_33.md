# Security Release Evidence Template - Issue #33

Copy this template for the actual release candidate evidence packet. Do not use it as a substitute for running `SECURITY_RELEASE_RUNBOOK_ISSUE_33.md`; this file is where the resulting evidence is recorded.

## Release Manifest

| Surface | Value | Evidence |
| --- | --- | --- |
| Root repository commit |  | `git rev-parse HEAD` |
| Root release tag |  | `git tag --points-at HEAD` or release URL |
| CLI crate version |  | `cli/Cargo.toml` package version |
| Mobile repository commit |  | `git -C mobile rev-parse HEAD` |
| iOS build |  | EAS/TestFlight build URL and build number |
| Android build |  | EAS/internal build URL and versionCode, or deferred decision |
| Website repository commit |  | `git -C website rev-parse HEAD` |
| Website deploy URL |  | deploy preview or production URL |
| Transport stance |  | trusted LAN/Tailscale only, Tailscale-only, or `wss://`/message encryption |
| Android release stance |  | public, internal-only, or deferred |

## Local Preflight Evidence

| Gate | Command | Result | Evidence path/link |
| --- | --- | --- | --- |
| CLI formatting | `cd cli && cargo fmt --check` |  |  |
| CLI lint | `cd cli && cargo clippy --all-targets -- -D warnings -A dead-code` |  |  |
| CLI tests | `cd cli && cargo test` |  |  |
| Linux target check | `cd cli && cargo check --target x86_64-unknown-linux-gnu` |  |  |
| Windows target check | `cd cli && cargo check --target x86_64-pc-windows-gnu` |  |  |
| Mobile TypeScript | `cd mobile && npx tsc --noEmit` |  |  |
| Mobile security tests | `cd mobile && npm run test:security` |  |  |
| Mobile release preflight | `cd mobile && npm run preflight:release` |  |  |
| Website build | `cd website && npm run build` |  |  |
| Installer syntax | `bash -n install.sh && bash -n website/public/install.sh` |  |  |
| Installer parity | `diff -u install.sh website/public/install.sh` |  |  |
| Installer checksum tests | `bash scripts/test-installer-checksum.sh` |  |  |
| Workflow YAML parse | see runbook command |  |  |
| Whitespace diff check | root/mobile/website `git diff --check` |  |  |

## Mobile Build Evidence

| Platform | Required value | Recorded value | Result |
| --- | --- | --- | --- |
| iOS marketing version | matches app release |  |  |
| iOS build number | `112` or higher |  |  |
| iOS EAS build URL | production/TestFlight candidate |  |  |
| iOS App Store/TestFlight status | installable by testers |  |  |
| Android versionName | matches app release if shipped |  |  |
| Android versionCode | `92` or higher if shipped |  |  |
| Android EAS build URL | internal candidate or deferred |  |  |
| Android signing evidence | not debug-signed if shipped |  |  |
| Android production cleartext decision | verified or deferred |  |  |

## Desktop Release Artifact Evidence

| Archive | Present in release | SHA256SUMS entry | Install smoke | Result |
| --- | --- | --- | --- | --- |
| Linux x86_64 `.tar.gz` |  |  |  |  |
| Linux ARM64 `.tar.gz` |  |  |  |  |
| macOS Intel `.tar.gz` |  |  |  |  |
| macOS Apple Silicon `.tar.gz` |  |  |  |  |
| Windows x64 `.zip` |  |  |  |  |

Negative installer tests against draft release:

| Scenario | Expected | Result | Evidence |
| --- | --- | --- | --- |
| Corrupted archive | fails before extraction |  |  |
| Missing checksum entry | fails before extraction |  |  |
| Invalid checksum entry | fails before extraction |  |  |
| Wrong archive name | fails before extraction |  |  |

## Physical Device Smoke Evidence

| Host | Mobile | Network | Auth pair | Restart reconnect | Bad token | Revoked credential | Sessions/control | Filesystem | Push | Result |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| macOS Apple Silicon | iOS | LAN |  |  |  |  |  |  |  |  |
| macOS Apple Silicon | iOS | Tailscale |  |  |  |  |  |  |  |  |
| macOS Apple Silicon | Android | LAN/Tailscale or deferred |  |  |  |  |  |  |  |  |
| macOS Intel | iOS | LAN/Tailscale |  |  |  |  |  |  |  |  |
| Linux x86_64 | iOS | LAN/Tailscale |  |  |  |  |  |  |  |  |
| Linux ARM64 | iOS | LAN/Tailscale |  |  |  |  |  |  |  |  |
| Windows x64 | iOS | LAN/Tailscale |  |  |  |  |  |  |  |  |

## Security-Specific Evidence

| Check | Expected | Result | Evidence |
| --- | --- | --- | --- |
| Legacy `hello` first message | `auth_required`, no sensitive data |  |  |
| Invalid proof | `auth_invalid`, no client registration |  |  |
| Revoked credential | no reconnect, no PTY/filesystem/push data |  |  |
| PTY registration without local proof | rejected |  |  |
| Mobile spawn without working dir | uses approved root or fails |  |  |
| MobileCLI config path read | denied |  |  |
| OS secret paths | denied |  |  |
| Delete/rename default | denied |  |  |
| Delete/rename opt-in | allowed only in approved throwaway root |  |  |
| Push opt-out while offline | unregister drains after reconnect |  |  |
| AsyncStorage device metadata | no `authToken` or `auth_token` |  |  |

## Final Sign-Off

| Area | Owner | Status | Evidence |
| --- | --- | --- | --- |
| Desktop CLI release |  |  |  |
| Mobile iOS release |  |  |  |
| Mobile Android release/defer decision |  |  |  |
| Website/docs release |  |  |  |
| Transport stance approved |  |  |  |
| Security smoke complete |  |  |  |
| Rollback plan ready |  |  |  |

Release decision:

- [ ] Ship stable release
- [ ] Ship prerelease only
- [ ] Block release pending fixes

Decision notes:

-
