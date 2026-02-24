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

