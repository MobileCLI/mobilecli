# TMUX Architecture Rewrite Plan

Date: 2026-02-24
Status: Proposed (execute after approval)
Scope: `cli/` daemon + wrapper + protocol integration, mobile protocol-compatible first

## 1. Objective

Build a reliable local terminal streaming system where:
- desktop and phone can control/view the same live terminal session,
- mobile uses mobile-native dimensions,
- reconnects do not blank or duplicate terminal UI,
- desktop terminal is never forcibly cleared by mobile detach,
- behavior remains stable for Codex/Claude/any TUI/CLI.

## 2. Why Current PTY Model Fails

Current design has daemon + wrapper manually coordinating:
- raw PTY byte replay,
- attach/detach resize races,
- suppression/unsuppression timing,
- epoch ordering across reconnects.

This has repeatedly produced:
- blank reattach windows,
- clipped history,
- duplicate prompt/input artifacts,
- desktop clears on detach.

Root issue: we are re-implementing terminal multiplexer semantics in ad-hoc logic.

## 3. Why tmux

tmux is a local terminal multiplexer built for exactly this problem:
- persistent session lifecycle,
- multi-client attach/detach,
- stable scrollback,
- full-screen app handling.

Control mode gives machine-readable events (`%output`, `%session-changed`, etc.) and explicit client sizing controls.

Research references:
- tmux control mode: https://github.com/tmux/tmux/wiki/Control-Mode
- tmux window size policy (`largest|smallest|latest|manual`): https://www.man7.org/linux/man-pages/man1/tmux.1.html
- gotty/ttyd recommend tmux for reliable shared single-process terminals:
  - https://github.com/yudai/gotty
  - https://github.com/tsl0922/ttyd

## 4. Important Constraint (must be explicit)

A single interactive terminal process has one effective PTY geometry at a time.

So "desktop and mobile both attached at different dimensions" is a policy choice, not a free capability.

We will support explicit size policy modes:
- `active_client` (default): whichever client is actively interacting drives size (`tmux window-size latest`).
- `desktop_priority`: desktop size wins unless user explicitly hands control to mobile.
- `fixed`: stable canonical size (no automatic resize), useful for maximum determinism.

## 5. Target Architecture

### 5.1 Session Runtime

Each MobileCLI session runs inside tmux.

Daemon owns a control-mode client per session group and maps:
- MobileCLI `session_id` -> tmux session/window/pane IDs.

### 5.2 Data Flow

- Output to mobile: tmux control-mode `%output` stream.
- Input from mobile: tmux `send-keys`/pane input command path.
- Reconnect history: tmux `capture-pane` (authoritative scrollback), not custom PTY ring replay semantics.
- Resize: daemon issues tmux client/window sizing commands based on selected policy.

### 5.3 Desktop Behavior

Desktop keeps normal terminal behavior; no daemon-driven clear/reset on detach.

## 6. Migration Strategy

## Phase 0 - Freeze and Cleanup

- Stop adding new replay/suppression heuristics to current PTY path.
- Remove known harmful behavior immediately (desktop clear-on-detach).
- Add feature flag:
  - `MOBILECLI_RUNTIME=pty|tmux` (default `pty` for short rollout period).

## Phase 1 - tmux Adapter (daemon-only)

Implement `TmuxAdapter` in daemon:
- create/list/attach/kill tmux sessions,
- start control-mode client (`tmux -CC ...`) and parse control stream,
- map `%output` to existing `pty_bytes` protocol for mobile compatibility,
- implement `get_session_history` via `capture-pane`.

## Phase 2 - Wrapper Integration

Update wrapper startup path:
- launch command inside tmux session (`new -A -s ...`),
- attach desktop terminal as normal tmux client,
- stop forwarding raw PTY websocket bytes for tmux sessions.

## Phase 3 - Resize Policy Engine

Implement explicit policy resolver in daemon:
- `active_client`, `desktop_priority`, `fixed`.
- no keyboard-overlay resize forwarding.
- deterministic logs for every size decision.

## Phase 4 - Mobile Simplification

Once tmux runtime is stable:
- remove alt-screen suppression/epoch dependency for tmux sessions,
- keep protocol shape stable where possible,
- request history from daemon/tmux as authoritative restore source.

## Phase 5 - Default Switch

- Run matrix validation.
- Make `tmux` default runtime.
- Keep `pty` runtime as fallback for one release, then deprecate.

## 7. Test and Validation Gates

## 7.1 Automated

- daemon unit tests for tmux parser, reconnect, size policy transitions.
- integration tests for:
  - detach/reattach with no output,
  - stale/out-of-order resize messages,
  - long Codex output history retrieval.

## 7.2 Manual Matrix

Required before default switch:
- CLIs: Codex, Claude, bash, vim, htop, less.
- Hosts: KDE Konsole + at least one additional terminal emulator.
- Devices: iOS + Android.
- Scenarios:
  - repeated detach/reattach,
  - keyboard show/hide storms,
  - background/foreground,
  - dual-view (desktop + mobile) with explicit size policy checks.

Pass criteria:
- no desktop clear on detach,
- no blank mobile reattach,
- no duplicated input bars/prompts,
- history restoration not clipped unexpectedly under configured scrollback limits.

## 8. Rollout and Risk Control

- Runtime flag default remains `pty` until matrix pass.
- Add runtime telemetry counters:
  - blank-reattach incidents,
  - duplicate-line detection signals,
  - reconnect restore latency.
- If tmux runtime fails any critical gate, keep fallback and block release.

## 9. Immediate Execution Tasks

1. Remove desktop clear-on-detach behavior in wrapper.
2. Create `TmuxAdapter` module and control-mode parser skeleton.
3. Add runtime feature flag plumbing.
4. Implement tmux-backed `get_session_history`.
5. Validate Codex reattach path end-to-end on tmux runtime.
6. Open focused PR: `feat/tmux-runtime-phase1` with evidence logs.

