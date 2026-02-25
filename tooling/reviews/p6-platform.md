# Review: platform.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:58:23-08:00

## platform.rs (186 lines)

**Finding 1:** `config_dir()` uses `.unwrap_or_else(|| PathBuf::from("."))` which panics if the home directory cannot be determined (e.g., on systems without a user profile). This can cause MobileCLI to crash during initialization.

*Fix:* Replace with safe handling, e.g. return an empty path or log error instead of unwrapping.

**Finding 2:** `default_shell()` uses `.unwrap_or_else(|_| "/bin/sh".to_string())` on Unix platforms when the `$SHELL` environment variable is missing. While unlikely to panic in production, it silently falls back without logging, making debugging difficult if a user has no shell set.

*Fix:* Log a warning or error message indicating that the default shell fallback was used and provide guidance for setting `$SHELL`.

**Finding 3:** The Windows implementation of `default_shell()` uses unsafe FFI calls to query process information. If these fail (e.g., due to insufficient permissions), they may panic, causing MobileCLI to crash.

*Fix:* Wrap the unsafe block in a safe wrapper that returns an error instead of panicking and handle it gracefully elsewhere in the codebase.

**Finding 4:** `is_process_alive()` on Windows uses raw FFI calls without proper error handling. If `OpenProcess` or `GetExitCodeProcess` fail, they may return null handles which could lead to undefined behavior when dereferenced later.

*Fix:* Validate all handle returns and propagate errors instead of assuming success, ensuring robust process state checking even under failure conditions.

**Finding 5:** The Unix implementation of `is_process_alive()` uses `.unwrap_or_else(|| false)` on the result of `kill()`. While unlikely to panic in production, it silently ignores any error from `kill`, potentially leading to incorrect process status reporting without detection.

*Fix:* Log a warning or error when kill fails and return false explicitly, making failure modes observable for debugging purposes.

---
Complete: 2026-02-20T17:58:31-08:00
