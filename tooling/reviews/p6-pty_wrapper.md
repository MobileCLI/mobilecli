# Review: pty_wrapper.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:58:06-08:00

## pty_wrapper.rs (493 lines)

**Finding 1:** `unwrap()` on daemon port retrieval can panic if the config file is missing or corrupted.

*Severity:* High

*Description:* The code uses `.unwrap()` when reading the daemon's listening port from a configuration file (`get_port().unwrap_or(DEFAULT_PORT)`). If the file does not exist, contains malformed JSON, or lacks the expected `port` field, this will cause an unhandled panic and crash the CLI. This is especially problematic because it happens early in session setup before any error handling logic runs.

*Fix:* Replace `.unwrap()` with proper error propagation using `?`. The function should return a `Result`, allowing callers to handle missing or invalid configuration gracefully instead of panicking unexpectedly.

**Finding 2:** Silent swallowing of errors when writing PTY output and stdin input can lead to data loss without any indication.

*Severity:* Medium

*Description:* Both the local terminal writer (`writer.write_all`) and daemon WebSocket sender use `_ = ...` which discards I/O errors. If a write fails (e.g., network disconnect, full buffer), the error is ignored entirely, potentially causing silent data corruption or loss without any user notification.

*Fix:* Replace the discard with proper error handling that logs failures to `tracing::debug!`. This makes debugging easier and ensures operators are aware when communication channels become unavailable.

**Finding 3:** No graceful degradation strategy for daemon unavailability during session setup.

*Severity:* Medium

*Description:* The function assumes a running daemon at startup. If the daemon is not present or fails to start, `connect_async` will fail with an error that propagates as `WrapError::DaemonConnection`. However, there's no fallback logic – the CLI simply returns the error and exits without any user guidance on how to recover.

*Fix:* Implement a retry mechanism (with exponential backoff) for daemon connection attempts. Additionally, provide clear diagnostic messages indicating why the daemon could not be reached and suggest troubleshooting steps such as checking network connectivity or restarting the service.

**Finding 4:** No validation of `config.working_dir` when resolving it from environment variables.

*Severity:* Low

*Description:* The code uses `.unwrap_or_else()` on `std::env::current_dir()`, which can panic if the current working directory cannot be determined (e.g., in a chroot jail or after unmounting). While unlikely, this could cause an unexpected crash during session setup.

*Fix:* Replace with proper error handling that returns a descriptive `WrapError` instead of panicking. The caller should then handle the failure gracefully rather than abort abruptly.

**Finding 5:** No synchronization mechanism for concurrent access to PTY resources (reader/writer).

*Severity:* High

*Description:* Multiple threads concurrently read from and write to the same PTY master device without any locking or coordination. This can lead to race conditions, corrupted output sequences, or data loss when multiple operations occur simultaneously.

*Fix:* Introduce a `Mutex` around the PTY reader/writer pair (or use thread-safe abstractions provided by the portable-pty crate). Ensure that all read/write operations are performed under exclusive access to prevent concurrent modifications of shared state.

**Finding 6:** No explicit cleanup for WebSocket connections or file descriptors on error paths.

*Severity:* Medium

*Description:* While the main execution path closes resources (WebSocket, PTY handles) before returning, there's no guarantee that these cleanups occur if an early panic happens. This can lead to resource leaks such as open sockets and unclosed PTY devices.

*Fix:* Use `defer`-style cleanup patterns or ensure all error paths explicitly close the WebSocket connection and drop the PTY master/slave handles before returning, even in failure cases.

**Finding 7:** No handling of terminal resize events when daemon is unavailable.

*Severity:* Low

*Description:* The code only resizes the local terminal based on mobile input. If the daemon disconnects or fails to send resize messages, the local terminal may retain an outdated size, causing display artifacts without any user notification.

*Fix:* Implement a fallback mechanism that restores the original terminal dimensions when no resize information is received from the daemon (e.g., after a timeout). This ensures consistent visual state across all scenarios.

---
Complete: 2026-02-20T17:58:23-08:00
