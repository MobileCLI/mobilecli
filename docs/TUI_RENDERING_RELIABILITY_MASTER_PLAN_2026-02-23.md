# TUI Rendering Reliability Master Plan

Date: 2026-02-23
Status: Ready for execution
Scope: `cli/` + `mobile/` (daemon-first, mobile-minimal where possible)
Related: `docs/TUI_RENDERING_BUG.md`, `docs/TUI_RENDERING_STABILIZATION_PLAN.md`

## 1. Mission

Deliver deterministic, cross-device terminal rendering where:
- desktop and mobile can view the same live session simultaneously,
- Codex-style full-screen TUIs do not duplicate or clip UI,
- keyboard transitions do not cut sessions or corrupt layout,
- stable behavior for Claude/shell/text workflows stays intact.

This plan is designed to avoid more "build and hope" loops. Every change must be tied to a hypothesis, instrumentation, and explicit pass/fail results.

## 2. Non-Negotiable Requirements

- No duplicate prompt boxes, no ghost layers, no left-shifted/cut-off TUI frames.
- No chat/session truncation after keyboard show/hide, reconnect, or background/foreground.
- Desktop host window must remain unchanged by default (`preserve` policy), while mobile stays native-sized on mobile.
- Mobile keyboard behavior must prioritize UX without creating PTY resize storms.
- Existing working behavior for Claude/main-screen text flows must not regress.
- Solution must be CLI-agnostic and future-compatible for unknown terminal agents.

## 3. Root-Cause Framing (Current Best Model)

1. Codex and similar TUIs use alternate screen + full-screen redraw patterns.
2. Claude-like flows are mostly main-screen streaming text, which tolerates resize churn better.
3. Keyboard-driven mobile layout changes currently propagate into PTY resize too often.
4. Resize storms plus emulator redraw/reflow behavior create artifacts (duplicates, partial redraw, blank regions).
5. One PTY cannot safely track multiple volatile viewport transitions unless resize semantics are constrained and deterministic.

Conclusion:
- Keyboard transitions should be local UX events, not PTY geometry events.
- PTY resizes should be reserved for semantic geometry changes only.
- Daemon/wrapper must enforce correctness even if clients misbehave.

## 4. Architecture Decisions

### 4.1 Semantic Resize Model

Introduce and enforce resize reasons:
- `attach_init`
- `geometry_change`
- `reconnect_sync`
- `detach_restore`
- `keyboard_overlay` (local only; never forwarded to PTY)

Policy:
- `keyboard_overlay` must never produce PTY `SIGWINCH`.
- PTY resize only on `attach_init`, meaningful `geometry_change`, `reconnect_sync`, and last-viewer `detach_restore`.

### 4.2 Desktop Geometry Policy

- Keep `MOBILECLI_DESKTOP_RESIZE_POLICY=preserve` as default.
- `mirror` remains explicit opt-in legacy mode.
- No implicit host-window manipulation in default production behavior.

### 4.3 Transition Safety

Attach/detach are transactions, not ad-hoc events:
1. Subscribe and determine session mode (alt/main screen).
2. Apply one authoritative resize (with epoch).
3. Wait for `pty_resized(epoch)` confirmation.
4. Stream data normally.
5. On last detach, perform one restore transaction.

## 5. Workstreams

## A. Daemon + Wrapper (Primary)

1. Protocol hardening
- Add optional `reason` field to `pty_resize`.
- Keep backward compatibility if older clients omit reason.

2. Server-side validation
- Reject stale epochs.
- Reject no-op dimensions.
- Ignore keyboard-only resize reasons.
- Keep viewer-count-aware restore logic.

3. Resize coalescing
- Add short server/wrapper coalescing window (for rapid equivalent updates).
- Ensure only the latest meaningful resize applies.

4. Transition controller
- Implement attach/detach state handling per session.
- Guarantee exactly one restore when last viewer leaves.

5. Logging and telemetry
- Emit structured logs for request/applied/ignored resize decisions.
- Include `session_id`, `reason`, `epoch`, `cols`, `rows`, `viewer_count`, `alt_screen`, decision.

Acceptance criteria (Workstream A):
- No duplicate `master.resize` calls for keyboard-only transitions.
- Deterministic epoch progression and ack behavior.
- Restore logic cannot override active viewers.

## B. Mobile Terminal Path (Minimal but Necessary)

1. Keyboard policy
- Keep keyboard as overlay/padding UX behavior.
- Do not emit PTY resize for keyboard open/close events.

2. Resize emit policy
- Emit PTY resize only on:
- initial terminal ready
- orientation/split-screen/actual container geometry change
- reconnect sync when dimensions are unknown/stale

3. UX continuity
- Preserve scroll position behavior.
- Keep input toolbar and keyboard interactions reliable.
- Avoid duplicate local refit loops that do not represent real geometry changes.

Acceptance criteria (Workstream B):
- Keyboard toggles produce zero PTY resize requests.
- Terminal remains usable and visually correct with keyboard visible/hidden.

## C. Cross-Emulator and CLI Compatibility

1. Host terminal matrix
- Konsole, GNOME Terminal, WezTerm, Kitty, iTerm2 (where available).

2. CLI matrix
- Main-screen text CLIs: shell, Claude-like, Gemini-like.
- Alt-screen TUIs: Codex-like, `vim`, `htop`, `less`, curses samples.

3. Device matrix
- iOS and Android phones, at least one tablet profile.
- portrait/landscape and split-screen where supported.

Acceptance criteria (Workstream C):
- No critical rendering regressions in any tested matrix cell.

## D. Regression Protection and Release Discipline

1. Feature flags
- Keep ability to toggle fallback behavior if needed.
- Never remove safety toggles until matrix signoff.

2. PR structure
- Small, auditable PRs:
- PR 1: daemon/wrapper semantics + logging
- PR 2: mobile resize policy changes
- PR 3: tests and docs hardening (if needed)

3. Merge gate
- No merge unless test matrix and task log are complete.

## 6. Task Logging Protocol (Required)

Create and maintain a running log for this effort:
- File: `docs/TUI_RENDERING_TASK_LOG_2026-02-23.md`

Each task entry must include:
- Task ID
- Hypothesis
- Files touched
- Commands run
- Evidence collected (logs/screenshots)
- Result (`pass`/`fail`/`partial`)
- Next action

Template:

```md
## Task T-00X - <title>
Date:
Owner:

Hypothesis:

Changes:
- file/path: what changed

Commands:
- <command>

Evidence:
- <log snippet reference>
- <screenshot reference>

Result:
- pass | fail | partial

Next action:
```

Rule:
- No new build submission without at least one completed task-log entry proving what changed and why it should help.

## 7. Test Plan

## 7.1 Automated

- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml`
- `npx tsc --noEmit` in `mobile/`
- Add/extend daemon tests for:
- stale epoch rejection
- viewer-count restore gating
- alt-screen transition detection (split-chunk sequences)

## 7.2 Integration (Scripted/Repeatable)

Build a deterministic resize scenario runner:
- attach mobile viewer
- send controlled resize events with reasons
- simulate keyboard-only transitions
- reconnect and resend latest dimensions
- detach/restore with single and multi-viewer cases

Assertions:
- keyboard-only transitions do not call PTY resize
- latest epoch wins
- exactly one restore on last detach
- no stale ack unsuppression

## 7.3 Manual Matrix Scenarios

- keyboard show/hide repeated 20+ times while Codex prompt is active
- quick type + send during keyboard transitions
- rotate while in alt-screen
- app background/foreground loops
- reconnect during active output
- desktop-only continuation after mobile detach

Expected outcomes:
- no duplicate prompt boxes
- no blank top gaps/truncated history
- no persistent left-shifted frame after detach
- Claude/main-screen sessions unchanged or improved

## 8. Definition of Done

All of the following must be true:
- semantic resize model implemented and verified
- keyboard overlay policy active (no PTY resize from keyboard toggles)
- daemon logs prove correct routing/ignoring decisions
- compatibility matrix completed with no open critical issues
- task log fully populated for all major changes
- no regression in known-working flows

## 9. Stop Conditions (To Prevent Another 10-Build Loop)

Pause implementation and return to analysis if any of these occur:
- two consecutive builds with no measurable metric improvement,
- fix changes behavior but evidence does not match hypothesis,
- regressions appear in Claude/main-screen baseline flows.

When paused:
- collect logs, screenshots, and diff summary,
- update task log with failure analysis,
- revise hypothesis before next code change.

## 10. Immediate Execution Checklist

- [ ] Create `docs/TUI_RENDERING_TASK_LOG_2026-02-23.md`.
- [ ] Add resize reason field support in daemon protocol path.
- [ ] Enforce server-side keyboard-only resize ignore policy.
- [ ] Add daemon/wrapper structured resize decision logs.
- [ ] Update mobile resize emission rules to exclude keyboard transitions.
- [ ] Add/extend automated tests for epoch and restore logic.
- [ ] Run full CLI/emulator/device validation matrix.
- [ ] Prepare PRs with evidence-linked task log entries.
- [ ] Merge only after definition-of-done checklist is complete.
