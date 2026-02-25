# Review: daemon.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:55:26-08:00

## daemon.rs (2624 lines, 6 chunks)

### Chunk 1/6 (lines 1-500)

**Finding 1:** The daemon binds to `0.0.0.0` and accepts connections from any IP address without authentication. This allows unauthenticated access from external networks.

*Severity:* High

*Description:* Binding the WebSocket server to all interfaces (`0.0.0.0`) exposes it to any device on the local network or internet, bypassing intended pairing/token-based authentication checks. An attacker could connect directly and perform actions such as file uploads/downloads without proper authorization.

**Fix:** Bind only to `127.0.0.1` (localhost) so that connections are restricted to the same machine where MobileCLI is running, eliminating external network exposure.

---

REVIEW_COMPLETE

### Chunk 2/6 (lines 501-1000)

**Finding 1:** The `handle_pty_session` function validates the command name but does not sanitize or escape it before passing to shell execution. An attacker could craft a malicious command that bypasses validation by using shell metacharacters (e.g., semicolons, backticks) and still execute arbitrary code.

**Fix 1:** Add proper escaping of all user-provided input fields in `handle_pty_session` before constructing the final shell command line. Use functions like `shell_quote_posix` to safely quote arguments and ensure no injection vectors remain.

REVIEW_COMPLETE

### Chunk 3/6 (lines 1001-1500)

**Finding 1:** The daemon spawns shell commands without sanitizing the `command` and `args`. An attacker could inject malicious arguments (e.g., `--help; rm -rf /`) which would be executed in the user's terminal session.

*Severity:* High

*Description:* Shell injection via unvalidated command/argument strings can lead to arbitrary code execution with elevated privileges. The daemon should validate and sanitize inputs before spawning processes, or use a safer API that prevents shell interpretation of arguments.

**Finding 2:** When using `tmux` in headless mode the session name is derived directly from user input without sanitization. An attacker could craft a malicious session ID (e.g., containing semicolons) to inject additional tmux commands into the command line, potentially gaining unauthorized access or causing denial of service.

*Severity:* Medium

*Description:* The daemon constructs `tmux` sessions using unsanitized names from client requests. Attackers can exploit this by injecting special characters that alter session creation behavior (e.g., semicolons to inject additional commands). Sanitizing the name before passing it to tmux prevents injection attacks.

**Finding 3:** When spawning iTerm2 or Terminal.app via AppleScript, user input is directly embedded into the script without escaping. An attacker could provide a malicious `shell` value (e.g., containing semicolons) that would be executed in the target terminal session.

*Severity:* High

*Description:* The daemon builds AppleScript commands by interpolating shell arguments directly. If an attacker can control these values, they may inject additional script code or escape sequences to execute arbitrary actions within the user's terminal emulator. Proper escaping and validation of all inputs is required before embedding them into scripts.

**Finding 4:** When spawning processes in headless mode without a detected terminal emulator, the daemon directly executes shell commands using `Command::new(&shell)`. This bypasses any sandboxing or security checks that might be applied to terminal sessions. An attacker could exploit this by crafting malicious shell values (e.g., containing semicolons) to execute arbitrary code.

*Severity:* Medium

*Description:* The headless path spawns processes directly without additional validation, allowing injection of malicious arguments into the underlying shell command line. Adding input sanitization and validating that `shell` is a safe executable before spawning would mitigate this risk.

**Finding 5:** When handling file system operations (list_directory/read_file) the daemon does not validate whether the requested path points to sensitive files such as `.ssh/authorized_keys`, environment variables, or other protected data. An attacker could request these paths and obtain unauthorized access to private information.

*Severity:* High

*Description:* The daemon allows clients to read arbitrary file system entries without any filtering for sensitive directories (e.g., .ssh/, /etc). This can expose private keys, configuration files, or credentials that should not be accessible over the mobile client. Implementing a blacklist of protected paths and enforcing access controls would prevent unauthorized disclosure.

**Finding 6:** The daemon does not enforce authentication before spawning terminal sessions. An unauthenticated attacker could connect to the WebSocket server and spawn arbitrary shell commands on behalf of any user, leading to privilege escalation or denial-of-service attacks.

*Severity:* Critical

*Description:* All session creation endpoints are accessible without proper authentication checks. This allows unauthorized users to execute code in target terminals, potentially gaining full access to the host system. Implementing robust authentication (e.g., JWT tokens) and enforcing it for all spawn operations would prevent unauthenticated spawning of terminal sessions.

**Finding 7:** When using `tmux` in headless mode the daemon does not verify that the requested session name is valid or safe before creating a new tmux session. An attacker could provide an invalid name (e.g., containing spaces, semicolons) which would cause tmux to fail and potentially allow injection of additional commands.

*Severity:* Medium

*Description:* The headless path creates `tmux` sessions using the client-provided name without validation. If the name contains special characters or is malformed it can lead to session creation failures or unintended command execution within tmux. Adding input sanitization for session names would prevent injection attacks and ensure valid session identifiers.

**Finding 8:** The daemon does not enforce rate limiting on file system operations (list_directory/read_file) when spawning terminal sessions via the mobile client. An attacker could flood the server with rapid requests, causing denial of service or resource exhaustion while also bypassing any per-session limits that might be in place for other endpoints.

*Severity:* Medium

*Description:* The daemon allows unthrottled file system access from spawned terminal sessions, which can lead to excessive I/O and potential abuse. Implementing rate limiting on these operations would prevent malicious clients from overwhelming the server with requests while still allowing legitimate usage patterns.

**Finding 9:** When spawning iTerm2 or Terminal.app via AppleScript, user input is directly embedded into the script without escaping. An attacker could provide a malicious `shell` value (e

### Chunk 4/6 (lines 1501-2000)

**Finding 1:** Path traversal vulnerability in `UploadFile` handler allows uploading files outside the project directory by manipulating session IDs or using relative paths.

*Severity:* High

*Description:* The upload endpoint validates only that a session exists but does not enforce strict path sanitization, enabling attackers to craft malicious file names (e.g., "../" sequences) and write arbitrary files on disk. This can lead to unauthorized access or data leakage.

*Fix:* Implement comprehensive path validation using `path_utils::validate_path` before writing the uploaded content, ensuring only allowed project directories are writable.

**Finding 2:** Missing authentication check for file operations in daemon handler.

*Severity:* Medium

*Description:* All file system operations (read/write/delete) bypass any session or permission verification. An unauthenticated client can perform arbitrary filesystem actions on behalf of other users, leading to unauthorized data access and potential privilege escalation.

*Fix:* Introduce a central authentication layer that validates the caller's identity before processing each request, rejecting requests from unknown clients with appropriate error responses.

**Finding 3:** Race condition in `WatchDirectory` handler when multiple watchers are added concurrently.

*Severity:* Medium

*Description:* The code increments watch counts without proper synchronization. If two concurrent connections attempt to start watching the same directory simultaneously, one may miss the watcher registration and receive duplicate events or stale state, leading to inconsistent file change notifications.

*Fix:* Use atomic operations (e.g., `AtomicUsize`) for count updates and consider acquiring a lock around subscription management to guarantee exclusive access during modification.

**Finding 4:** Potential TOCTOU race in directory validation when multiple threads modify the filesystem concurrently.

*Severity:* Low

*Description:* Between validating that a path exists as a directory and actually watching it, another thread could delete or rename the target. The watcher may then receive events for non-existent paths, causing confusion but not security breach.

*Fix:* Validate existence again after acquiring a lock on the watch state before invoking `watch`, ensuring consistency between validation and registration steps.

**Finding 5:** Sensitive file exposure via unfiltered upload handling in `UploadFile`.

*Severity:* High

*Description:* The handler accepts any MIME type without filtering, allowing uploads of sensitive files (e.g., `.env` or SSH keys) that could be accessed later. This can lead to credential leakage if the uploaded content is stored under a predictable name.

*Fix:* Implement strict MIME whitelisting and reject uploads containing known sensitive file extensions before writing them to disk.

**Finding 6:** Missing rate limiting for `GetHomeDirectory`.

*Severity:* Low

*Description:* The handler does not participate in the global rate limiter, allowing rapid repeated requests that could exhaust system resources or be used as a denial‑of‑service vector. This is less critical than other findings but still represents an oversight.

*Fix:* Add explicit rate limiting for `GetHomeDirectory` to align with existing limits and prevent abuse of this endpoint.

**Review_COMPLETE:**

### Chunk 5/6 (lines 2001-2500)

**Finding 1:** The daemon does not enforce authentication for any WebSocket endpoint, allowing unauthenticated clients to perform filesystem operations and access session data.

*Severity:* High

*Description:* All handlers in `handle_client_message` are accessible without prior authentication checks, enabling unauthorized file reads/writes or session manipulation. This violates the intended security model where only authenticated users should interact with the daemon.

**Fix:** Implement a per‑connection authentication mechanism (e.g., token verification) and enforce it before processing any client messages. Reject unauthenticated requests early in `handle_client_message`.

---

REVIEW_COMPLETE

### Chunk 6/6 (lines 2501-2624)

**Finding 1:** The `sanitize_upload_file_name` function replaces invalid characters but does not enforce a maximum length limit for the sanitized name. This could allow extremely long filenames that may exceed filesystem limits or cause performance issues.

**Fix**: Add an explicit check to ensure the sanitized filename does not exceed MAX_UPLOAD_FILE_NAME_BYTES (256 bytes) and truncate if necessary, preserving file extensions when truncating.

REVIEW_COMPLETE

---
Complete: 2026-02-20T17:56:12-08:00
