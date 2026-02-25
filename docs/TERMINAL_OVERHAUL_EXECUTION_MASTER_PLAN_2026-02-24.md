# Terminal Overhaul Execution Master Plan (No-Band-Aids)

Date: 2026-02-24
Status: Ready for execution
Owner: Codex + MobileCLI team
Scope: `cli/` + `mobile/` + websocket protocol + runtime model
Supersedes:
- `CODEBASE_ANALYSIS.md` (strategy sections)
- `docs/TUI_RENDERING_RELIABILITY_MASTER_PLAN_2026-02-23.md`
- `docs/TERMINAL_VIEWPORT_REFACTOR_MASTER_PLAN_2026-02-24.md`

## 1. Mandate

We are no longer optimizing the current terminal pipeline with incremental patches.

We will remove and replace unstable components that cause:
- broken mobile scrolling behavior,
- duplicate or stale terminal output on subscribe/reconnect,
- perceived input lag,
- resize-induced rendering churn,
- excessive state coupling between daemon, wrapper, and mobile client.

This is an overhaul plan, not a stabilization patch list.

## 2. Non-Negotiable Outcomes

1. Attach/reconnect is deterministic: `clear -> snapshot -> live stream`, every time.
2. No duplicated output caused by replay/live overlap.
3. Mobile scrolling uses native behavior only; no custom pan physics layer.
4. Resize path is reduced to essential semantics and is auditable.
5. Typing latency perception is materially improved with local echo.
6. Terminal architecture is understandable and maintainable without fragile heuristics.

## 3. Current Root Problems

1. Rendering/state inference is over-owned in daemon:
- alt-screen tracking across chunk boundaries,
- frame-render heuristics,
- deferred replay queues,
- multiple replay sources.

2. Resize logic is distributed and defensive in too many places:
- daemon reason/epoch/viewer/restore guards,
- wrapper no-op force redraw and jitter resize,
- mobile keyboard/geometry special-casing.

3. Mobile web terminal has custom gesture plumbing that fights native behavior:
- manual touchmove/pan/scrollTop,
- preventDefault-based scroll interception,
- auto-follow suppression timers intertwined with touch state.

4. Replay model is multi-source and race-prone:
- direct `pty_bytes` replay,
- `session_history` replay fallback,
- deferred replay after `pty_resized`,
- live bytes arriving while replay decisions are in-flight.

5. UX latency:
- current input rendering is round-trip dependent,
- no predictive/local echo path for immediate feedback.

## 4. Architectural Direction (Target State)

## 4.1 Session Authority

Use runtime authority explicitly:
- `tmux` runtime: tmux is source of snapshot truth.
- `pty` runtime: daemon owns bounded byte history only.

No implicit blending of snapshot and deferred raw replay for tmux sessions.

## 4.2 Attach Handshake v2

Every subscribe creates a new `attach_id` (monotonic per session-viewer pair).

Server flow:
1. Send `attach_begin { session_id, attach_id, mode }`.
2. Send `clear`.
3. Send snapshot stream tied to `attach_id`.
4. Send `attach_ready { session_id, attach_id, last_seq }`.
5. Send live bytes with monotonic `seq` and same `attach_id`.

Client flow:
1. Ignore data for stale `attach_id`.
2. Ignore `seq <= last_seq_seen` for active attach.
3. Render only active attach stream.

This removes replay/live overlap ambiguity.

## 4.3 Output Model

Introduce sequence numbers for stream messages:
- `seq` is strictly increasing per session.
- all live output and snapshot chunks are sequence-addressable.

Minimal server messages required:
- `attach_begin`
- `attach_clear`
- `attach_snapshot_chunk`
- `attach_ready`
- `pty_chunk` (live)
- `pty_resized`

## 4.4 Resize Model

Keep semantic reasons but simplify behavior:
- `attach_init`
- `geometry_change`
- `reconnect_sync`
- `detach_restore`
- `keyboard_overlay`

Hard rules:
1. `keyboard_overlay` never reaches PTY.
2. `detach_restore` only applies when last viewer leaves.
3. Remove jitter/no-op redraw forcing by default.
4. Keep epoch validation only for stale-ack rejection.

## 4.5 Mobile Rendering Model

Short-term:
- keep WebView+xterm as rendering engine,
- remove custom gesture layer and manual pan logic,
- rely on native scroll behavior of viewport.

Medium-term:
- run a parallel native-terminal feasibility track,
- switch only if acceptance criteria are met (Section 15).

## 5. Explicit Deletion List (Rip-Off Phase)

Delete or deprecate these patterns unless a measurable blocker appears:

Daemon:
- frame-render heuristic counters and threshold logic.
- cross-chunk alt-screen tracking for tmux runtime decisions.
- deferred replay queues tied to `pty_resized`.
- overlapping replay branches for same subscribe path.

Wrapper:
- jitter resize target path.
- forced no-op redraw resize path by reason.
- non-essential legacy branches around resize refresh.

Mobile xterm HTML:
- gesture overlay div.
- touchstart/touchmove/touchend manual pan handlers.
- preventDefault scroll interception for terminal surface.
- auto-follow suppression timers coupled to touch pan state.

Mobile sync layer:
- replay fallback logic that can race against live stream after attach v2.
- stale suppress/unsuppress branches that become obsolete under attach+seq protocol.

## 6. Code Reduction Targets

Targets are estimates for post-overhaul baseline:

| Component | Current | Target |
| --- | ---: | ---: |
| `cli/src/daemon.rs` | ~4216 | ~2200 |
| `cli/src/pty_wrapper.rs` | ~1107 | ~700 |
| `mobile/assets/xterm.html` | ~700 | ~380 |
| `mobile/components/TerminalView.tsx` | ~897 | ~620 |
| `mobile/hooks/useSync.ts` | large multi-state path | reduce replay state surface by 40%+ |

## 7. Workstreams

## WS-A Protocol + Stream Ordering

Deliverables:
- protocol additions for `attach_id` and `seq`,
- server-side monotonic sequence generator per session,
- client-side stale attach and duplicate sequence drop logic.

Acceptance:
- replay/live duplicate incidents = 0 across stress loop (Section 12).

## WS-B Subscribe/Replay Rewrite

Deliverables:
- single attach pipeline per runtime,
- tmux snapshot-first path as canonical for tmux,
- no deferred raw replay for tmux.

Acceptance:
- 30 reconnect loops without blank/duplicated frames.

## WS-C Resize Simplification

Deliverables:
- strip jitter/no-op redraw forcing,
- keep only semantic forwarding + epoch stale checks + last-viewer restore logic.

Acceptance:
- keyboard transitions produce zero PTY resizes,
- no resize storms during focus/keyboard/orientation churn.

## WS-D Mobile Interaction Rewrite

Deliverables:
- remove gesture layer and touch pan implementation,
- native scroll with clean at-bottom state reporting,
- preserve tap-to-focus behavior without scroll conflicts.

Acceptance:
- smooth momentum scrolling on iOS and Android,
- no accidental snap-back while user is reading history.

## WS-E Local Echo

Deliverables:
- local echo pipeline for text input,
- server confirmation/correction path,
- safety gating for complex raw escape sequences.

Acceptance:
- median perceived key feedback < 30ms,
- correction artifacts below agreed threshold in CLI matrix.

## WS-F Observability + Tooling

Deliverables:
- structured logs around attach, seq, snapshot source, resize decisions,
- command-level debug toggles for tracing problematic sessions.

Acceptance:
- every failure case in matrix is diagnosable from logs without ad-hoc repro.

## 8. Phase Plan and Timeline

## Phase 0 - Baseline Lock (1-2 days)

Tasks:
- freeze current behavior with reproducible scripts,
- capture baseline metrics for duplicate rate, reconnect reliability, input latency.

Exit criteria:
- baseline report committed.

## Phase 1 - Protocol and Attach v2 (3-4 days)

Tasks:
- add `attach_id` and `seq`,
- update daemon+mobile parser/renderer paths,
- keep temporary backward compatibility shim if necessary.

Exit criteria:
- all attaches use v2 path in development.

## Phase 2 - Replay Rewrite and tmux Canonical Snapshot (3-4 days)

Tasks:
- implement single snapshot pipeline,
- remove tmux deferred raw replay.

Exit criteria:
- reconnect stress loops pass.

## Phase 3 - Resize Path Reduction (2-3 days)

Tasks:
- delete jitter/no-op redraw code,
- reduce resize logic to semantic forward rules.

Exit criteria:
- resize matrix passes with lower event count and no regressions.

## Phase 4 - Mobile Scroll/Touch Rewrite (2-3 days)

Tasks:
- remove gesture layer and custom pan,
- validate native scroll behavior on devices.

Exit criteria:
- manual QA passes scroll matrix.

## Phase 5 - Local Echo Implementation (3-5 days)

Tasks:
- build optimistic echo pipeline with correction,
- add runtime guards and fallback toggle.

Exit criteria:
- latency and correctness KPIs pass.

## Phase 6 - Cleanup and Hard Delete (2-3 days)

Tasks:
- remove obsolete flags and dead code,
- update docs and architecture map.

Exit criteria:
- code reduction targets materially achieved.

## 9. File-Level Execution Map

Primary files expected to change:
- `cli/src/protocol.rs`
- `cli/src/daemon.rs`
- `cli/src/pty_wrapper.rs`
- `mobile/hooks/useSync.ts`
- `mobile/components/TerminalView.tsx`
- `mobile/components/XTermView.tsx`
- `mobile/assets/xterm.html`
- `docs/ARCHITECTURE_QUICK_REFERENCE.md` (post-overhaul update)

## 10. Migration and Compatibility Strategy

1. Introduce protocol v2 fields as optional first.
2. Switch mobile to consume v2 when available.
3. Once both sides are deployed and validated, remove v1 replay paths.
4. Delete compatibility branches in cleanup phase.

## 11. Feature Flags and Safety Switches

Temporary flags allowed during migration only:
- `MOBILECLI_ATTACH_PROTOCOL=v1|v2`
- `MOBILECLI_ENABLE_LOCAL_ECHO=0|1`
- `MOBILECLI_RESIZE_SIMPLIFIED=0|1`
- `MOBILECLI_TMUX_SNAPSHOT_ONLY=0|1`

Rule:
- no permanent operational dependence on migration flags after Phase 6.

## 12. Validation Matrix

CLI matrix:
- Codex
- OpenCode
- Claude
- Gemini
- shell/text (`bash`, `zsh`, `tail -f`)
- curses apps (`vim`, `htop`, `less`)

Device matrix:
- iPhone portrait/landscape
- Android portrait/landscape
- tablet split-screen

Scenario matrix:
1. fresh attach
2. attach during active output
3. disconnect/reconnect loops (30x)
4. background/foreground loops (30x)
5. keyboard open/close spam (50x)
6. orientation flips (20x)
7. dual-viewer conflict cases
8. long-running output with deep scrollback

Pass criteria:
- no duplicate prompt frames
- no blank snapshot attach
- no clipped viewport after reconnect
- no unexpected desktop geometry mutation in preserve mode
- input latency KPI pass with local echo enabled

## 13. Metrics (Tracked Per Build)

1. Duplicate-frame incident count (target: 0).
2. Blank-attach rate (target: 0% over 30 loops).
3. PTY resize events per 10 minutes during keyboard usage (target: near 0 for keyboard-only interactions).
4. Median key-to-visual latency (target: <30ms with local echo, <120ms without).
5. Client memory growth during sustained output (bounded).
6. Reconnect completion time to first stable frame.

## 14. Testing and Tooling Requirements

Automated:
- `cargo check --manifest-path cli/Cargo.toml`
- `cargo test --manifest-path cli/Cargo.toml`
- `npm test` where applicable
- `npx tsc --noEmit` in mobile

Integration:
- scripted reconnect loop harness,
- scripted resize reason harness,
- protocol conformance tests for `attach_id` and `seq`.

Manual:
- device QA with recorded evidence (screenshots/video/log traces).

## 15. xterm vs Native Terminal Decision Gate

This plan does not assume xterm forever.

Parallel spike criteria for native component migration:
1. supports ANSI parity for core CLI usage,
2. supports stable selection/copy/paste UX,
3. supports scrollback and performance requirements,
4. supports iOS and Android with acceptable maintenance overhead.

Decision point:
- At end of Phase 4, run formal go/no-go for replacing WebView+xterm.

If go:
- create Phase 7 native migration plan and execute.

If no-go:
- continue on hardened xterm path with reduced complexity baseline.

## 16. Risk Register

1. Protocol migration mismatch:
- Mitigation: optional fields first, strict logging, staged rollout.

2. Local echo correctness drift in complex TUIs:
- Mitigation: scoped enablement, quick disable flag, correction telemetry.

3. tmux snapshot edge cases:
- Mitigation: retry/backoff, fallback only under explicit guard, add test fixtures.

4. Mobile scroll regressions after gesture removal:
- Mitigation: device QA gates before merge.

5. Hidden dependencies on deleted heuristics:
- Mitigation: phase-based deletion with diagnostics before hard remove.

## 17. Definition of Done

All conditions must be true:
1. attach v2 + sequence ordering is the default and validated.
2. replay duplication class of bugs is eliminated in matrix runs.
3. resize path is simplified with keyboard overlay excluded from PTY.
4. custom touch pan layer is gone.
5. local echo is implemented, measurable, and controllable.
6. obsolete replay/resize heuristics are deleted.
7. architecture and runbooks are updated to match final system.

## 18. Immediate Execution Order

1. Start WS-A and WS-B together (protocol + replay rewrite).
2. Start WS-C once attach v2 is functional.
3. Execute WS-D immediately after replay path stabilizes.
4. Implement WS-E local echo after stream ordering is stable.
5. Run WS-F and matrix gates continuously; no blind build submissions.

## 19. Working Rules During Overhaul

1. No speculative fixes without a hypothesis and measurable expected effect.
2. No merge to main without matrix evidence for affected scenarios.
3. No preserving dead branches "just in case" after a phase exits green.
4. Every bug found must map to a failed gate, then a new explicit test.

## 20. Success Statement

When complete, MobileCLI terminal behavior is deterministic under attach/reconnect/resize stress, feels responsive for typing, and is maintainable without layered heuristics that fight tmux, xterm, or mobile platform behavior.
