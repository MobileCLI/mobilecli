# TMUX Runtime Execution Master Plan

Date: 2026-02-24
Status: Ready for implementation
Owner: Codex
Scope: `cli/` first, `mobile/` only where required for reliability
Supersedes: `docs/TMUX_ARCHITECTURE_REWRITE_PLAN_2026-02-24.md`

## 1) Mission and Non-Negotiables

Goal:
- A user can open/view/control the same desktop terminal session from phone with stable behavior.

Non-negotiables:
- No desktop terminal wipe/clear on mobile detach.
- No blank mobile reattach for Codex/TUI sessions.
- No duplicated input bars or ghost layers.
- Reconnect restores visible state and scrollback deterministically.
- Works for unknown future terminal tools as long as they run in a terminal.
- Keep existing mobile protocol compatible during migration.

## 2) Verified Baseline Facts (No Assumptions)

### 2.1 Current implementation facts from code

1. Wrapper currently contains explicit local clear logic in resize path.
- File: `cli/src/pty_wrapper.rs`
- Functions: `clear_local_terminal_view`, `should_clear_local_before_resize`, resize handling in `run_wrapped`.

2. Daemon session model is wrapper/PTy-stream-centric.
- File: `cli/src/daemon.rs`
- Entry points: `handle_pty_session`, `process_client_msg`, `restore_pty_size`, `send_sessions_list`, `persist_sessions_to_file`.

3. Mobile suppresses PTY bytes for alt-screen sessions until `pty_resized` is observed.
- File: `mobile/hooks/useSync.ts`
- Logic: `subscribe_ack` sets suppression; `pty_bytes` dropped while suppressed; suppression released on `pty_resized`.

4. Mobile requests replay using `get_session_history` after resize ack.
- File: `mobile/hooks/useSync.ts`

5. Protocol is already resize-reason/epoch aware and supports `session_history`.
- File: `cli/src/protocol.rs`

6. Local environment has tmux available.
- Command: `tmux -V`
- Result: `tmux 3.4`

### 2.2 Verified tmux behavior from local probes

Probe A: control mode framing + notifications
- Command: `tmux -C ...`
- Observed: `%begin/%end`, `%session-changed`, `%layout-change`, `%output`, `%exit`.

Probe B: output encoding in control mode
- Observed `%output` contains escaped sequences (example `\015\012`) not raw bytes.
- Implication: parser/decoder is required before forwarding to `pty_bytes`.

Probe C: history capture fidelity
- Command: `capture-pane -p` and `capture-pane -p -e`
- Observed: `-e` preserves ANSI styling escapes in captured text.

Probe D: input injection works through tmux command channel
- Command: `send-keys -t %pane ...`
- Observed output echoed back through `%output`.

## 3) External Research Summary

Primary references:
- tmux control mode protocol and notifications.
- tmux session/window sizing policy (`window-size`: `largest|smallest|manual|latest`).
- tmux client flags (`ignore-size`, `read-only`, `pause-after`) for multi-client behavior.
- gotty/ttyd usage patterns showing tmux-backed sharing as established practice.

Sources:
- https://github.com/tmux/tmux/wiki/Control-Mode
- https://man7.org/linux/man-pages/man1/tmux.1.html
- https://github.com/tmux/tmux/wiki
- https://github.com/yudai/gotty
- https://github.com/tsl0922/ttyd/wiki
- https://github.com/xtermjs/xterm.js

## 4) Why Rewrite Is Required

The current daemon+wrapper path re-implements terminal multiplexer semantics manually:
- replay assembly,
- attach/detach ordering,
- resize race control,
- suppression timing coordination.

This is structurally fragile for frame-rendered TUIs. The reliability goal needs native multiplexer semantics, not more heuristics.

## 5) Target Architecture

## 5.1 Runtime abstraction

Introduce runtime backend abstraction in daemon:
- `PtyRuntime` (legacy backend)
- `TmuxRuntime` (new backend)

Selection:
- env/config flag `MOBILECLI_RUNTIME=pty|tmux`.
- initial default remains `pty` until tmux passes matrix.

## 5.2 Session model additions

Extend daemon session state with runtime metadata:
- `runtime: pty|tmux`
- tmux IDs:
  - `tmux_session`
  - `tmux_window`
  - `tmux_pane`
  - `tmux_control_client`
- authoritative `history_limit_lines` and current policy mode.

## 5.3 TmuxAdapter responsibilities

New module set (proposed):
- `cli/src/tmux/mod.rs`
- `cli/src/tmux/command.rs` (safe command builder/executor)
- `cli/src/tmux/control_parser.rs` (`%begin/%end/%output/...` parser)
- `cli/src/tmux/runtime.rs` (backend implementation)
- `cli/src/runtime/mod.rs` (backend trait)

Responsibilities:
- spawn/attach/kill tmux sessions
- maintain one control-mode client per managed session
- decode `%output` stream -> daemon `pty_bytes`
- map mobile input -> `send-keys` (raw and text-safe paths)
- produce `session_history` via `capture-pane` (`-e` mode when requested)
- manage resize policy through tmux commands only

## 5.4 Mobile compatibility contract

Phase 1-2 keep existing mobile protocol messages:
- `pty_bytes`
- `subscribe_ack`
- `pty_resized`
- `session_history`

Optional additive fields (backward compatible) for migration:
- in `sessions` items: `runtime`, `capabilities`
- in `subscribe_ack`: `runtime`, `replay_mode`

## 6) Size Policy (Explicit and Deterministic)

Single process cannot have multiple simultaneous PTY sizes. We will support explicit policy modes:

1. `active_client` (default)
- most recently interactive client drives tmux window size (`window-size latest` behavior aligned).
- mobile can be authoritative while actively interacting.

2. `desktop_priority`
- desktop-attached client dominates size unless user explicitly requests mobile control.

3. `fixed`
- canonical stable size; no dynamic resize by client activity.

Policy state tracked per session:
- active controller (`desktop|mobile|none`)
- last interaction timestamp per client
- current target size and reason

## 7) Detailed Workstreams

## W1 - Immediate Stabilization (short-lived before tmux cutover)

Purpose:
- prevent actively harmful behavior while rewrite is built.

Tasks:
- disable desktop clear on detach/restore path.
- stop adding new replay/suppression heuristics in `pty` backend.

Exit criteria:
- no explicit clear sequence emitted on detach path in wrapper logs.

## W2 - Runtime Backend Abstraction

Tasks:
- create backend trait for common operations:
  - spawn session
  - subscribe/unsubscribe viewer
  - send input
  - resize
  - history snapshot
  - close
- migrate daemon code paths to call backend interface.

Files:
- `cli/src/daemon.rs`
- new `cli/src/runtime/*`

Exit criteria:
- `pty` backend behavior unchanged under tests.

## W3 - Tmux Control-Mode Backbone

Tasks:
- implement robust parser for control-mode line stream:
  - `%begin/%end/%error`
  - `%output`
  - `%layout-change`
  - `%exit`
  - `%session-changed`
- implement output decode for escaped content from `%output`.
- forward decoded data as protocol `pty_bytes`.

Exit criteria:
- integration harness shows command output roundtrip from tmux to mobile stream.

## W4 - Input, History, and Session Lifecycle

Tasks:
- input mapping:
  - `send_input raw=true` path to tmux (byte-safe translation strategy)
  - normal text path with newline semantics parity.
- history mapping:
  - `get_session_history` -> `capture-pane` with configurable start/end and ANSI preservation mode.
- session close:
  - close pane/window/session and emit `session_ended` reliably.

Exit criteria:
- repeated reconnect gets deterministic non-empty history for Codex sessions.

## W5 - Resize and Policy Engine

Tasks:
- implement policy resolver with structured decision logs.
- map mobile/desktop resize intents to tmux commands.
- enforce one transaction pipeline: request -> apply -> ack.

Exit criteria:
- no stale resize race causes blank screen in harness tests.

## W6 - Mobile Adjustments (only where required)

Allowed changes (if needed for correctness):
- condition suppression behavior on runtime capability.
- for tmux runtime, avoid dropping useful bytes while waiting for ack when safe.
- simplify reconnect replay trigger if daemon provides authoritative snapshot immediately.

Files (if needed):
- `mobile/hooks/useSync.ts`
- `mobile/components/TerminalView.tsx`

Exit criteria:
- no app-side suppression deadlock on tmux sessions.

## W7 - Rollout and Default Switch

Tasks:
- expose runtime selector in config/env.
- run full matrix.
- switch default to tmux only after gates pass.
- keep `pty` fallback one release.

## 8) PR and Branch Plan

PR 1: runtime abstraction scaffold
- no behavior change.

PR 2: tmux parser + control client + output pipeline
- hidden behind flag.

PR 3: tmux input/history/close + tests

PR 4: resize policy engine + deterministic logs

PR 5: minimal mobile changes (only if required by runtime capability)

PR 6: docs + rollout flags + matrix results

Each PR must include:
- hypothesis
- changed files
- commands/tests run
- evidence snippets/screenshots
- risk and rollback note

## 9) Test Strategy

## 9.1 Automated (required)

Rust unit tests:
- control-mode parser correctness
- `%output` decode correctness
- policy resolver decisions
- history snapshot translation

Rust integration tests (new harness):
- spawn tmux session, emit output, verify websocket `pty_bytes`
- detach/reattach with no new output
- stale/out-of-order resize acks
- long-output history retrieval (Codex-like volume)

Existing checks:
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml`

Mobile checks when touched:
- `cd mobile && npx tsc --noEmit`

## 9.2 Manual matrix (required)

Hosts:
- KDE Konsole (required)
- one additional emulator (GNOME Terminal/WezTerm)

Clients:
- iOS
- Android

CLIs:
- Codex
- Claude
- bash
- vim
- htop
- less

Scenarios:
- 20x detach/reattach loop
- keyboard show/hide stress
- background/foreground reconnect
- dual view with each size policy mode
- long session scrollback recovery

Pass criteria:
- no desktop clear
- no blank mobile reattach
- no duplicate prompt bars
- history not unexpectedly chopped within configured limits

## 10) Observability and Evidence Requirements

Add structured logs for tmux backend:
- session_id
- runtime
- command/action
- size policy decision
- controller (`desktop|mobile`)
- requested size, applied size
- ack token/sequence
- replay source (`live_output|capture_pane`)

Task log file (required):
- `docs/TMUX_RUNTIME_TASK_LOG_2026-02-24.md`

Every task entry includes:
- hypothesis
- files touched
- commands run
- evidence
- result
- next action

No build/release without completed task-log evidence for each major change.

## 11) Risks and Mitigations

Risk: tmux control parser bugs cause dropped output
- Mitigation: parser unit tests + golden fixtures from real `%output` lines.

Risk: raw input parity differences vs current PTY path
- Mitigation: explicit raw-input conformance tests (arrow keys, ctrl keys, enter semantics).

Risk: Windows hosts without tmux
- Mitigation: runtime fallback to `pty` backend by platform capability.

Risk: performance/backpressure on high-volume output
- Mitigation: bounded queues, optional pause strategies, instrumentation for lag.

Risk: mobile suppression logic conflicts with tmux runtime
- Mitigation: runtime capability gating and staged mobile changes only when proven necessary.

## 12) Rollback Plan

- Runtime flag can force legacy backend:
  - `MOBILECLI_RUNTIME=pty`
- Keep legacy backend codepath until tmux default has one release soak.
- If critical regressions occur, revert default and keep tmux behind opt-in.

## 13) Pre-Implementation Verification Checklist

Must pass before coding phase starts:
- [x] tmux installed and version confirmed (`3.4`).
- [x] control mode notifications observed locally.
- [x] `%output` escaping behavior observed and documented.
- [x] `capture-pane -e` ANSI preservation observed.
- [ ] verify alt-screen capture strategy for curses workloads (`vim/htop`) with `capture-pane` modes.
- [ ] verify exact resize command mapping for per-policy behavior in control clients.
- [ ] verify macOS host behavior with tmux backend on one machine.

## 14) Definition of Done

Done when all are true:
- tmux backend supports spawn, input, output, history, resize, close.
- desktop detach does not clear terminal.
- Codex mobile reattach is reliable across stress matrix.
- logs and task evidence complete.
- tmux runtime can be enabled safely with fallback available.

## 15) Immediate Next Actions

1. Create task log file and record baseline probes.
2. Implement runtime abstraction scaffold (no behavior change).
3. Implement tmux control parser with fixture tests.
4. Wire tmux output to websocket `pty_bytes` behind runtime flag.
5. Validate Codex reattach loop on tmux runtime before any mobile edits.

