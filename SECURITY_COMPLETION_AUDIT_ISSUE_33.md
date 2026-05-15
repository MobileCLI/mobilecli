# Security Completion Audit - Issue #33

Date: 2026-05-15
Status: not complete for release; local implementation and local verification are complete for the current checkout

## Objective Restated

The user asked for a thorough issue #33 remediation plan and implementation path that covers:

- the truth of the GitHub issue #33 claims and Claude audit notes
- a completed remediation plan document
- all desktop daemon/CLI/security components
- all mobile app/security components
- docs, website, installer, and release workflows
- Linux, macOS, Windows, iOS, and Android compatibility
- use of subagents for speed/depth
- no premature coding-only conclusion before release/device gates are known

## Prompt-To-Artifact Checklist

| Requirement | Artifact or evidence | Current state |
| --- | --- | --- |
| Audit issue #33 claims | `SECURITY_AUDIT_ISSUE_33.md` plus local code review | Historical audit retained; operational note says raw-token remediation is superseded by auth-v2 plan |
| Complete remediation plan | `SECURITY_REMEDIATION_PLAN_ISSUE_33.md` | Updated with implemented local state, remaining release gates, transport limitations, and verification status |
| Executable release/device plan | `SECURITY_RELEASE_RUNBOOK_ISSUE_33.md` | Added host/mobile/network matrix, EAS/TestFlight/internal build order, evidence tables, and final sign-off table |
| Release evidence capture | `SECURITY_RELEASE_EVIDENCE_TEMPLATE_ISSUE_33.md` | Added a manifest/evidence template for commits, builds, artifacts, platform smoke, security smoke, and release sign-off |
| Desktop auth-v2 | CLI protocol/auth/daemon changes | Implemented locally with challenge-response, credential scopes, revocation, timeout/size cap, and unit coverage |
| Local PTY registration hardening | CLI daemon/wrapper/auth changes | Implemented local PTY proof so loopback clients cannot register unauthenticated PTY sessions; daemon-side validation has focused unit coverage |
| Desktop filesystem hardening | CLI filesystem/daemon/setup changes | Implemented project roots, denied secrets, destructive opt-in, safe empty roots, Windows-style pattern normalization, copy/search clamps |
| Desktop spawn hardening | CLI daemon changes | Mobile spawn uses supported profiles, rejects args/paths/interpreter flags, and defaults to approved roots instead of home |
| Desktop config secret handling | CLI setup/platform changes | Unix private writes, Windows ACL failures surfaced, no project-local config fallback when home is missing |
| Mobile auth-v2 | Mobile QR/auth/sync changes | Implemented QR auth fields, proof generation, pre-auth message gating, auth-state tests |
| Mobile credential storage | Mobile devices/storage changes | Pairing tokens are SecureStore-only per device; AsyncStorage stores metadata only; old inline tokens migrate/scrub |
| Mobile push behavior | Mobile sync/push changes | Push registration waits for auth; opt-out unregister state persists across app termination |
| Mobile native build need | Plan/runbook/checklist | New build required; current configured floor is iOS build `112`, Android versionCode `92` |
| Android release signing | Android Gradle/preflight | Release no longer signs with debug config; preflight fails if that pattern returns |
| Installer integrity | Root/website installer and release workflow | Installer verifies GitHub Release checksums; root and website scripts are byte-for-byte identical locally |
| Root release workflow | `.github/workflows/release.yml` | Adds CLI gates, tag/version sync, native release-runner tests, archive completeness, checksum completeness |
| Per-workspace CI | root/mobile/website workflows | Added/updated local workflow files; note mobile and website are separate nested workspaces and must be committed/released separately |
| Public docs accuracy | README/docs/website/LLM artifacts | Updated auth-v2/manual pairing/Tailscale/Expo/filesystem/install claims; stale command scan has no unsupported flag hits except intentional warnings |
| Subagent depth | Subagent results in session | Desktop, mobile, and docs/release subagents found residual blockers; local patch set addresses all actionable code findings except transport confidentiality |
| Linux compatibility | Local target check and CI matrix | `cargo check --target x86_64-unknown-linux-gnu` passed locally; release/manual Linux ARM64 still external |
| Windows compatibility | Local target check and docs/workflow | `cargo check --target x86_64-pc-windows-gnu` passed locally; real Windows smoke and ACL inspection still external |
| macOS compatibility | Workflow/runbook | Planned in CI/release and runbook; no local macOS runner evidence in this sandbox |
| iOS compatibility | Mobile build metadata/runbook | Build numbers updated and TypeScript/preflight pass; real TestFlight/device smoke still external |
| Android compatibility | Mobile build metadata/runbook | versionCode updated and preflight passes; production cleartext and signing evidence still external |

## Fresh Local Verification Evidence

Commands passed after the latest edits:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings -A dead-code`
- `cargo test` (`74 passed`)
- `cargo check --target x86_64-unknown-linux-gnu`
- `cargo check --target x86_64-pc-windows-gnu`
- `npx tsc --noEmit` in `mobile/`
- `npm run test:security` in `mobile/`
- `npm run preflight:release` in `mobile/` with expected warnings for unavailable EAS env access and dirty worktree
- `npm run build` in `website/`
- `bash -n install.sh`
- `bash -n website/public/install.sh`
- `diff -u install.sh website/public/install.sh`
- `bash scripts/test-installer-checksum.sh`
- workflow YAML parse across root, mobile, and website workflows
- `git diff --check`, `git -C mobile diff --check`, `git -C website diff --check`

## Not Yet Achieved

The overall release objective is not complete until these are done outside this sandbox:

- iOS physical-device smoke on LAN and Tailscale
- Android physical-device smoke if Android is in release scope
- EAS/TestFlight/internal build evidence for the new mobile app build
- production Android cleartext decision and smoke evidence
- macOS Apple Silicon and Intel release-runner evidence
- Linux x86_64 and ARM64 release artifact smoke
- Windows x64 release artifact smoke and config ACL inspection
- actual GitHub Release `SHA256SUMS.txt` verification
- installer corrupt/missing checksum negative tests against draft release artifacts
- final transport security stance: trusted LAN/Tailscale only, or add `wss://`/message encryption before claiming MITM resistance
- final sign-off table in `SECURITY_RELEASE_RUNBOOK_ISSUE_33.md`

## Completion Decision

Do not mark the active goal complete yet. The local implementation and local verification are substantially complete, but the stated objective includes all devices and cross-platform compatibility. Those requirements still need physical-device, EAS/store, release-artifact, and real platform evidence.
