# MobileCLI Code Review -- Consolidated Report

Generated: 2026-02-20
Source: 22 review files from `tooling/reviews/` (Strand-Rust-Coder-14B, RunPod)

---

## Summary

| Severity | Count |
|----------|-------|
| CRITICAL | 1     |
| HIGH     | 11    |
| MEDIUM   | 14    |
| LOW      | 7     |
| **Total (deduplicated)** | **33** |
| False Positives / Hallucinations | 10 |

---

## CRITICAL (1)

### C-1: Pairing token mismatch does not reject connections

**Files:** `cli/src/daemon.rs` (lines 340-376)
**Source reviews:** p1-daemon (Chunk 3 Finding 6, Chunk 5 Finding 1)

**Description:** The daemon has a pairing token mechanism, but when a mobile client provides a *mismatched* token or *no token at all*, the connection is still allowed with only a log warning. This means the pairing token provides zero actual security -- any client on the network can connect regardless of token validity.

**Suggested fix:** Reject connections when `expected_token` is set and the provided token is missing or mismatched. Return a WebSocket close frame with an authentication error. Only allow unauthenticated connections when no pairing token is configured.

**Note:** The p1-daemon review described this as "unauthenticated session spawning." The actual situation is more nuanced: authentication infrastructure exists but is not enforced. This is the single most impactful issue in the codebase.

---

## HIGH (11)

### H-1: Daemon binds to 0.0.0.0 (all interfaces)

**File:** `cli/src/daemon.rs` (line 219)
**Source reviews:** p1-daemon (Chunk 1 Finding 1)

**Description:** The WebSocket server binds to `0.0.0.0`, exposing it to all network interfaces. The code has a comment explaining this is intentional for mobile client access, with security delegated to network-level controls (local WiFi or Tailscale). However, combined with C-1 (token not enforced), any device on the same network can connect and execute commands.

**Suggested fix:** If the pairing token enforcement (C-1) is fixed, this becomes acceptable as designed. Otherwise, consider binding to `127.0.0.1` and requiring Tailscale/SSH tunneling for remote access.

### H-2: AppleScript injection in iTerm2/Terminal.app session spawning

**File:** `cli/src/daemon.rs` (lines 1059-1085)
**Source reviews:** p1-daemon (Chunk 3 Finding 3)

**Description:** When spawning sessions via iTerm2 or Terminal.app, the shell command is interpolated into an AppleScript string. The escaping (`replace('\\', "\\\\").replace('"', "\\\""`) is minimal. While `shell_command_line()` uses `shell_quote_posix()` for POSIX quoting, the AppleScript layer itself has different escaping requirements. A crafted command could break out of the AppleScript string context.

**Suggested fix:** Use AppleScript's own quoting mechanism or pass commands via a temp file/environment variable instead of string interpolation. Additionally, the `is_shell_safe()` check runs before this point, which mitigates the most dangerous vectors (backticks, `$()`), but does not cover all AppleScript-specific injection characters.

### H-3: Symlink check ordering in security.rs validate_existing

**File:** `cli/src/filesystem/security.rs`
**Source reviews:** p1-security (Finding 1)

**Description:** The `validate_existing` method originally canonicalized the path before checking for symlinks when `follow_symlinks = false`. This could miss intermediate symlinks pointing to disallowed locations.

**Suggested fix:** Check for symlinks on the original (non-canonical) path first, before canonicalization. The review indicates this was already fixed (reordered). Verify the current code has the correct ordering.

### H-4: Path traversal in UploadFile handler

**File:** `cli/src/daemon.rs` (upload handling, ~line 1632)
**Source reviews:** p1-daemon (Chunk 4 Finding 1)

**Description:** The upload endpoint processes file names from mobile clients. While `sanitize_upload_file_name()` exists and handles many cases (invalid chars, Windows reserved names, length truncation), the review flagged that session IDs or relative paths could be used to write outside the project directory.

**Suggested fix:** Verify that the `build_upload_destination_path()` function produces paths that are always validated against the filesystem's `PathValidator` before writing. The `sanitize_upload_file_name` function itself looks solid with tests, but the path composition step should be audited for traversal.

### H-5: Unbounded channels for PTY input/resize

**File:** `cli/src/daemon.rs` (lines 573-574), `cli/src/pty_wrapper.rs` (lines 257, 289), `cli/src/link.rs` (line 218)
**Source reviews:** p2-daemon (Chunk 2 Finding 1)

**Description:** Multiple `mpsc::unbounded_channel` calls are used for PTY input, resize messages, and output. A malicious or malfunctioning client could flood these channels and exhaust memory since there is no backpressure.

**Suggested fix:** Switch to bounded channels (e.g., capacity 1024-4096) and handle the `SendError` when the channel is full (drop messages or return backpressure to the client).

### H-6: No limit on concurrent connections/sessions

**File:** `cli/src/daemon.rs`
**Source reviews:** p2-daemon (Chunk 1 Finding 1, Chunk 3 Finding 1)

**Description:** The daemon spawns a new task for every incoming connection and a new process for every session request, with no upper bound. This allows resource exhaustion via connection flooding.

**Suggested fix:** Introduce a `tokio::sync::Semaphore` to limit concurrent connections (e.g., 20) and a separate limit on active sessions (e.g., 10). Reject new connections/spawns when limits are reached.

### H-7: Panic on malformed JSON in resize handling (pty_wrapper.rs)

**File:** `cli/src/pty_wrapper.rs` (resize branch of main event loop)
**Source reviews:** p3-pty_wrapper (Finding 3)

**Description:** The resize handler directly casts `msg["cols"]` and `msg["rows"]` as u64 without checking for existence. If the daemon sends malformed JSON, this causes a panic.

**Suggested fix:** Add explicit existence checks before casting, defaulting to safe values (e.g., 80x24) or logging an error.

### H-8: Panic on malformed base64 input (pty_wrapper.rs)

**File:** `cli/src/pty_wrapper.rs`
**Source reviews:** p3-pty_wrapper (Finding 6)

**Description:** Base64 decoding of PTY data does not handle malformed input gracefully. Corrupted data from the daemon causes a panic.

**Suggested fix:** Validate or use `.ok()` / `match` on base64 decode results, logging errors instead of panicking.

### H-9: load_config uses .unwrap() on JSON parsing (setup.rs)

**File:** `cli/src/setup.rs`
**Source reviews:** p6-setup (Chunk 1 Finding 1)

**Description:** `load_config` panics if the config JSON is malformed or corrupted, which can happen with hand-edited config files or disk corruption.

**Suggested fix:** Replace `.unwrap()` with `.ok()` or proper `Result` propagation, returning a default config on parse failure.

### H-10: Concurrent PTY reader/writer access without synchronization (pty_wrapper.rs)

**File:** `cli/src/pty_wrapper.rs`
**Source reviews:** p6-pty_wrapper (Finding 5)

**Description:** Multiple async tasks concurrently read from and write to the PTY master without explicit locking, potentially causing race conditions or corrupted output.

**Suggested fix:** Introduce a `Mutex` around shared PTY handles or verify that the portable-pty crate's handles are thread-safe. If they are, this is a false positive (see False Positives section).

### H-11: Windows FFI in platform.rs lacks error handling

**File:** `cli/src/platform.rs`
**Source reviews:** p6-platform (Findings 3, 4)

**Description:** The Windows implementation of `default_shell()` and `is_process_alive()` uses unsafe FFI calls. If `OpenProcess` or `GetExitCodeProcess` fail, they may return null handles leading to undefined behavior.

**Suggested fix:** Validate all handle returns from Windows API calls and propagate errors instead of assuming success.

---

## MEDIUM (14)

### M-1: TOCTOU race in directory deletion (operations.rs)

**File:** `cli/src/filesystem/operations.rs`
**Source reviews:** p1-operations (Chunk 1 Finding 2)

**Description:** Between checking for children and deleting a directory, another process can create new entries, leading to partial deletions or permission errors.

**Suggested fix:** Use atomic operations or add a retry loop with backoff that rechecks state before proceeding.

### M-2: rename_path does not validate destination writability (operations.rs)

**File:** `cli/src/filesystem/operations.rs` (line ~535)
**Source reviews:** p1-operations (Chunk 2 Finding 1)

**Verification status:** The actual code at line 535 shows `if !self.validator.is_writable(&old_path) || !self.validator.is_writable(&new_path)` -- this check ALREADY EXISTS. **This finding appears to be a false positive** (see False Positives section).

### M-3: Unsanitized tmux session names

**File:** `cli/src/daemon.rs` (headless tmux spawning)
**Source reviews:** p1-daemon (Chunk 3 Findings 2, 7)

**Description:** When spawning tmux sessions, the session name comes from user input. While `is_shell_safe()` blocks newlines, nulls, backticks, and `$(`, tmux session names with semicolons or other special characters could cause issues.

**Suggested fix:** Restrict tmux session names to `[a-zA-Z0-9_-]` before passing to tmux. The existing `is_shell_safe()` mitigates the worst cases but is not tmux-specific.

### M-4: Race condition in WatchDirectory concurrent subscription

**File:** `cli/src/daemon.rs` (~line 1946)
**Source reviews:** p1-daemon (Chunk 4 Finding 3)

**Description:** Multiple concurrent connections starting watches on the same directory may cause duplicate events or missed watcher registrations due to unsynchronized count updates.

**Suggested fix:** Use atomic operations for count updates and acquire a lock around subscription management.

### M-5: Missing rate limiting for GetHomeDirectory

**File:** `cli/src/daemon.rs`
**Source reviews:** p1-daemon (Chunk 4 Finding 6), p2-daemon (Chunk 4 Finding 1)

**Description:** The `GetHomeDirectory` handler does not participate in the global rate limiter, unlike other filesystem operations.

**Suggested fix:** Add `check_fs_rate_limit(state, addr).await?` before handling the request.

### M-6: normalize_input_newlines may corrupt multi-byte UTF-8 (pty_wrapper.rs)

**File:** `cli/src/pty_wrapper.rs`
**Source reviews:** p3-pty_wrapper (Finding 2)

**Description:** The function replaces `\n` and `\r\n` with `\r` but may not handle multi-byte UTF-8 sequences safely if operating at the byte level.

**Suggested fix:** Ensure the function operates on `str` (validated UTF-8) rather than raw bytes when doing newline replacement.

### M-7: Shell hook installer does not check for existing markers

**File:** `cli/src/shell_hook.rs`
**Source reviews:** p4-shell_hook (Chunk 1 Finding 1)

**Description:** The installer writes to user config files (.bashrc, .zshrc, etc.) without checking for existing sentinel comments, potentially duplicating injected snippets.

**Suggested fix:** Detect existing sentinel markers before writing and skip or update rather than append.

### M-8: Silent error swallowing in shell hook install_quiet/uninstall_quiet

**File:** `cli/src/shell_hook.rs`
**Source reviews:** p4-shell_hook (Chunk 2 Finding 1)

**Description:** The quiet variants of install/uninstall silently ignore errors from individual hook operations, making failures invisible during the setup wizard.

**Suggested fix:** Propagate errors so callers know when operations fail.

### M-9: No request_id validation in handle_client_message

**File:** `cli/src/daemon.rs`
**Source reviews:** p2-daemon (Chunk 5 Finding 1)

**Description:** Client request IDs are not validated for format or length. Extremely long or malformed IDs could cause excessive logging or memory usage.

**Suggested fix:** Validate that request IDs are non-empty, under a reasonable length limit (e.g., 128 bytes), and contain only allowed characters.

### M-10: setup.rs Tailscale error propagation lacks context

**File:** `cli/src/setup.rs`
**Source reviews:** p6-setup (Chunk 1 Finding 2)

**Description:** Errors from `start_tailscale()` propagate directly without contextual messages, causing confusing error output during setup.

**Suggested fix:** Add `.context("Failed to start Tailscale")` or equivalent error wrapping.

### M-11: setup.rs no recovery for mid-setup network loss

**File:** `cli/src/setup.rs`
**Source reviews:** p6-setup (Chunk 1 Finding 3)

**Description:** If network connectivity is lost between configuration and pairing, the device can be left in an inconsistent state without recovery guidance.

**Suggested fix:** Add retry logic for network operations and provide clear recovery instructions.

### M-12: Silent I/O error swallowing in pty_wrapper.rs

**File:** `cli/src/pty_wrapper.rs`
**Source reviews:** p6-pty_wrapper (Finding 2)

**Description:** Both terminal writes and WebSocket sends use `_ = ...` which discards I/O errors entirely, causing silent data loss.

**Suggested fix:** Replace discards with `tracing::debug!` logging so failures are observable.

### M-13: No graceful degradation for daemon unavailability

**File:** `cli/src/pty_wrapper.rs`
**Source reviews:** p6-pty_wrapper (Finding 3)

**Description:** When the daemon is unreachable, `connect_async` fails and the CLI exits with a generic error. No retry or user guidance is provided.

**Suggested fix:** Implement retry with exponential backoff and provide actionable diagnostic messages.

### M-14: No explicit resource cleanup on error paths (pty_wrapper.rs)

**File:** `cli/src/pty_wrapper.rs`
**Source reviews:** p6-pty_wrapper (Finding 6)

**Description:** WebSocket connections and file descriptors may leak if an early panic occurs, since cleanup only runs on the normal exit path.

**Suggested fix:** Use RAII / `Drop` implementations or `defer`-style cleanup patterns to ensure resources are released on all paths.

---

## LOW (7)

### L-1: TOCTOU race in directory watch validation

**File:** `cli/src/daemon.rs`
**Source reviews:** p1-daemon (Chunk 4 Finding 4)

**Description:** Between validating a path exists as a directory and starting the watch, the target could be deleted. Not a security issue but causes confusing watcher behavior.

### L-2: config_dir fallback to "." when $HOME is unset

**File:** `cli/src/platform.rs`
**Source reviews:** p4-platform (Finding 1), p6-platform (Finding 1)

**Description:** When `$HOME` cannot be determined, config files are written to the current working directory, which may be unexpected.

**Suggested fix:** Return `None` and force callers to handle the fallback explicitly.

### L-3: default_shell() silent fallback without logging

**File:** `cli/src/platform.rs`
**Source reviews:** p6-platform (Finding 2)

**Description:** When `$SHELL` is unset on Unix, the function falls back to `/bin/sh` without logging a warning, making debugging harder.

### L-4: is_process_alive() silent error on Unix

**File:** `cli/src/platform.rs`
**Source reviews:** p6-platform (Finding 5)

**Description:** Errors from `kill()` are silently swallowed, potentially leading to incorrect process status reporting.

### L-5: detect_wait_event processes unbounded input length

**File:** `cli/src/detection.rs`
**Source reviews:** p4-detection (Finding 1)

**Description:** The function processes the entire input string without truncation, causing performance issues with very large terminal outputs.

**Suggested fix:** Introduce a length limit (e.g., 10,000 characters) for the detection input.

### L-6: Unnecessary string cloning in search.rs walker

**File:** `cli/src/filesystem/search.rs`
**Source reviews:** p5-search (Finding 1)

**Description:** `content_pattern` is cloned for every directory entry in the parallel walker, causing unnecessary heap allocations.

**Suggested fix:** Clone once per thread or use an `Arc<str>`.

### L-7: Unnecessary string cloning in watcher.rs

**File:** `cli/src/filesystem/watcher.rs`
**Source reviews:** p5-watcher (Finding 1)

**Description:** Path strings are cloned multiple times per watch operation.

**Suggested fix:** Use a single owned `String` and avoid redundant `.clone()` calls.

---

## False Positives and Hallucinations (10)

These findings from the review model are incorrect, refer to non-existent code, or describe issues that are already handled:

### FP-1: "Missing authentication checks -- all file operations bypass session validation" (p1-operations Chunk 1 Finding 4)

**Reason:** This is a design-level architectural observation, not a code bug. The filesystem module does not handle authentication -- that is the daemon's responsibility. The operations module is called *after* the daemon has processed the client message. Suggesting "per-operation auth wrappers" in operations.rs conflates layers.

### FP-2: "Sensitive file exposure via read_file -- .env, .ssh/* not blocked" (p1-operations Chunk 1 Finding 3)

**Reason:** The code already has extensive `denied_patterns` in `config.rs` (lines 36-49) that block `.ssh/*`, `.env`, `.env.*`, `*.pem`, `*.key`, `id_rsa*`, `.gnupg/*`, `.aws/credentials`, `secrets.*`, `*.secret`, `token*`, `.npmrc`, `.pypirc`. The `is_denied()` check is called in `list_directory` (line 78) and throughout operations. This finding is factually wrong.

### FP-3: "rename_path does not validate destination writability" (p1-operations Chunk 2 Finding 1)

**Reason:** The actual code at `operations.rs` line 535 already contains `if !self.validator.is_writable(&old_path) || !self.validator.is_writable(&new_path)`. The finding describes a bug that was already fixed or never existed.

### FP-4: "copy_dir_recursive does not verify writability of intermediate parent directories" (p1-operations Chunk 2 Finding 2)

**Reason:** The actual code at `operations.rs` lines 808-814 shows explicit `is_denied()` and `is_writable()` checks on `current_dst` during recursive copy. The finding describes missing checks that actually exist.

### FP-5: "get_file_info reads 8KB without permission check" (p1-operations Chunk 2 Finding 3)

**Reason:** `get_file_info` calls `validate_existing` at the top (line 606), which validates the path is within allowed roots and not denied. The `is_readable` check is implicitly performed by the validator. The finding mischaracterizes the security model.

### FP-6: session.rs findings 3-8 are duplicates of findings 1-2 (p6-session)

**Reason:** The review model output for `session.rs` repeated the same two findings (`load_sessions` unwrap and `save_sessions` expect) four times each, producing 8 findings for what is actually 2. Furthermore, examining the actual `session.rs` code, `load_sessions()` already uses `.ok().and_then().unwrap_or_default()` (NOT bare `.unwrap()`), and `save_sessions()` uses `?` operator (NOT `.expect()`). **Both issues described are already handled in the actual code.** The review model hallucinated the presence of `.unwrap()` and `.expect()` calls.

### FP-7: "sanitize_upload_file_name does not enforce maximum length" (p1-daemon Chunk 6 Finding 1)

**Reason:** The actual code has `MAX_UPLOAD_FILE_NAME_BYTES` and explicitly truncates with `truncate_file_name_preserving_extension()` (line 2113-2114). There are also tests for this behavior. The finding describes missing functionality that exists.

### FP-8: "No TLS certificate validation on WebSocket connection" (p3-pty_wrapper Finding 4)

**Reason:** The PTY wrapper connects to `ws://127.0.0.1:{port}` (line 163) -- a local loopback connection. TLS is not used and would be unnecessary for localhost IPC. The finding assumes a TLS connection that does not exist.

### FP-9: "Shell injection via unvalidated command/argument strings" (p1-daemon Chunk 3 Finding 1)

**Reason:** The actual code has `is_allowed_command()` (allowlist of specific commands), `is_shell_safe()` (rejects newlines, nulls, backticks, `$()`), and `shell_quote_posix()` (proper POSIX single-quoting). The review describes "unsanitized" input but the code has three layers of validation. While improvements to AppleScript escaping are valid (see H-2), the broad claim of "no sanitization" is incorrect.

### FP-10: "Floating-point division in rate_limit.rs is a performance concern" (p5-rate_limit)

**Reason:** Rate limiter `allow()` is called at most once per incoming request, not in a hot loop. The suggestion to precompute integer tokens-per-millisecond is premature optimization for a function called perhaps a few hundred times per second at most. The performance impact is negligible.

---

## Cross-Cutting Observations

1. **Authentication enforcement is the #1 priority.** The pairing token exists but is never enforced (C-1). Combined with 0.0.0.0 binding (H-1), any device on the same network can connect and issue arbitrary commands. Fixing C-1 would address the root cause and reduce the severity of several related findings.

2. **Panic-on-bad-input is a recurring pattern.** Multiple locations use `.unwrap()` or direct JSON field access without existence checks (H-7, H-8, H-9). A systematic audit for `unwrap()` and `expect()` calls on external input would be valuable.

3. **Unbounded resource allocation is systemic.** Unbounded channels (H-5), unlimited connections (H-6), and no session limits all share the same root pattern: no backpressure or caps on resource consumption from external input.

4. **The 14B review model has significant hallucination rates.** 10 out of ~43 raw findings (23%) were false positives, including findings about missing code that actually exists (FP-2, FP-3, FP-4, FP-6, FP-7), invented vulnerabilities in non-existent code paths (FP-8), and findings repeated 4x verbatim (FP-6). Review output from this model should always be verified against actual source code.

---

## Files Reviewed

| Pass | File | Review File |
|------|------|-------------|
| P1 | security.rs | p1-security.md |
| P1 | config.rs | p1-config.md |
| P1 | operations.rs | p1-operations.md |
| P1 | daemon.rs | p1-daemon.md |
| P2 | daemon.rs | p2-daemon.md |
| P3 | pty_wrapper.rs | p3-pty_wrapper.md |
| P3 | link.rs | p3-link.md |
| P3 | protocol.rs | p3-protocol.md |
| P4 | platform.rs | p4-platform.md |
| P4 | autostart.rs | p4-autostart.md |
| P4 | shell_hook.rs | p4-shell_hook.md |
| P4 | detection.rs | p4-detection.md |
| P5 | search.rs | p5-search.md |
| P5 | watcher.rs | p5-watcher.md |
| P5 | rate_limit.rs | p5-rate_limit.md |
| P5 | operations.rs | p5-operations.md |
| P6 | main.rs | p6-main.md |
| P6 | setup.rs | p6-setup.md |
| P6 | session.rs | p6-session.md |
| P6 | pty_wrapper.rs | p6-pty_wrapper.md |
| P6 | platform.rs | p6-platform.md |
| P6 | qr.rs | p6-qr.md |
