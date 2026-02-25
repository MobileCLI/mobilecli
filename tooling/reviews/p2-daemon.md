# Review: daemon.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:56:13-08:00

## daemon.rs (2624 lines, 6 chunks)

### Chunk 1/6 (lines 1-500)

**Finding 1:** The daemon's `handle_connection` function spawns a new task for every incoming connection but never waits for it to finish before accepting the next one. This can lead to unbounded growth of spawned tasks if many clients connect rapidly and may cause resource exhaustion.

**Fix**: Introduce a bounded semaphore (e.g., `tokio::sync::Semaphore`) with a limit equal to the expected maximum number of concurrent connections, acquiring permits before spawning each task and releasing them after the task completes. This prevents uncontrolled growth while still allowing graceful handling of legitimate connection spikes.

REVIEW_COMPLETE

### Chunk 2/6 (lines 501-1000)

**Finding 1:** The daemon uses `mpsc::unbounded_channel` for PTY input and resize messages without back‑pressure handling. Uncontrolled growth can exhaust memory from malicious clients.

**Fix**: Switch to bounded channels with a reasonable capacity (e.g., 1024) or implement flow control logic that drops excess inputs instead of allocating unboundedly.

REVIEW_COMPLETE

### Chunk 3/6 (lines 1001-1500)

**Finding 1:** The daemon spawns a new process for every session without limiting the number of concurrent processes. An attacker could flood the server with many sessions and exhaust system resources.

**Fix**: Introduce a configurable maximum number of active sessions (e.g., `MAX_SESSIONS`) and reject spawn requests when this limit is reached, returning an error to the client instead of silently failing or leaking memory.

REVIEW_COMPLETE

### Chunk 4/6 (lines 1501-2000)

**Finding 1:** The daemon does not enforce rate limiting for the `get_home_directory` operation, allowing unthrottled access to potentially sensitive information.

**Fix**: Add a call to `check_fs_rate_limit(state, addr).await?` before handling the request and return an error if throttling is required.

### Chunk 5/6 (lines 2001-2500)

**Finding 1:** The daemon's `handle_client_message` function processes client requests but never validates the request ID format or length. An attacker could send malformed IDs to exhaust memory or cause denial-of-service by triggering excessive logging/error handling.

**Fix**: Add validation of `request_id` (e.g., ensure it is non-empty and contains only allowed characters) before processing each message, returning a clear error when invalid.

---

REVIEW_COMPLETE

### Chunk 6/6 (lines 2501-2624)

**Finding 1:** The daemon uses a shared HTTP client without proper connection management; long-lived connections can exhaust resources and cause deadlocks under load.

**Fix**: Introduce per-request clients with explicit timeout handling to ensure timely closure of idle connections.

REVIEW_COMPLETE

---
Complete: 2026-02-20T17:56:30-08:00
