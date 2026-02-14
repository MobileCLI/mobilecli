# Desktop System Component Map

Last updated: 2026-02-14
Scope: `cli/` Rust daemon + wrapper + filesystem + setup/link flows.

## 1. Runtime Topology

- `cli/src/main.rs`
  - Command router for: daemon start/stop/status, setup, QR display, session list, linked mode, wrapped PTY mode.
  - Delegates long-running websocket server to `daemon::run`.
- `cli/src/daemon.rs`
  - Single websocket server on `0.0.0.0:<port>`.
  - Accepts two client classes:
    - Mobile clients (`hello`, `get_sessions`, FS ops, push token registration, spawn requests).
    - PTY wrapper clients (`register_pty`, `pty_output`, terminal lifecycle).
  - Owns shared state: sessions, mobile subscribers, waiting states, push tokens, file watchers, rate limiters.
- `cli/src/pty_wrapper.rs`
  - Wraps spawned command in a PTY and streams terminal bytes to daemon.
  - Handles terminal resize and raw mode bridging.
- `cli/src/filesystem/*`
  - Policy-enforced file operations (allow roots, deny patterns, symlink policy, rate limits, watches, chunked reads).
- `cli/src/detection.rs`
  - Detects wait/approval prompts from terminal output and emits normalized wait state.
- `cli/src/setup.rs`, `cli/src/qr.rs`, `cli/src/link.rs`
  - First-run config wizard, QR pairing payload generation/display, linked-terminal mode.

## 2. Request/Response Flow Map

### 2.1 Mobile connect
1. Mobile opens WS -> sends `hello`.
2. Daemon accepts mobile client, replies `welcome` + `sessions` + waiting states.
3. Mobile subscribes/unsubscribes per active session.

### 2.2 PTY session registration
1. Wrapper opens WS and first message is `register_pty`.
2. Daemon creates `PtySession`, broadcasts session list update.
3. PTY output is fanned out via broadcast channel to subscribed mobile clients.

### 2.3 File upload from phone
1. Mobile sends `upload_file` with base64 payload and target `session_id`.
2. Daemon resolves session `project_path`.
3. Daemon sanitizes filename, builds destination under:
   - `<project>/.mobilecli/uploads/<stamp>-<short_uuid>-<sanitized_name>`
4. Filesystem service performs bounded, atomic write.
5. Daemon returns `operation_success` (saved path) or `operation_error`.

### 2.4 File edit/save from mobile viewer
1. Mobile `write_file` to protocol path.
2. Security validator resolves path, validates allow roots/no traversal/no symlink violations.
3. Atomic replace through temp + backup strategy.
4. File watcher broadcasts `file_changed` updates.

## 3. File + Function Index

## `cli/src/main.rs`
- `main`, `start_daemon_background`, `stop_daemon`, `show_status`, `run_setup`, `show_pair_qr`

## `cli/src/daemon.rs`
- Server lifecycle: `run`, `run_server_loop_unix`, `run_server_loop_ctrlc_only`, `handle_connection`
- Client handlers: `handle_mobile_client`, `handle_pty_session`, `process_client_msg`
- Session spawn path: `is_allowed_command`, `is_shell_safe`, `build_wrap_shell_command`, `spawn_session_from_mobile`, `detect_terminal_emulator`
- File op helpers: `send_fs_error`, `check_fs_rate_limit`, `send_sessions_list`, `broadcast_sessions_update`
- Upload hardening: `sanitize_upload_file_name`, `truncate_file_name_preserving_extension`, `truncate_utf8_to_max_bytes`, `build_upload_destination_path`
- Notifications/waiting: `build_notification_text`, `broadcast_waiting_for_input`, `broadcast_waiting_cleared`, `send_push_notifications`

## `cli/src/pty_wrapper.rs`
- Core wrapper: `run_wrapped`
- PTY/terminal helpers: `resolve_command`, `get_terminal_size*`, `request_terminal_resize`, `setup_raw_mode`, `restore_terminal_mode`

## `cli/src/session.rs`
- Session persistence/listing: `load_sessions`, `save_sessions`, `list_active_sessions`

## `cli/src/setup.rs`
- Config lifecycle: `load_config`, `save_config`, `is_first_run`
- Host/network: `get_hostname`, `get_local_ip`, `check_tailscale`
- Wizard: `run_setup_wizard`

## `cli/src/link.rs`
- Linked-terminal mode: `run`, `show_session_picker`, `run_linked_mode`

## `cli/src/detection.rs`
- Prompt normalization/detection: `strip_ansi_and_normalize`, `detect_wait_event`, plus CLI-specific heuristics.

## `cli/src/protocol.rs`
- Wire protocol enums for all client/server messages, including file system and upload operations.

## `cli/src/filesystem/config.rs`
- Limits and policy defaults: allowed roots, denied patterns, max read/write sizes, list limits, symlink policy.

## `cli/src/filesystem/security.rs`
- Path safety: `validate_existing`, `resolve_new_path`, allowlist/denylist enforcement, symlink checks.

## `cli/src/filesystem/operations.rs`
- Directory/file ops: list/read/chunk-read/write/create/delete/rename/copy/get info
- Atomic write and ENOTDIR mapping helpers.

## `cli/src/filesystem/watcher.rs`
- File change watcher and event classification.

## `cli/src/filesystem/search.rs`
- Filename/content search with result shaping.

## `cli/src/filesystem/git.rs`
- Optional git status integration for entries.

## `cli/src/filesystem/mime.rs`
- MIME detection and text/binary classification helpers.

## `cli/src/platform.rs`, `cli/src/filesystem/platform.rs`
- Cross-platform process checks and filesystem metadata formatting.

## 4. Hardening Changes Applied In This Audit

- Increased websocket `max_message_size`/`max_frame_size` to allow base64 uploads without protocol disconnects.
- Upload filename sanitization hardened:
  - invalid char replacement
  - reserved Windows device-name handling
  - UTF-8 safe truncation
  - conservative filename budget aligned with atomic temp suffix behavior
- Added upload-focused tests in `daemon.rs`.
- Added message parse debug logs for easier protocol diagnostics.

## 5. Current Risks / Follow-ups

- Native app config sync warning in Expo workflow still needs policy decision:
  - keep prebuild-managed native dirs and manually sync config changes
  - or move fully back to CNG/prebuild workflow.
- Continue long-run soak tests for large uploads over weak/mobile networks.
- Add integration tests for daemon websocket protocol (currently most coverage is unit-level + manual smoke).
