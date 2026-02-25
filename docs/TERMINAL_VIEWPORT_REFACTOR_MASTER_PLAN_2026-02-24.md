# Terminal Viewport Refactor Master Plan

Date: 2026-02-24  
Status: Proposed (research-backed)  
Owner: Codex  
Scope: `cli/` + `mobile/` reliability pivot

## 1) Decision Summary

Recommended direction:
- Stop treating mobile viewport changes as the primary PTY geometry driver.
- Move to a desktop-canonical session model by default (or fixed canonical fallback), with mobile using a zoomed/fit terminal view.
- Keep an explicit opt-in "mobile takes control" mode for users who want true mobile PTY dimensions.
- Keep tmux as the session authority for replay/reconnect.

Why:
- It removes the highest-risk failure path: frequent PTY `SIGWINCH` churn from mobile layout/keyboard events.
- It matches behavior of mature mobile terminal tools (font/zoom control is a primary UX mechanism).
- It preserves session continuity and avoids desktop blank/clear regressions.

## 2) Evidence and Research Findings

### 2.1 tmux is built for this exact multi-client problem
- tmux control mode is a text protocol with output blocks (`%begin`/`%end`) and async notifications; it is designed for integrations and multi-view control surfaces.
- tmux window sizing is policy-driven (`window-size largest|smallest|manual|latest`), and client flags include `ignore-size` for clients that should not influence global size.

Implication:
- Size arbitration must be explicit policy, not accidental side effects of mobile attach/keyboard behavior.

### 2.2 Web terminal stacks commonly pair xterm frontend + websocket relay + multiplexer for sharing
- GoTTY docs explicitly state single-process sharing should be done through tmux/screen.
- Architecture pattern is relay output/input over websocket, not terminal-emulator-specific hacks.

Implication:
- MobileCLI should lean into tmux semantics and stop rebuilding multiplexer behavior with wrapper heuristics.

### 2.3 Mobile terminal UX commonly uses zoom/font controls
- Blink docs and README explicitly expose pinch to adjust terminal size/text scale.

Implication:
- A "zoomed-out desktop-fit" view is not a compromise; it is a normal terminal UX model on mobile.

### 2.4 xterm behavior relevant to current failures
- `@xterm/addon-fit` derives cols/rows from container dimensions and cell metrics.
- `onData` is the input event path from terminal-side typing.
- xterm supports DA/DSR-style terminal sequences (CSI `c`, CSI `> c`, DSR), so terminal report traffic must be handled safely.

Implication:
- Resizing is a product policy choice, not a technical necessity.
- Input filtering for terminal reports is still required in raw-input paths.

## 3) Root-Cause Reframing

Current pain comes from coupling too many concerns to live PTY resize:
- mobile keyboard/layout transitions,
- attach/reconnect replay timing,
- wrapper-host mirroring,
- frame-TUI redraw synchronization.

Even with debounces and suppression logic, this remains fragile under CLI diversity (Codex/OpenCode/Gemini/Kimi/curses apps).

Primary refactor target:
- Decouple mobile display ergonomics from PTY geometry by default.

## 4) Architecture Options

### Option A: Keep mobile-native PTY sizing as default (current direction)
Pros:
- Mobile can match native dimensions exactly.
Cons:
- Highest resize churn; recurring regressions (cutoff, blank reattach, duplicate bars).
Decision:
- Reject as default.

### Option B: Desktop-canonical PTY + mobile zoom/fit view (Recommended)
Pros:
- Stable PTY dimensions in normal workflow.
- Minimal resize churn, fewer redraw races.
- Desktop session continuity preserved.
Cons:
- Mobile may require smaller font / zoom-out to view wide TUIs.
Decision:
- Adopt as default.

### Option C: Fixed canonical PTY size (e.g., 120x35)
Pros:
- Max determinism.
Cons:
- Less ergonomic for some desktop windows.
Decision:
- Keep as fallback mode.

## 5) Target Policy Model

Per session, expose explicit `size_mode`:
- `desktop_canonical` (default)
- `mobile_canonical` (opt-in)
- `fixed_canonical` (fallback/debug)

Rules:
- In `desktop_canonical`:
  - Mobile keyboard transitions never emit PTY resize.
  - Mobile orientation/container changes do not emit PTY resize by default.
  - Mobile only changes local xterm fit/zoom.
- In `mobile_canonical`:
  - Mobile can emit PTY resize (attach/orientation only).
  - Keyboard overlays remain local-only.
- In `fixed_canonical`:
  - Ignore all client resize requests except explicit admin command.

## 6) Execution Plan (Phased)

### Phase P0 - Baseline and Instrumentation
Goals:
- Lock baseline behavior and metrics before more edits.
Tasks:
- Add structured logs for attach/replay/resize decisions with `session_id`, runtime, `size_mode`, reason, epoch.
- Add counters: dropped PTY chunks, replay source (`tmux_capture` vs `scrollback`), blank-snapshot retries.
Acceptance:
- One reproducible trace for Codex attach -> detach -> reattach cycle.

### Phase P1 - Daemon Size-Policy Engine
Goals:
- Centralize resize authority in daemon.
Tasks:
- Add `size_mode` to session state and protocol/session listings.
- Gate `pty_resize` forwarding by `size_mode`.
- Ignore keyboard overlay everywhere.
- In desktop-canonical mode, return synthetic `pty_resized` ack using current applied dims.
Files:
- `cli/src/daemon.rs`
- `cli/src/protocol.rs`
Acceptance:
- No PTY resize applied in desktop-canonical mode during mobile keyboard/orientation interactions.

### Phase P2 - Wrapper Simplification (remove legacy fragility)
Goals:
- Remove unnecessary host-mirroring artifacts from default path.
Tasks:
- Keep `MOBILECLI_DESKTOP_RESIZE_POLICY=preserve` default.
- Ensure no stdout clear/host resize logic is run in default mode.
- Keep mirror mode only as explicit opt-in legacy behavior.
- Remove dead code branches if no longer reachable.
Files:
- `cli/src/pty_wrapper.rs`
Acceptance:
- Desktop terminal never clears/blank-resets on mobile leave/reopen.

### Phase P3 - tmux Snapshot-First Reattach
Goals:
- Eliminate blank/chopped reattach.
Tasks:
- On subscribe/reconnect, always fetch authoritative `capture-pane -p -e` snapshot first for tmux runtime.
- Stream live bytes only after snapshot delivery checkpoint.
- Add retry/backoff for empty snapshots.
- Use size-mode aware capture behavior (include scrollback for text CLIs; visible-pane for frame TUIs).
Files:
- `cli/src/daemon.rs`
Acceptance:
- Reopen loop (20 cycles) never lands on blank terminal for Codex/Claude/OpenCode.

### Phase P4 - Mobile Zoomed Viewport UX
Goals:
- Deliver "desktop-fit" mobile usability without PTY resize coupling.
Tasks:
- Add adjustable terminal font scale preset + pinch support.
- Add explicit "Fit Desktop" action to auto-compute target local font size for canonical columns.
- Keep full-surface scroll/pan capture stable.
- Ensure keyboard appears without altering server geometry.
Files:
- `mobile/components/TerminalView.tsx`
- `mobile/components/XTermView.tsx`
- `mobile/assets/xterm.html`
Acceptance:
- User can read/control wide TUIs via zoom-out without triggering server resize.

### Phase P5 - Explicit Mobile Control Mode (Opt-in)
Goals:
- Preserve advanced use case when user truly wants mobile-native PTY size.
Tasks:
- Add session toggle in UI: `Desktop Fit` (default) / `Mobile Control`.
- Emit PTY resizes only while in `Mobile Control` mode.
- On exit from mobile control, restore canonical size with one deterministic transaction.
Files:
- `mobile/app/session/[id].tsx`
- `mobile/hooks/useSync.ts`
- `cli/src/daemon.rs`
Acceptance:
- Toggling modes is deterministic; no duplicate input bars, no clipped restore.

### Phase P6 - Validation Matrix + Release Gate
Goals:
- Stop build churn without objective quality gates.
Tasks:
- Run matrix across CLI types, device orientations, reconnect loops, host terminals.
- Require pass criteria before build submission.
Acceptance:
- All P0-P5 gates pass + manual matrix pass.

## 7) Test Matrix

CLI matrix:
- Frame/TUI-heavy: Codex, OpenCode, `vim`, `htop`, `less`.
- Stream/text: Claude, Gemini, shell output (`tail -f`, long logs).

Device matrix:
- iPhone portrait/landscape.
- Android portrait/landscape.
- iPad split-screen.

Behavior matrix:
- initial attach
- keyboard show/hide spam (50 toggles)
- orientation changes (10 cycles)
- detach/reattach loops (20 cycles)
- concurrent desktop + mobile control
- network drop/reconnect

Pass criteria:
- no blank reattach
- no duplicate input bars
- no forced desktop clears
- no chopped history after reopen
- smooth mobile scrolling over full terminal surface

## 8) Edge Cases To Explicitly Handle

- Terminal reports split across websocket chunks (already partly fixed; keep tests).
- Wide glyph/CJK/emoji width drift under small fonts.
- Alt-screen transitions right during reconnect snapshot.
- Session with very high scrollback (performance bound + truncation policy).
- Multiple mobile viewers with conflicting interactions.
- Host terminal differences (Konsole, GNOME Terminal, WezTerm, Kitty, iTerm2).

## 9) Logging and Task Discipline

- Continue task log in `docs/TMUX_RUNTIME_TASK_LOG_2026-02-24.md` with new task IDs for this pivot.
- Each task must include hypothesis, changes, commands, evidence, result.
- No build submission without matrix evidence attached.

## 10) Rollback and Safety

Feature flags:
- `MOBILECLI_SIZE_MODE_DEFAULT=desktop_canonical|mobile_canonical|fixed_canonical`
- per-session override allowed.

Rollback plan:
- If regressions appear, revert default to prior mode while keeping tmux snapshot improvements.

## 11) Immediate Next Steps

1. Implement P1 (daemon size policy gating) before any further mobile rendering tweaks.  
2. Implement P3 snapshot-first reattach hardening in parallel with P1 logs.  
3. After P1+P3 are green, implement P4 zoomed viewport UX.  
4. Only then expose P5 mobile-control toggle.

## 12) Sources

- tmux man page (control mode, client flags, window-size): https://man7.org/linux/man-pages/man1/tmux.1.html
- tmux control mode wiki: https://github.com/tmux/tmux/wiki/Control-Mode
- xterm.js addons guide (`FitAddon`, `onData`): https://xtermjs.org/docs/guides/using-addons/
- xterm.js window options security note: https://xtermjs.org/docs/api/terminal/interfaces/iwindowoptions/
- xterm.js supported terminal sequences (DA/DSR etc.): https://xtermjs.org/docs/api/vtfeatures/
- Blink gestures (pinch size adjust): https://docs.blink.sh/
- Blink README (pinch to zoom): https://github.com/blinksh/blink
- GoTTY architecture and tmux sharing guidance: https://github.com/yudai/gotty
