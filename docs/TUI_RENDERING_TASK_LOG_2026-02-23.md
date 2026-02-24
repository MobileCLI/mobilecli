# TUI Rendering Task Log

Date: 2026-02-23
Owner: Codex
Branch: `fix/tui-rendering-reliability-v3`

## Task T-001 - Baseline audit and failure-shape confirmation
Date: 2026-02-23
Owner: Codex

Hypothesis:
- Current failures come from non-semantic resize routing (keyboard/layout churn treated as PTY geometry) and missing server/wrapper-side guards.

Changes:
- No code changes.
- Reviewed `docs/TUI_RENDERING_RELIABILITY_MASTER_PLAN_2026-02-23.md`.
- Reviewed `docs/TUI_RENDERING_BUG.md`.
- Audited resize flow in:
  - `cli/src/protocol.rs`
  - `cli/src/daemon.rs`
  - `cli/src/pty_wrapper.rs`

Commands:
- `sed -n '1,260p' docs/TUI_RENDERING_RELIABILITY_MASTER_PLAN_2026-02-23.md`
- `sed -n '1,320p' docs/TUI_RENDERING_BUG.md`
- `rg -n "pty_resize|resize|SubscribeAck|keyboard|last_mobile_cols" cli/src mobile/components mobile/hooks`

Evidence:
- No semantic `reason` field in protocol.
- Daemon accepted resize events without reason-class filtering.
- Wrapper applied each resize directly; no reason-aware no-op/keyboard handling.

Result:
- pass

Next action:
- Implement semantic resize reasons and server-side decisioning first.

## Task T-002 - Daemon semantic resize policy + decision logging
Date: 2026-02-23
Owner: Codex

Hypothesis:
- Enforcing semantic reasons and explicit server-side decisions (including keyboard/no-op ignores) will eliminate resize churn reaching PTY and reduce TUI corruption.

Changes:
- `cli/src/protocol.rs`: added `PtyResizeReason` and optional `reason` on `ClientMessage::PtyResize`.
- `cli/src/daemon.rs`:
  - Added `ResizeRequest` channel type and reason propagation to wrapper.
  - Added `last_applied_size` session state.
  - Added `resolve_resize_reason` and `is_noop_resize` helpers.
  - Added structured resize-decision logs.
  - Added keyboard-overlay ignore policy (with synthetic ack).
  - Added no-op ignore policy (with synthetic ack).
  - Added resize coalescing before forwarding to wrapper.
  - Tagged restore path with `detach_restore`.

Commands:
- `cargo fmt --manifest-path cli/Cargo.toml`
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml daemon::tests:: -- --test-threads=1`

Evidence:
- `cargo check` passed.
- Daemon tests passed (12/12 for `daemon::tests::` filter), including new reason/no-op tests.

Result:
- pass

Next action:
- Update PTY wrapper to consume semantic reasons and apply transition-safe resize behavior.

## Task T-003 - PTY wrapper transition controller hardening
Date: 2026-02-23
Owner: Codex

Hypothesis:
- Wrapper-side reason-aware handling with no-op skipping + guaranteed `pty_resized` ack will stabilize attach/detach flows and avoid redundant SIGWINCH storms.

Changes:
- `cli/src/pty_wrapper.rs`:
  - Added `resolve_resize_reason` for incoming daemon resize events.
  - Added `last_applied_pty_size` tracking.
  - Added keyboard-overlay ignore path with explicit ack.
  - Added no-op resize skip while still acking epoch.
  - Added structured logs for received/applied/ignored decisions.
  - Added unit tests for reason resolution.

Commands:
- `cargo fmt --manifest-path cli/Cargo.toml`
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml pty_wrapper::tests:: -- --test-threads=1`

Evidence:
- `cargo check` passed.
- Wrapper tests passed (2/2 for `pty_wrapper::tests::` filter).

Result:
- pass

Next action:
- Apply minimal mobile emission changes so keyboard transitions remain local-only.

## Task T-004 - Validation checkpoint for CLI/daemon scope
Date: 2026-02-23
Owner: Codex

Hypothesis:
- End-to-end daemon/wrapper compile + targeted tests cover the newly introduced resize semantics sufficiently for PR review.

Changes:
- No code changes.

Commands:
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml daemon::tests:: -- --test-threads=1`
- `cargo test --manifest-path cli/Cargo.toml pty_wrapper::tests:: -- --test-threads=1`

Evidence:
- All targeted daemon/wrapper tests passed.
- Existing unrelated warning remains: `shell_hook::uninstall_quiet` dead code.

Result:
- pass

Next action:
- Open/submit daemon PR and request Greptile review.

## Task T-005 - Reopen history loss root-cause and daemon-only fix
Date: 2026-02-24
Owner: Codex

Hypothesis:
- Reopen history loss was caused by two daemon-side gaps:
  - frame-rendered sessions skipped subscribe replay and could return with only tiny redraw output,
  - 64KB scrollback rolled over too quickly for high-ANSI Codex/OpenCode flows.

Changes:
- `cli/src/daemon.rs`:
  - increased `DEFAULT_SCROLLBACK_MAX_BYTES` from `64 * 1024` to `512 * 1024`.
  - added deferred per-client replay queue for frame-rendered sessions: `pending_tui_replay`.
  - on `subscribe` for frame-rendered sessions with existing scrollback, queue one replay for post-resize ack.
  - on `broadcast_pty_resized`, send one replay payload (full scrollback) to queued viewers, then clear queue entry.
  - added cleanup for deferred replay state on unsubscribe/disconnect/session close/end.

Commands:
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml daemon::tests:: -- --nocapture`
- `cargo test --manifest-path cli/Cargo.toml pty_wrapper::tests:: -- --nocapture`
- websocket probe scripts (Python `websockets`) for attach/detach/reconnect flows with `get_session_history` + chunk-size capture.

Evidence:
- With old runtime binary path (`/home/bigphoot/.local/bin/mobilecli`) still active, reconnect showed:
  - `history_before = 3894`
  - post-resize `pty_bytes` sum = `323` (no full replay), matching user-visible "missing history".
- After installing patched binary to runtime path and restarting daemon:
  - reconnect showed `history_before = 3389` and first post-resize `pty_bytes` chunk = `3389` (full replay delivered).
  - follow-up test: replay occurs only once per reattach (first resize replayed, second resize in same attach did not replay).
- Scrollback-cap verification with high-volume bash output:
  - `get_session_history.total_bytes = 143392` (> 65536), confirming rollover headroom increase.

Result:
- pass

Next action:
- Add regression tests for one-shot deferred replay and larger default scrollback, then reinstall daemon binary for local runtime parity.

## Task T-006 - Regression tests and runtime parity install
Date: 2026-02-24
Owner: Codex

Hypothesis:
- Dedicated tests will prevent regression on the exact reconnect-history failure mode.

Changes:
- `cli/src/daemon.rs` tests:
  - added `default_scrollback_is_large_enough_for_frame_clis`.
  - added `pty_resized_replays_pending_tui_scrollback_only_once` (async test).

Commands:
- `cargo test --manifest-path cli/Cargo.toml daemon::tests::pty_resized_replays_pending_tui_scrollback_only_once -- --nocapture`
- `cargo test --manifest-path cli/Cargo.toml daemon::tests::default_scrollback_is_large_enough_for_frame_clis -- --nocapture`
- `cargo test --manifest-path cli/Cargo.toml daemon::tests:: -- --nocapture`
- `cargo test --manifest-path cli/Cargo.toml pty_wrapper::tests:: -- --nocapture`
- `install -m 755 cli/target/debug/mobilecli /home/bigphoot/.local/bin/mobilecli`

Evidence:
- New daemon tests pass.
- Full targeted daemon/wrapper suites pass.
- Runtime probe on installed daemon binary confirms reconnect first chunk equals pre-subscribe history bytes (`3219`), demonstrating working one-shot replay on actual running daemon.

Result:
- pass

Next action:
- Commit daemon-only patchset and continue mobile-side validation with no additional mobile code changes.
