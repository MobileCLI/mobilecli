# Review: pty_wrapper.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:56:30-08:00

## pty_wrapper.rs (493 lines)

**Finding 1:** The `run_wrapped` function uses the session name directly from user input without sanitization or validation. An attacker could craft a malicious session name containing shell metacharacters (e.g., semicolons) that would be interpreted by the daemon, leading to command injection.

*Severity:* High
*File/Line:* pty_wrapper.rs:run_wrapped
*Description:* The function uses `config.session_name` directly in JSON messages sent to the daemon. If an attacker provides a session name like `"malicious; rm -rf /"`, it could be executed on the server side, causing data loss or other malicious actions.
*Fix:* Sanitize and validate the session name before using it. Reject names containing semicolons, newlines, or any shell metacharacters.

**Finding 2:** The `normalize_input_newlines` function converts all newline characters to `\r`, but does not handle multi-byte UTF-8 sequences correctly. If input contains non-Latin1 Unicode (e.g., emojis), the conversion can corrupt data by truncating bytes, leading to garbled output or crashes.

*Severity:* Medium
*File/Line:* pty_wrapper.rs:normalize_input_newlines
*Description:* The function replaces `\n` and `\r\n` with `\r`, but does not validate that each byte is a valid UTF-8 character. Multi-byte Unicode sequences can be split across the conversion, resulting in corrupted output or panics.
*Fix:* Validate input as proper UTF-8 before normalization and handle multi-byte characters safely.

**Finding 3:** The resize handling logic assumes `msg["cols"]` and `msg["rows"]` are always present. If either field is missing (malformed JSON), the code will panic when casting to u64, causing a crash in production.

*Severity:* High
*File/Line:* pty_wrapper.rs:resize branch of main event loop
*Description:* The resize handler directly casts `msg["cols"]` and `msg["rows"]` as u64 without checking for existence. If the daemon sends malformed JSON (e.g., missing fields), this will cause a panic at runtime.
*Fix:* Add explicit checks that both fields exist before casting, returning an error or defaulting to safe values.

**Finding 4:** The WebSocket connection is established but no validation of the server's certificate occurs when using TLS. An attacker could intercept traffic by presenting a self-signed certificate, leading to man-in-the-middle attacks and data exfiltration.

*Severity:* High
*File/Line:* pty_wrapper.rs:connect_async call
*Description:* The WebSocket connection is established without verifying the daemon's SSL/TLS certificate chain. This allows attackers with valid certificates (or through MITM) to intercept session traffic.
*Fix:* Use `tokio_tungstenite::client_with_config` and configure TLS verification, ensuring only trusted certificates are accepted.

**Finding 5:** The PTY resize logic does not handle the case where both local terminal size is unavailable AND mobile dimensions are zero. In this edge case it falls back to a default (80x24), which may be incorrect for non-standard terminals and could cause layout issues or data corruption.

*Severity:* Medium
*File/line:* pty_wrapper.rs:resize branch of main event loop
*Description:* When both local terminal size is unavailable AND mobile dimensions are zero, the code falls back to a hard-coded default (80x24). This may not match actual terminal geometry on some systems.
*Fix:* Add fallback logic that queries system defaults or uses platform-specific APIs when neither source provides valid dimensions.

**Finding 6:** The base64 encoding/decoding in `normalize_input_newlines` does not handle malformed input gracefully. If the daemon sends corrupted data, decoding will fail and panic, causing a crash during normal operation.

*Severity:* High
*File/line:* pty_wrapper.rs:resize branch of main event loop (base64 decode)
*Description:* The function decodes base64 without validating that the payload is properly padded or contains only valid characters. Malformed data can cause panics and disrupt session handling.
*Fix:* Validate input as proper base64 before decoding, returning an error if invalid.

REVIEW_COMPLETE

---
Complete: 2026-02-20T17:56:48-08:00
