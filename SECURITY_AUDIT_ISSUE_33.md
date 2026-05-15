# Security Audit — Issue #33: WebSocket Authentication

> Operational note (2026-05-15): this file is retained as third-party audit evidence and claim verification. The remediation section later in this document proposed an older raw-token/hash design; that design is superseded by `SECURITY_REMEDIATION_PLAN_ISSUE_33.md` and `SECURITY_RELEASE_RUNBOOK_ISSUE_33.md`, which use auth-v2 challenge-response, credential scopes, filesystem hardening, and per-workspace release gates.

**Audited:** 2026-05-15
**Branch:** `main` (HEAD)
**Issue:** https://github.com/MobileCLI/mobilecli/issues/33
**Auditor:** Claude Code (claude-sonnet-4-6), multi-agent codebase analysis

---

## Executive Summary

GitHub issue #33 alleges 6 security findings against the MobileCLI daemon. After a full source code audit, the **code-level findings are largely accurate** for the current `main` branch. However, the framing partially misses context:

- The README's auth token claims are **aspirational documentation that was never implemented** — not a deliberate deception.
- The actual security model is **network-level isolation** (Tailscale VPN or LAN trust), which is intentional and documented in code comments, but not communicated to users anywhere they would read it.
- The daemon source explicitly states this intent in `daemon.rs:394`:
  > *"Security model: Access is controlled at the network level via: Local network: Only devices on same WiFi can connect / Tailscale: Only authenticated Tailscale network members can connect"*

This means: for Tailscale users, the security posture is reasonable (Tailscale membership = authentication). For local-network users on untrusted WiFi, it is not. The README is wrong either way.

---

## Finding-by-Finding Verdict

### Finding 1 — No Authentication (Critical) — **ACCURATE, missing context**

**Issue claim:** `handle_connection` accepts any WebSocket with no token check. `Config` has no `auth_token`. `ConnectionInfo.encryption_key` is always `None`. QR code contains no secret.

**Code confirms:**

- `daemon.rs:394`: `TcpListener::bind(format!("0.0.0.0:{}", port))` — binds all interfaces unconditionally
- `daemon.rs:497–535`: `handle_connection` calls `rx.next().await` as first action — no auth check before or after
- `setup.rs:22–30`: `Config` struct fields are `device_id`, `device_name`, `connection_mode`, `tailscale_ip`, `local_ip` — **no `auth_token` field**
- `protocol.rs:602–619`: `ConnectionInfo` has `encryption_key: Option<String>` that is **always `None`**, never set, never checked
- `protocol.rs:630–661`: QR encodes `mobilecli://host:port?device_id=UUID&device_name=HOSTNAME[&wss=1]` — **no secret of any kind**

**What the README says (false):**
- Line 86: *"generates a cryptographic auth token… The QR encodes a `ws://` URL with your token"*
- Line 129: *"Auth token required"* (architecture diagram)
- Line 199: *"Every WebSocket connection requires a cryptographic token generated during `mobilecli setup`. The token is stored in iOS Keychain and `~/.mobilecli/config.json`."*
- Line 200: *"Token stripping. Auth tokens are scrubbed from session output before streaming."*
- Line 322: *"config.json | Device identity, connection URL, **auth token hash**"*
- `docs/ARCHITECTURE_QUICK_REFERENCE.md:84`: *"Pairing Token (Optional): A per-device `auth_token` is generated during setup"*

**Actual `~/.mobilecli/config.json` on disk:**
```json
{
  "connection_mode": "tailscale",
  "device_id": "4fbff292-8275-473c-b958-144198219418",
  "device_name": "Sandman",
  "local_ip": null,
  "tailscale_ip": "100.120.208.57"
}
```
No token. Never generated.

**Verdict:** All README auth token claims are false. The security model is network isolation, not application-layer tokens. The daemon binds `0.0.0.0` and accepts all connections with zero application-layer checks.

---

### Finding 2 — Unauthenticated RCE (Critical) — **ACCURATE, minor mechanism nuance**

**Issue claim:** `is_shell_safe` (line 1139) only blocks `` ` ``, `$(`, `\n`, `\r`, `\0`. Attacker can spawn `bash` with `args: ["-c", "malicious"]`.

**Code confirms — `daemon.rs:1139–1145`:**
```rust
fn is_shell_safe(s: &str) -> bool {
    !s.contains('\n')
        && !s.contains('\r')
        && !s.contains('\0')
        && !s.contains('`')
        && !s.contains("$(")
}
```
Semicolons, pipes, redirects, `&`, `||`, `&&` are **not blocked**.

**Full allowed command list (`daemon.rs:1112–1128`):**
```
claude, codex, gemini, opencode, bash, zsh, sh, fish, nu, pwsh, powershell, python, python3, node, ruby
```

**Nuance on `-c` flag:** The issue says pass `args: ["-c", "malicious"]`. In practice, `shell_args_for_command` (`daemon.rs:1173–1188`) internally appends `-l -i -c <command>` for shell commands. However, **the command string passed to `-c` is not sanitized for semicolons/pipes**. An attacker passes `bash` with a command string containing `;` to chain arbitrary commands. The exact mechanism is slightly different from what the issue describes but the RCE risk is real and confirmed.

**Additional finding — `SendInput` (daemon.rs:2129–2160):**
- Any connected client can inject arbitrary keystrokes into **any session by session ID**
- **No session ownership check** exists
- No restriction on which session a client can target
- This includes injecting "yes" into AI tool approval prompts (Claude Code, Codex, Gemini)

**Verdict:** RCE risk is real. `is_shell_safe` is insufficient. `SendInput` has no ownership enforcement.

---

### Finding 3 — Unauthenticated File System Access (High) — **ACCURATE**

**Code confirms — `cli/src/filesystem/config.rs:33–50`:**

`allowed_roots` defaults to `home_dir()` — **full home directory access**.

Current `denied_patterns`:
```
**/.ssh/*        *.pem          *.key          **/id_rsa*
**/.gnupg/*      **/.aws/credentials
**/.env          **/.env.*      **/secrets.*   *.secret
**/token*        **/.npmrc      **/.pypirc
```

Supported operations: **list, read (50 MB), read_chunk, write (50 MB), create_directory, delete (recursive), rename, copy, get_file_info**

**Confirmed denylist gaps (not blocked):**
- `~/.kube/config` — Kubernetes cluster credentials
- `~/.docker/config.json` — Docker registry credentials
- `~/.config/gcloud/` — GCP credentials
- `~/.bash_history`, `~/.zsh_history` — shell command history
- `~/.psql_history`, `~/.python_history`, `~/.node_repl_history` — REPL histories
- `~/.config/google-chrome/Default/Login Data` — Chrome saved passwords
- `~/.mozilla/firefox/**/*.sqlite` — Firefox credentials
- `~/.git-credentials`, `~/.netrc` — git/HTTP credentials
- `~/.vault-token` — HashiCorp Vault token
- `~/*.tfstate` — Terraform state (contains secrets)
- `~/.aws/**` (only `credentials` is blocked, not the whole directory)

**Verdict:** Issue is accurate. Delete is enabled, write is enabled. Denylist approach has significant gaps.

---

### Finding 4 — Listens on All Interfaces (High) — **ACCURATE, INTENTIONAL**

`daemon.rs:394`: `TcpListener::bind(format!("0.0.0.0:{}", port))` — no conditions, always all interfaces.

The code comment explicitly justifies this with the network-isolation security model. The concern about public WiFi/shared office networks is valid for users in local mode who did not choose Tailscale.

**Verdict:** Accurate and intentional. Risk depends on user's network context.

---

### Finding 5 — Push Notification Hijacking (Medium) — **ACCURATE**

**Code confirms:**
- `daemon.rs:2646–2670`: `RegisterPushToken` — any connected client can register, no verification, no limit
- `daemon.rs:4500–4549`: Sends to `https://exp.host/--/api/v2/push/send` (Expo Push API)
- Push payload includes: session ID, title, body, "waiting_for_input" type

**README line 50:** *"No cloud. No accounts. No relay servers. Just a direct WebSocket between your machine and your phone."*

This is **false** — Expo push uses an external cloud relay service.

**Verdict:** Any client can register push tokens. "No cloud" claim is false for users with push notifications. The prompt content risk is slightly overstated — push body says "waiting for input" with session ID, not the actual prompt text.

---

### Finding 6 — No Binary Integrity Verification (Medium) — **ACCURATE**

- `install.sh:94`: Downloads binary from GitHub releases
- No SHA-256 verification anywhere in the script
- `install.sh:117–119`: Escalates to `sudo` with only a printed warning, no explicit confirmation prompt

**Verdict:** Accurate. No checksum verification exists.

---

## Summary Table

| Finding | Verdict | Notes |
|---------|---------|-------|
| No auth token in code | TRUE | Never implemented |
| README auth token claims | TRUE — README is outdated/false | Aspirational docs, never built |
| RCE via SpawnSession + `is_shell_safe` gaps | TRUE | Semicolons/pipes not blocked |
| `-c` flag injection specifically | PARTIALLY TRUE | `-c` is added internally, but semicolons in command string still allow injection |
| `SendInput` injects into any session | TRUE | No ownership check |
| Filesystem: home dir default | TRUE | `allowed_roots = [home_dir()]` |
| Filesystem: denylist gaps | TRUE | Many sensitive paths uncovered |
| `0.0.0.0` binding | TRUE, INTENTIONAL | Necessary for mobile; risk depends on network |
| Push token registration unvalidated | TRUE | Any client registers |
| "No cloud" contradicted by Expo push | TRUE | Expo uses `exp.host` external service |
| `install.sh` no checksums | TRUE | Confirmed |
| `install.sh` silent sudo | PARTIALLY TRUE | Prints warning, no "proceed?" prompt |

---

---

# Remediation Plan

## Fix 1 — Implement Application-Layer Auth Token

**Goal:** Build what the README claims. Generate a real token during setup, embed it in the QR code, verify it on every WebSocket connection.

### CLI changes

**`cli/src/setup.rs`**
- Add `auth_token_hash: String` field to `Config` struct
- During setup wizard: generate 32 cryptographically random bytes (`rand` or `getrandom` crate), hex-encode as raw token
- SHA-256 hash the raw token, store hex-encoded hash in `config.json` as `auth_token_hash`

**`cli/src/protocol.rs`**
- Update `ConnectionInfo.to_compact_qr()` to append `&token={raw_token}` to QR URL
- Add `token: Option<String>` field to `ConnectionInfo`
- Add `ClientMessage::Auth { token: String }` variant to client message enum

**`cli/src/daemon.rs`**
- Load `config.json` at startup, read `auth_token_hash`
- In `handle_connection` (~line 497): expect first client message to be `ClientMessage::Auth { token }`. Compute `sha256hex(token)`, compare to stored hash. Close connection if mismatch.
- Add ~1 second constant-time delay on auth failure to resist timing attacks

**`cli/Cargo.toml`**
- Add `sha2 = "0.10"` (or `ring`)
- Confirm `rand` or `getrandom` present

### Mobile app changes (`mobile/`)

- Parse `token` query param from deep-link/QR URL
- Store raw token in iOS Keychain (`kSecAttrService = "mobilecli"`) and Android Keystore
- On WebSocket connect, send `{ "type": "auth", "token": "<raw_token>" }` as **first message** before any other message

---

## Fix 2 — Session Ownership for SendInput

**Goal:** Only the client that spawned a session can inject input into it.

**`cli/src/daemon.rs`**
- Generate `connection_id: Uuid` per WebSocket connection at start of `handle_connection`
- Add `owner_connection_id: Uuid` to the session struct
- In `SpawnSession` handler: set `session.owner_connection_id = connection_id`
- In `SendInput` handler (~line 2129): verify `connection_id == session.owner_connection_id`, reject if mismatch
- Apply same ownership check to `ResizeSession` and `KillSession`

---

## Fix 3 — Strengthen `is_shell_safe`

**`cli/src/daemon.rs` (line 1139)**

Replace current implementation with:
```rust
fn is_shell_safe(s: &str) -> bool {
    !s.contains('\n')
        && !s.contains('\r')
        && !s.contains('\0')
        && !s.contains('`')
        && !s.contains("$(")
        && !s.contains(';')    // command chaining
        && !s.contains("||")   // OR chaining
        && !s.contains("&&")   // AND chaining
        && !s.contains('>')    // stdout redirect
        && !s.contains('<')    // stdin redirect
        && !s.contains('&')    // background / AND
        && !s.contains('|')    // pipe
}
```

Also audit every call site to confirm `is_shell_safe` is applied to both the command string **and** each arg in `SpawnSession`. Apply it to args if not already done.

---

## Fix 4 — Tighten Filesystem Denylist

**`cli/src/filesystem/config.rs` (lines 36–50)**

Append to `denied_patterns`:
```rust
// Kubernetes
"**/.kube/**",
// Docker
"**/.docker/config.json",
"**/.docker/contexts/**",
// Google Cloud
"**/.config/gcloud/**",
// Shell histories
"**/.bash_history",
"**/.zsh_history",
"**/.sh_history",
"**/.fish/fish_history",
"**/.local/share/fish/fish_history",
// REPL histories
"**/.psql_history",
"**/.python_history",
"**/.node_repl_history",
"**/.irb_history",
// Browser credentials (Linux)
"**/.config/google-chrome/Default/Login Data",
"**/.config/chromium/Default/Login Data",
"**/.mozilla/firefox/**/*.sqlite",
// macOS Keychain
"**/Library/Keychains/**",
// Git credentials
"**/.git-credentials",
"**/.config/git/credentials",
// netrc
"**/.netrc",
// Vault / 1Password
"**/.vault-token",
"**/.config/op/**",
// yarn tokens
"**/.yarnrc",
"**/.config/yarn/**",
// Terraform state (contains secrets)
"**/*.tfstate",
"**/*.tfstate.backup",
// Broader AWS (not just credentials file)
"**/.aws/**",
// age encryption keys
"**/*.age",
```

---

## Fix 5 — Add Checksum Verification to install.sh

**`install.sh` (after the download block, ~line 94)**

```bash
# Download checksum file published alongside the release binary
checksum_url="${download_url}.sha256"
download_file "${checksum_url}" "${tmp_dir}/${archive_name}.sha256"

# Verify integrity
if command -v sha256sum >/dev/null 2>&1; then
    (cd "${tmp_dir}" && sha256sum --check "${archive_name}.sha256") \
        || die "SHA-256 checksum verification failed — binary may be corrupt or tampered with"
elif command -v shasum >/dev/null 2>&1; then
    (cd "${tmp_dir}" && shasum -a 256 -c "${archive_name}.sha256") \
        || die "SHA-256 checksum verification failed — binary may be corrupt or tampered with"
else
    warn "Neither sha256sum nor shasum found — skipping checksum verification (not recommended)"
fi
```

**Also update the GitHub Actions release workflow** (`.github/workflows/release.yml` or equivalent) to generate and upload `{archive}.sha256` files alongside each release binary.

---

## Fix 6 — Update README and Docs

**`README.md`** — remove/replace these specific false claims:

| Location | Remove | Replace with |
|----------|--------|--------------|
| Line 86 | "generates a cryptographic auth token… QR encodes a `ws://` URL with your token" | Accurate description after Fix 1 is live |
| Line 129 | "Auth token required" in architecture diagram | "Auth token required (256-bit, verified on connect)" |
| Line 199 | Entire auth token paragraph | Accurate description of the token system after Fix 1 |
| Line 200 | "Token stripping. Auth tokens are scrubbed from session output…" | Remove entirely |
| Line 322 | "auth token hash" in config.json table | Remove or update |

**Add a "Security Model" section** explaining:
- **Token auth:** 256-bit token generated during `mobilecli setup`. Raw token embedded in QR. Hash stored in `config.json`. Required for every WebSocket connection.
- **Local mode:** Token auth + LAN access. Suitable for trusted home/office networks.
- **Tailscale mode:** Token auth + Tailscale membership. Recommended for remote/public access.
- **Push notifications:** The only outbound network call is Expo push (`exp.host`) for "waiting for input" alerts. No terminal output leaves your machine.

**`docs/ARCHITECTURE_QUICK_REFERENCE.md:84`** — replace false "Pairing Token (Optional)" claim with accurate post-Fix 1 description.

---

## Fix 7 — Push Token Rate Limiting

**`cli/src/daemon.rs` (~line 2646)**

- After Fix 1 lands, push token registration is already gated behind auth
- Add a cap: maximum 3 push tokens per authenticated `device_id`
- Scope push tokens to `device_id` so re-registration replaces rather than accumulates

---

## Implementation Order

| Priority | Fix | Scope | Notes |
|----------|-----|-------|-------|
| 1 | Fix 3 — `is_shell_safe` | Small, 1 function | Highest bang-for-buck, isolated change |
| 2 | Fix 4 — filesystem denylist | Small, 1 file | Low risk, append-only |
| 3 | Fix 6 — README/docs | No code changes | Fix the false claims immediately |
| 4 | Fix 5 — install.sh checksums | install.sh + release workflow | Self-contained |
| 5 | Fix 2 — session ownership | daemon.rs only | Moderate scope, no protocol change |
| 6 | Fix 1 — auth token system | daemon + protocol + mobile app | Largest; breaks existing clients; do last |
| 7 | Fix 7 — push rate limit | After Fix 1 | Trivial once Fix 1 is done |

---

## Critical Files

| File | Relevant Fixes |
|------|---------------|
| `cli/src/daemon.rs` | Fix 1, 2, 3, 7 — auth, ownership, is_shell_safe, push limit |
| `cli/src/setup.rs` | Fix 1 — token generation, Config struct |
| `cli/src/protocol.rs` | Fix 1 — QR encoding, ClientMessage::Auth |
| `cli/src/filesystem/config.rs` | Fix 4 — denylist extension |
| `cli/Cargo.toml` | Fix 1 — sha2/rand deps |
| `install.sh` | Fix 5 — checksum verification |
| `.github/workflows/release.yml` | Fix 5 — publish .sha256 files |
| `README.md` | Fix 6 — remove false claims, add Security Model |
| `docs/ARCHITECTURE_QUICK_REFERENCE.md` | Fix 6 — remove false claim line 84 |
| `mobile/` (WebSocket connect code) | Fix 1 — send Auth message, store token |

---

## Verification Checklist

- [ ] **Fix 1:** `websocat ws://localhost:9847` closes immediately with error. Client with correct token gets `Welcome`.
- [ ] **Fix 2:** Client A spawns session. Client B sends `SendInput` to that session ID → rejected.
- [ ] **Fix 3:** `SpawnSession { command: "bash", args: ["; echo pwned"] }` → rejected by `is_shell_safe`.
- [ ] **Fix 4:** Read `~/.kube/config` and `~/.bash_history` via file bridge → denied.
- [ ] **Fix 5:** Corrupt downloaded binary, run `install.sh` → fails with checksum error message.
- [ ] **Fix 6:** Full README read — no false auth token claims remain.
- [ ] **End-to-end:** Setup → QR scan → mobile connects with token → sessions work normally.
