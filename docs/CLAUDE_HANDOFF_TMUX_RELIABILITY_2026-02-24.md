# Claude Handoff: TMUX Redesign + Current Reliability State (2026-02-24)

## 1) User Intent and Non-Negotiables (from this thread)

1. Core goal:
- Mobile must behave like a reliable view/control surface for desktop terminal sessions.
- It must work for any terminal CLI, including unknown future tools (user explicitly framed 2028/new vendors).

2. Hard requirements repeatedly emphasized:
- Simultaneous desktop + mobile viewing must remain supported.
- Mobile dimensions should work on phone without physically shrinking desktop window.
- No duplicated UI artifacts, no blank reopen, no cut-off sessions.
- Full scrollability and history access from mobile.
- No speculation-only fixes; evidence-based changes only.
- Minimize unnecessary mobile changes; user is skeptical mobile-side edits are root cause.

3. Current user position:
- Progress acknowledged on framing/layout quality.
- Remaining blockers are:
  - history/scrollback being chopped or overwritten
  - occasional stuck mobile dimensions after mobile leaves
  - inconsistent scroll interaction and missing full session history

## 2) Repos, Branches, PRs

1. Desktop/daemon repo:
- Repo: `MobileCLI/mobilecli`
- Branch: `feat/tmux-runtime-phase1`
- PR: `https://github.com/MobileCLI/mobilecli/pull/16`
- Head: `0c5274d`

2. Mobile repo:
- Repo: `MobileCLI/mobile`
- Branch: `fix/tui-rendering-reliability-v3`
- PR: `https://github.com/MobileCLI/mobile/pull/17`
- Head: `67a5e9a`

## 3) High-Level Evolution Since TMUX Overhaul

1. Pre-overhaul context:
- Existing plan and bug docs captured repeated failures with PTY-only + clear/reflow heuristics.
- Important baseline docs:
  - `docs/TUI_RENDERING_BUG.md`
  - `docs/TUI_RENDERING_RELIABILITY_MASTER_PLAN_2026-02-23.md`

2. TMUX architecture decision:
- Core pivot was from PTY heuristic replay/suppression toward tmux-backed runtime semantics.
- Design docs:
  - `docs/TMUX_RUNTIME_EXECUTION_MASTER_PLAN_2026-02-24.md`
  - `docs/TMUX_RUNTIME_TASK_LOG_2026-02-24.md`

3. Implementation direction:
- Wrapper default runtime moved to tmux (`auto -> tmux when installed`) with `pty` fallback.
- Daemon replay/history paths moved toward tmux snapshot behavior (`capture-pane`).
- Mobile received targeted sync/replay/queue hardening for reconnect and large history payloads.

## 4) Desktop/Daemon Commit Timeline (Post-main)

1. Early resize/semantic hardening:
- `2464034` `fix(cli): harden semantic PTY resize flow for TUI stability`
- `bffe62f` `chore(cli): rename shell hook UX to autolaunch and drop dead helper`

2. Interim TUI fixes before tmux default:
- `4643e39` `fix(tui): force redraw on mobile reattach and clear stale frames`
- `cd70e69` `fix(tui): classify frame-rendered codex sessions as tui mode`
- `4afae32` `fix(daemon): preserve tui history across mobile reattach`

3. tmux plan publication:
- `8f89638` docs rewrite plan
- `e841b97` execution master plan

4. tmux cutover and replay stabilization:
- `201ffef` `feat(cli): switch wrapper runtime to tmux and remove clear-based resizing`
- `3fb08b0` tmux `capture-pane` reconnect history
- `c580cb3` terminal-report filtering + replay changes
- `73826a8` stateful split-sequence filtering + replay stability
- `bfebb95` keep tmux `window-size` dynamic
- `485bc8f` allow bootstrap resizes before viewer registration
- `6597236` replay gating + larger daemon scrollback + runtime-aware subscribe ack

5. Review-driven and lifecycle fixes:
- `60c4b10` tmux bootstrap test race fix
- `96bd2ab` disable tmux alternate-screen to preserve scrollback
- `2d9ba0f` detach restore uses pre-mobile baseline in wrapper preserve mode
- `0c5274d` stale mobile socket eviction via sender ID + resize semantics hardening

## 5) Mobile Commit Timeline (Post-main)

1. Resize semantic and churn suppression:
- `ac66f8b` semantic resize reasons + suppress keyboard PTY churn

2. Release metadata and Android prep:
- `a8c8f80` Android readiness + version sync
- `eb02105` build numbers to 92

3. Reattach/replay/scroll hardening:
- `14f0a2a` alt-screen history rehydrate
- `0fc9442` blank reattach and full-surface scroll hardening
- `483e90c` remove faux alt-screen injection and force post-ready refit
- `c47382e` replay chunking + canonical resize path cleanup + gesture updates

4. Latest lifecycle fixes:
- `f53e740` explicit `detach_restore` send on leave + keyboard overlay suppression
- `67a5e9a` include `sender_id` in `hello` for reconnect de-duplication

## 6) Key Code Areas Changed

1. Daemon/runtime:
- `cli/src/daemon.rs`
- `cli/src/protocol.rs`
- `cli/src/pty_wrapper.rs`
- `cli/src/link.rs`

2. Mobile:
- `mobile/hooks/useSync.ts`
- `mobile/components/TerminalView.tsx`
- `mobile/components/XTermView.tsx`
- `mobile/assets/xterm.html`
- `mobile/app/session/[id].tsx`

## 7) Evidence Gathered During This Thread

1. Compile/test evidence:
- `cargo test --manifest-path cli/Cargo.toml --bin mobilecli -- --skip test_list_directory_sorts_directories_first`
- Repeatedly green at 43/43 (1 filtered).
- Mobile `npx tsc --noEmit` repeatedly green.

2. Runtime probes:
- tmux control behavior verified (`%output`, `%layout-change`, `%exit`, etc.).
- `capture-pane` behavior validated for reconnect history.

3. Control-sequence capture evidence:
- Live captures showed frame-style redraw output (cursor reposition, screen clears, scroll-region changes) for Codex/Kimi interactive flows.
- This explains why "visible history" can appear overwritten even when transport is functioning.

4. Direct attach/unsubscribe probe:
- Simulated mobile subscribe/resize/unsubscribe to wrapper session showed non-deterministic restore symptoms under reconnect churn.
- Led to sender-id stale socket eviction fix.

## 8) Greptile Review Status

1. `mobilecli` PR #16:
- Multiple commented reviews during iterative commits.
- A tmux test race comment was fixed (`60c4b10`).
- Latest commit `0c5274d` is newer than the last greptile review event currently visible in prior checks.

2. `mobile` PR #17:
- Prior review pass reported "no comments" at one stage.
- Latest commit `67a5e9a` is newer than earlier review checkpoints.

## 9) Current Unresolved Problem Statement

1. User-reported current failures:
- Terminal history appears chopped as content scrolls.
- Mobile shows limited/no scrollbar behavior for full session depth.
- Desktop sometimes remains in mobile-like dimensions after mobile leaves.
- Header/top portions can be cut off after session transitions.

2. Why this remains difficult:
- There are two overlapping failure classes:
  - lifecycle/state bugs (viewer tracking, restore timing, stale socket state)
  - frame-redraw terminal behavior (not append-only output), which can overwrite viewport content and reduce practical scrollback utility

## 10) What Was Added Specifically to Address Lifecycle State

1. Sender ID plumbing:
- `mobile/hooks/useSync.ts` sends `sender_id` in `hello`.
- `cli/src/protocol.rs` accepts optional `sender_id`.
- `cli/src/daemon.rs` tracks sender->addr and evicts stale previous addr.

2. Explicit detach restore:
- `mobile/app/session/[id].tsx` sends `pty_resize(0,0,detach_restore)` before unsubscribe.

3. Wrapper restore hardening:
- `cli/src/pty_wrapper.rs` preserves pre-mobile baseline size in `preserve` policy restore path.
- `detach_restore` reason mapping corrected.

4. tmux no-op jitter reduction:
- Force-noop redraw jitter is now constrained to `pty` runtime only to reduce tmux churn.

## 11) Important Constraint Clarification for Claude

1. User currently does not want per-CLI launch-flag "solutions" as the primary path.
- Even if a CLI offers a no-alt-screen flag, user wants platform-level reliability, not per-tool patches.

2. User is skeptical about mobile-side changes.
- Any additional mobile edit should be justified as protocol/lifecycle necessity, not convenience.

3. User expects:
- full terminal control from mobile
- session continuity on reopen
- robust behavior without ongoing build churn

## 12) Suggested Next Debug Order (if continuing from here)

1. Confirm sender-id stale-socket eviction under real phone reconnect loops.
2. Instrument and capture one end-to-end lifecycle trace:
- focus in
- resize
- keyboard show/hide
- focus out
- unsubscribe
- restore
- focus in again

3. Separate "history overwritten by frame redraw" from "history truly missing":
- compare tmux `capture-pane -S -N` output against mobile rendered viewport for same session/time.

4. Decide architecture-level history model:
- if frame redraw CLIs are expected, keep terminal fidelity for live control but provide deterministic replay lane for readable historical transcript (generic, not CLI-specific).

## 13) Additional Planning Docs Added

1. `docs/TERMINAL_SCROLLBACK_DIMENSION_ROOT_CAUSE_PLAN_2026-02-24.md`
- Documents current root-cause framing and phased close-out plan focused on:
  - deterministic viewer state
  - lifecycle restore correctness
  - generic history reliability path

