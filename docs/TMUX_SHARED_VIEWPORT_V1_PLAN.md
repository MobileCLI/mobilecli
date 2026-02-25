# TMUX Shared Viewport V1 Plan

## Goal
Deliver deterministic tmux scrolling for mobile by making desktop authoritative for viewport content.

## Constraints
- Keep existing user terminal workflow (Konsole/Terminal/iTerm/PowerShell + tmux session).
- No client-side synthetic scrollback reconstruction.
- Preserve live interactive terminal behavior at bottom.

## Architecture
1. Mobile sends `tmux_viewport` action (`page_up`, `page_down`, `scroll_up`, `scroll_down`, `follow`).
2. Daemon executes tmux action with retry.
3. Daemon queries tmux viewport state (`copy_mode`, `scroll_position`, `history_size`).
4. Daemon captures visible pane frame from tmux (`capture-pane -p -e`, `-M` when in copy-mode).
5. Daemon sends `tmux_viewport_frame` (base64 bytes, action sequence, state fields).
6. Daemon sends `tmux_viewport_state` (same action sequence).
7. Mobile resets xterm canvas and writes frame bytes; no optimistic local paging.

## Protocol Additions
- `server.tmux_viewport_frame`
  - `session_id`
  - `action_seq`
  - `data` (base64)
  - `in_copy_mode`
  - `scroll_position`
  - `history_size`
  - `following_live`
- `server.tmux_viewport_state`
  - add optional `action_seq` for deterministic state sync.

## Mobile Behavior
- Block PTY live writes while:
  - tmux is not following live, or
  - a viewport action is in-flight.
- Apply only increasing `action_seq` frames.
- `follow` restores live mode; deferred keyboard input flushes after follow state confirmation.

## Keyboard/Toolbar Behavior
- iOS inset source of truth: prefer `screenY` derived inset.
- Android inset source of truth: prefer `height`.
- Clamp keyboard lift to avoid extreme jumps.
- Remove double safe-area accounting while keyboard is visible.

## Risk Controls
- Action sequencing prevents stale frame overwrites.
- Action retry remains in daemon.
- If frame capture fails, state still returns and client remains functional.

## Manual Acceptance Matrix
1. Swipe up in tmux session: mobile viewport changes each action, no stale frame.
2. Swipe down/follow: live output resumes and input echoes normally.
3. Rapid swipes: no crash, no out-of-order viewport render.
4. Heavy output while scrolled up: mobile remains on frozen viewport until follow.
5. iOS keyboard open/close: toolbar remains directly above keyboard with no large gap.
6. Gemini/Codex prompt editing with keyboard visible: input line remains visible.

## Out of Scope (V1)
- Per-character smooth inertial scroll.
- Multi-controller arbitration beyond existing controller reassignment.
- Windows-native tmux edge-case hardening outside WSL path.
