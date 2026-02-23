# TUI Rendering Stabilization Plan (Cross-Device, Future-Proof)

Date: 2026-02-23
Scope: `cli/` daemon + PTY wrapper and `mobile/` xterm WebView client

## 1. Goal

Make terminal rendering reliable for:
- Any CLI/TUI today (Codex, Claude, Gemini, OpenCode, shells, ncurses apps)
- Any device class (desktop, phone, tablet)
- Any viewport transitions (keyboard show/hide, rotation, split-screen, reconnect)
- Multi-view sessions (desktop + one or more mobile viewers)

Success means:
- No duplicate ghost rendering caused by host-window resize/reflow side effects
- No session cut-offs from resize storms, reconnects, or dropped output chunks
- Deterministic resize semantics independent of a specific CLI implementation

## 2. Ground Truth From Audit

### 2.1 Core behavioral decision

Default behavior should preserve host terminal geometry and resize only the child PTY.

Rationale:
- Xterm control sequence docs define `CSI 8 ; h ; w t` as terminal window manipulation. Triggering host window resizing invites emulator-specific reflow behavior that TUIs do not expect.
- Linux `TIOCSWINSZ` semantics send `SIGWINCH` to the foreground process group; that signal is what TUIs need for redraw. The child PTY resize is sufficient for functional correctness.

References:
- xterm control sequences (window manipulation): https://www.invisible-island.net/xterm/ctlseqs/ctlseqs.html
- `TIOCSWINSZ` + `SIGWINCH`: https://man7.org/linux/man-pages/man2/TIOCSWINSZ.2const.html

### 2.2 xterm.js constraints that affect architecture

- `term.write(...)` is asynchronous and buffered. Very fast producers can overflow buffers and drop data unless flow control is explicit.
- Fit/resizing and high-frequency writes need backpressure-aware handling.

References:
- xterm flow control guide: https://xtermjs.org/docs/guides/flowcontrol/
- xterm API `write`, `onResize`: https://xtermjs.org/docs/api/terminal/classes/terminal/

### 2.3 Keyboard/viewport reality

Mobile keyboard and viewport events are noisy and bursty. The visual viewport can resize independently of layout viewport; resize handling must be idempotent and debounced.

Reference:
- VisualViewport resize: https://developer.mozilla.org/en-US/docs/Web/API/VisualViewport/resize_event

## 3. Implemented Fixes (Current Branch)

Branch:
- Top-level repo: `fix/tui-resize-coordination`
- Nested mobile repo (`mobile/`): `fix/tui-resize-coordination`

### 3.1 PTY wrapper and daemon

- Added desktop resize policy in PTY wrapper:
  - `MOBILECLI_DESKTOP_RESIZE_POLICY=preserve` (default)
  - `mirror` (legacy behavior)
- Removed screen-clearing resize path from wrapper logic.
- Wrapper now emits `pty_resized` confirmations after applied PTY resize.
- Daemon now rebroadcasts `pty_resized` to active viewers only.

### 3.2 Mobile terminal path

- `pty_resized` now clears suppression in `useSync`.
- `pty_resize` messages are queueable during transient WS disconnects.
- Removed overlapping AppState resize path in `TerminalView`.
- Reduced duplicate resize notifications in `assets/xterm.html`.
- Switched to streaming UTF-8 decode to avoid multibyte split corruption.
- Session lifecycle consolidated to attach/detach callbacks.

### 3.3 Additional hardening in this pass

- Daemon alt-screen tracking now handles split escape sequences across PTY chunk boundaries.
- Daemon restore (`cols=0, rows=0`) is ignored when multiple viewers still watch a session.
- Resize epochs now propagate mobile -> daemon -> wrapper -> daemon -> mobile, and stale epoch requests are rejected server-side.
- Mobile reconnect path now re-sends last known dimensions to break suppression deadlocks.
- Session detach no longer sends eager `resize(0,0)`; daemon restores when last viewer unsubscribes.

## 4. Remaining Risk Areas

1. Output backpressure in `mobile/components/XTermView.tsx`
- Current queue still has bounded-drop behavior under sustained high throughput.
- Risk: partial history loss under very heavy output bursts.

2. Resize event storms and stale dims ordering
- Multiple async layers (RN layout, WebView, WS queue, daemon channel, PTY) can still reorder effectively under reconnect churn.

3. Alt-screen model is stronger but still heuristic
- We currently infer mode from known DECSET/DECRST sequences; unusual apps may use non-standard flows.

4. Cross-emulator behavior variance
- Konsole, GNOME Terminal, iTerm2, WezTerm, Kitty differ in host-resize and repaint behavior.

## 5. Stabilization Roadmap

## Phase A: Deterministic Resize State Machine (High Priority)

1. Introduce monotonic resize epochs per session.
- Mobile tags each outbound `pty_resize` with `epoch`.
- Wrapper/daemon echo `epoch` in `pty_resized`.
- Mobile clears suppression only for matching/latest epoch.

2. Ensure single source of truth for active dimensions.
- Store `last_applied_cols/rows` in daemon session state.
- Reject no-op or stale resizes server-side.

3. Collapse redundant restore paths.
- Keep restore exclusively server-owned when viewer count reaches 0.
- Treat client-side `resize(0,0)` as advisory only (or remove protocol path entirely).

Deliverable:
- Race-free handshake: `subscribe_ack -> resize(epoch) -> pty_resized(epoch)`.

## Phase B: Lossless Output Delivery Under Load (High Priority)

1. Replace drop-oldest queue strategy with watermark backpressure.
- Follow xterm guidance: pause/resume producer with ACK-based watermark.
- Since transport is WebSocket, apply logical ACK protocol between WebView and RN, and RN to daemon.

2. Add bounded memory policy without silent loss.
- If hard limit must trigger, inject explicit marker frame (`[output truncated due to overload]`) and metric event.
- Never silently discard bytes.

3. Add throughput telemetry.
- Queue depth, max lag ms, dropped bytes (must trend to zero), flush batch size.

Deliverable:
- No silent truncation during stress tests.

## Phase C: Mobile Viewport Robustness (Medium Priority)

1. Introduce viewport stabilization gate.
- Delay outward resize until dimensions stable for N ms and changed by threshold.
- Use VisualViewport where available, fallback to layout events.

2. Keyboard transition guardrails.
- Coalesce keyboard show/hide oscillations into one applied resize per transition.

3. Orientation and split-screen profile tests.
- Portrait/landscape flips while alt-screen is active.

Deliverable:
- Predictable resize rate and no resize thrash from keyboard animations.

## Phase D: Interop and Emulator Matrix (Medium Priority)

1. Validate `preserve` default across host emulators:
- Konsole, GNOME Terminal, iTerm2, WezTerm, Kitty.

2. Validate CLI matrix:
- main-screen output (shell, Claude-like)
- alt-screen TUI (Codex-like, ncurses, htop, vim, less)

3. Keep `mirror` as explicit legacy mode only.
- Document as compatibility fallback, not default.

Deliverable:
- Compatibility matrix checked and documented before merge.

## 6. Test Plan (Required Before Final Commit)

## 6.1 Automated checks

- `cargo check --manifest-path cli/Cargo.toml`
- `npx tsc --noEmit` in `mobile/`
- Add targeted unit tests:
  - alt-screen detector with chunk-split sequences (`1049h`, `1047h`, `47h` and leave variants)
  - resize gating with multi-viewer restore rejection

## 6.2 Scripted integration tests

- Simulate sequence:
  - subscribe
  - alt-screen enter
  - rapid resize burst
  - unsubscribe/reconnect
- Assert:
  - no stale suppression
  - latest epoch wins
  - no unexpected restore when viewer count > 1

## 6.3 Manual device matrix

- iOS (latest + one prior major), Android (two API levels)
- Small phone, large phone, tablet
- Desktop Linux/macOS host emulators

Scenarios:
- Keyboard up/down spam
- Rotate during active prompt
- Background/foreground app repeatedly
- Reconnect under packet loss or daemon restart
- Large output flood (long logs, rapid redraw TUI)

Acceptance criteria:
- No duplicate UI artifacts
- No blank top gaps after scrollback
- No history truncation except explicit overload marker
- Desktop view remains stable in preserve mode

## 7. Observability Additions

Add structured logs and counters (session-scoped):
- resize requested/applied (cols, rows, epoch)
- suppression set/cleared reason
- queued messages count and flush latency
- output queue high-water and drops
- viewer_count transitions and restore decisions

Use these to verify behavior before and after rollout.

## 8. Rollout Strategy

1. Keep feature flags/env controls:
- desktop resize policy (`preserve` default)
- optional experimental backpressure protocol

2. Release in two steps:
- Step 1: deterministic resize + multi-view safety + observability
- Step 2: backpressure/lossless pipeline

3. If regressions appear:
- fallback to prior stable path via flag without code rollback.

## 9. Proposed Immediate Next Implementation Tasks

1. Extend daemon tests to cover stale-epoch rejection in multi-resize reconnect scenarios.
2. Implement non-lossy high/low watermark output flow in WebView/RN path.
3. Add compatibility matrix checklist and record results in docs.
4. Add resize/apply telemetry dashboards for queue depth and ack lag.
