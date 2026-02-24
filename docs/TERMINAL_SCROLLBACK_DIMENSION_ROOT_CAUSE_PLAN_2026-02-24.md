# Terminal Scrollback + Dimension Reliability Plan (2026-02-24)

## Goal

Make mobile viewing behave like a stable mirror of desktop terminal usage:

- no persistent "stuck mobile dimensions" after mobile leaves
- no session blanks on reopen
- reliable scrolling/history behavior in mobile and desktop
- no CLI-specific command hacks as primary strategy

## Confirmed Findings (Evidence-Based)

1. Some modern agent CLIs (verified: Codex, Kimi) use frame-style redraw output, not append-only line output.

- Observed control patterns in live PTY capture:
  - `CSI J` clears
  - repeated cursor repositioning
  - scroll-region resets
- Result: visible viewport content can be overwritten in place during normal output.

2. "History disappearing" is not solely a mobile rendering bug.

- Same overwrite behavior is visible in desktop terminal output stream for frame CLIs.
- This means terminal-native scrollback alone cannot guarantee full conversational history for every CLI.

3. Stuck dimensions are lifecycle/state issues, not just rendering.

- If mobile viewer state is stale (reconnect races, duplicate sockets), restore can be skipped.
- This leaves wrapper PTY in prior resized dimensions.

## Changes Already Applied in This Pass

1. Explicit detach restore from mobile leave path

- `mobile/app/session/[id].tsx`
- Sends `pty_resize` with `cols=0`, `rows=0`, `reason=detach_restore` before unsubscribe.

2. Keyboard resize churn suppression

- `mobile/components/TerminalView.tsx`
- Keyboard show/hide resize bursts are tagged `keyboard_overlay` (ignored by wrapper as geometry changes).

3. Wrapper restore baseline hardening

- `cli/src/pty_wrapper.rs`
- Preserve-mode restore now uses captured pre-mobile baseline when available.

4. Stale socket eviction by logical sender ID (new)

- `mobile/hooks/useSync.ts`: `hello` now includes `sender_id`.
- `cli/src/protocol.rs`: `ClientMessage::Hello` includes optional `sender_id`.
- `cli/src/daemon.rs`:
  - tracks `mobile_sender_addrs`
  - evicts prior socket for same sender ID
  - cleans associated view/watch state

5. Wrapper resize semantics cleanup (new)

- `cli/src/pty_wrapper.rs`
- `detach_restore` reason mapping corrected.
- No-op jitter redraw now restricted to `pty` runtime only (not tmux).

## Remaining Work (Execution Plan)

## Phase 1: Deterministic Viewer-State Validation

1. Run daemon in debug mode and capture attach/resize/unsubscribe/restore timeline.
2. Add a short reproducible probe script for:
   - subscribe
   - resize to mobile dims
   - unsubscribe
   - verify restore request reached wrapper
3. Validate no stale viewer entries persist after reconnect/reload loops.

Acceptance:
- Every focus-leave produces either explicit detach restore or last-viewer restore.
- No session remains at mobile dims after leave when no active viewer exists.

## Phase 2: Scrollback Semantics for Frame CLIs (Generic, Not CLI-Specific)

1. Keep terminal-fidelity path as-is for live control.
2. Add a daemon-side append-only "text transcript lane" derived from raw output stream:
   - control-sequence stripped
   - carriage-return normalized
   - bounded buffer (separate from terminal scrollback)
3. Expose transcript replay for mobile reopen and deep history scroll fallback.

Acceptance:
- Reopen never appears blank.
- Users can scroll prior conversational text even when terminal viewport is frame-redrawn.

## Phase 3: Mobile Scroll UX Reliability

1. Ensure full-surface gesture capture remains active after reconnect.
2. Verify scroll can start from any vertical region, not only over text glyphs.
3. Add regression checks for keyboard/input bar visibility during long output.

Acceptance:
- Scroll gesture works from anywhere in terminal pane.
- Input bar text never becomes invisible/cut off.

## Phase 4: Verification Matrix Before Next Build

Run and record outcomes for:

- Codex
- Claude
- Gemini
- Kimi
- OpenCode
- Plain shell

For each:

1. cold open
2. long output
3. mobile leave/reenter x3
4. deep scroll back
5. keyboard show/hide while output streams

Release gate:
- no stuck dimensions
- no blank reopen
- no duplicate input bars
- usable deep history path in mobile

## Notes

- This plan intentionally avoids relying on per-CLI launch flags as the primary fix strategy.
- For frame CLIs, terminal-native scrollback is inherently limited by in-place redraw behavior; transcript lane is the generic reliability layer.
