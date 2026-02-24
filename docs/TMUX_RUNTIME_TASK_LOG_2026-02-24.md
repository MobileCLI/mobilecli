# TMUX Runtime Task Log

Date: 2026-02-24
Owner: Codex
Branch: `feat/tmux-runtime-phase1`

## Task T-001 - Baseline architecture and failure-shape capture
Date: 2026-02-24
Owner: Codex

Hypothesis:
- Current PTY-centric attach/replay logic is structurally fragile for frame-rendered TUIs and should be replaced with tmux-backed session multiplexing.

Changes:
- No code changes.
- Audited current behavior and code paths:
  - `cli/src/daemon.rs`
  - `cli/src/pty_wrapper.rs`
  - `mobile/hooks/useSync.ts`
  - `mobile/components/TerminalView.tsx`
  - `mobile/app/session/[id].tsx`

Commands:
- `rg -n "should_clear_local_before_resize|clear|DetachRestore|subscribe_ack|pty_resized|suppressPtyUntilResize" cli/src mobile`
- `sed -n ...` on files listed above.

Evidence:
- Desktop clear logic exists in wrapper resize policy path.
- Mobile suppresses `pty_bytes` until `pty_resized` ack for alt-screen sessions.
- Daemon session model is tied to wrapper `register_pty` stream.

Result:
- pass

Next action:
- Validate tmux control-mode semantics and output/history behavior with local probes.

## Task T-002 - tmux capability probe and command semantics verification
Date: 2026-02-24
Owner: Codex

Hypothesis:
- tmux control mode and capture-pane provide sufficient primitives to replace custom PTY replay/resize heuristics.

Changes:
- No code changes.
- Ran local tmux probes for:
  - control mode notification framing,
  - `%output` data representation,
  - `capture-pane` ANSI behavior,
  - `send-keys` input roundtrip.

Commands:
- `tmux -V`
- tmux probe scripts using `tmux -L <sock> -f /dev/null ...`
- `man tmux | col -b | sed -n ...`

Evidence:
- `tmux 3.4` available locally.
- Control mode emits `%begin/%end`, `%output`, `%layout-change`, `%exit`.
- `%output` is escaped text (decoded parser needed).
- `capture-pane -e` preserves ANSI escapes.
- `send-keys` roundtrip observed in `%output` stream.

Result:
- pass

Next action:
- Finalize comprehensive execution plan with explicit phases, risks, test gates, and rollback.

## Task T-003 - Master execution plan publication
Date: 2026-02-24
Owner: Codex

Hypothesis:
- A file-level, phase-gated plan with explicit verification checkpoints will prevent further speculative fixes and support reliable execution.

Changes:
- Added:
  - `docs/TMUX_RUNTIME_EXECUTION_MASTER_PLAN_2026-02-24.md`

Commands:
- Document authoring only.

Evidence:
- Plan includes:
  - validated baseline facts,
  - tmux-backed target architecture,
  - runtime abstraction strategy,
  - phased PR sequence,
  - automated/manual matrix gates,
  - risk and rollback controls,
  - pre-implementation verification checklist.

Result:
- pass

Next action:
- Begin Phase W2 runtime abstraction scaffold on `feat/tmux-runtime-phase1`.

## Task T-004 - Wrapper stabilization and tmux runtime cutover
Date: 2026-02-24
Owner: Codex

Hypothesis:
- Running wrapped sessions through a tmux-backed runtime and removing local clear sequences will eliminate desktop wipe behavior and provide multiplexer semantics needed for frame-TUI reliability.

Changes:
- Updated `cli/src/pty_wrapper.rs`:
  - Added runtime resolver (`MOBILECLI_RUNTIME=auto|tmux|pty`), defaulting `auto` to tmux when available.
  - Added tmux session bootstrap (`new-session`) and cleanup (`kill-server`) helpers.
  - Session registration now reports runtime metadata to daemon.
  - Wrapper now launches tmux attach client in PTY when tmux runtime is active.
  - Removed destructive local terminal clear path from resize handling.
- Added wrapper tests:
  - runtime override behavior,
  - tmux token sanitization,
  - tmux bootstrap/cleanup roundtrip.

Commands:
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml --bin mobilecli -- --skip test_list_directory_sorts_directories_first`
- `rg -n "2J\\x1b\\[H|clear_local_terminal_view|should_clear_local_before_resize" cli/src/pty_wrapper.rs cli/src/daemon.rs`

Evidence:
- `rg` returned no matches for local clear/clear-policy functions in wrapper/daemon.
- tmux lifecycle unit test passes: `pty_wrapper::tests::tmux_session_bootstrap_and_cleanup_roundtrip`.
- Resize handling still preserves `attach_init/reconnect_sync` no-op redraw path via jittered PTY resize.

Result:
- pass

Next action:
- Propagate runtime metadata through daemon session surfaces and increase replay headroom.

## Task T-005 - Daemon runtime metadata and replay headroom
Date: 2026-02-24
Owner: Codex

Hypothesis:
- Tagging session runtime at daemon level and increasing scrollback headroom reduces replay truncation risk and enables runtime-aware diagnostics during rollout.

Changes:
- Updated `cli/src/daemon.rs`:
  - `PtySession` now stores `runtime`.
  - registration parser accepts `runtime` from wrapper (`register_pty` payload).
  - sessions list serialization includes runtime.
  - increased `DEFAULT_SCROLLBACK_MAX_BYTES` from `512KB` to `2MB`.
  - updated tests/fixtures for new struct field and constant.
- Updated `cli/src/protocol.rs`:
  - `SessionListItem` now includes optional `runtime`.

Commands:
- `cargo fmt --manifest-path cli/Cargo.toml`
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml --bin mobilecli -- --skip test_list_directory_sorts_directories_first`
- `rg -n "runtime" cli/src/pty_wrapper.rs cli/src/daemon.rs cli/src/protocol.rs`

Evidence:
- Test run after formatting: `35 passed; 0 failed` (with one known filtered test).
- Runtime metadata appears in:
  - wrapper registration payload,
  - daemon session state,
  - protocol session list schema.

Result:
- pass

Next action:
- Commit changes and run target manual validation loop on Codex session attach/detach cycles using tmux runtime.

## Task T-006 - tmux reconnect replay correctness hardening
Date: 2026-02-24
Owner: Codex

Hypothesis:
- Blank reattach and "terminal code pasted into chat" regressions come from replaying raw daemon PTY scrollback for tmux sessions instead of replaying an authoritative tmux pane snapshot.

Changes:
- Updated `cli/src/daemon.rs`:
  - Added tmux metadata to in-memory session state (`tmux_socket`, `tmux_session`).
  - Derived tmux identifiers from `session_id` at registration for runtime `tmux`.
  - Added tmux snapshot helpers:
    - `capture_tmux_history_blocking`
    - `capture_tmux_history`
    - `tail_scrollback_bytes` (fallback path).
  - `GetSessionHistory` now prefers tmux `capture-pane -p -e -S -200000` for tmux sessions; falls back to daemon buffer only on capture failure.
  - `Subscribe` now:
    - disables deferred raw replay for tmux runtime,
    - sends immediate `session_history` snapshot from tmux when available.
  - `broadcast_pty_resized` now skips deferred raw replay for tmux sessions (prevents corrupted frame replays).
- Added daemon test:
  - `tmux_capture_history_returns_snapshot_text`.

Commands:
- `cargo fmt --manifest-path cli/Cargo.toml`
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml --bin mobilecli -- --skip test_list_directory_sorts_directories_first`

Evidence:
- Test suite result after patch: `36 passed; 0 failed` (1 filtered known filesystem test).
- New tmux capture test validates snapshot contains expected marker from live tmux session.
- Deferred raw replay path explicitly bypassed for runtime `tmux`.

Result:
- pass

Next action:
- Install updated release binary locally and run Codex mobile attach/detach verification loop with logs.
