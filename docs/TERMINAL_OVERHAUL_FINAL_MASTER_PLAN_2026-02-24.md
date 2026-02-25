# Terminal Overhaul Final Master Plan

Date: 2026-02-24  
Status: Final, canonical execution plan  
Owner: MobileCLI team (backend + mobile + QA)  
Primary scope: `cli/`, `mobile/`, websocket protocol, test infrastructure

Supersedes:
- `docs/TERMINAL_OVERHAUL_EXECUTION_MASTER_PLAN_2026-02-24.md`
- `docs/TERMINAL_OVERHAUL_REVIEW_AND_ADDENDUM.md`
- `docs/TERMINAL_OVERHAUL_TASK_BOARD.md`

---

## 1. Mission

Replace the current fragile terminal streaming pipeline with a deterministic, observable, and maintainable architecture.

This is a full overhaul. We are not preserving problematic behavior for compatibility unless explicitly required for controlled migration.

---

## 2. Non-Negotiable Outcomes

1. Attach/reconnect is deterministic and race-free.
2. No replay/live duplication artifacts.
3. Mobile scrolling is native and smooth, without custom pan interception.
4. Keyboard transitions do not drive PTY geometry churn.
5. Input feels responsive; local echo reduces perceived lag.
6. Terminal stack complexity is materially reduced and auditable.

---

## 3. Root-Cause Summary

1. Too much inferred terminal state in daemon (`frame_*`, alt tracking tails, deferred replay maps).
2. Replay comes from multiple paths and can overlap with live stream.
3. Resize semantics are over-distributed across daemon, wrapper, and mobile.
4. Custom touch pan logic in WebView fights native scrolling behavior.
5. No local echo, so user-perceived input latency stays round-trip bound.

---

## 4. Final Architecture Decisions

## 4.1 Runtime Authority

- `tmux` runtime: tmux snapshot (`capture-pane`) is authoritative for attach snapshot.
- `pty` runtime: daemon keeps bounded in-memory scrollback for snapshot.
- No tmux deferred raw replay path after attach.

## 4.2 Attach Protocol v2

Attach is explicit and versioned. Every subscribe creates a new `attach_id`.

Server sequence:
1. `attach_begin`
2. `attach_clear`
3. `attach_snapshot_chunk` (one or more)
4. `attach_ready`
5. live `pty_chunk` stream

Client rules:
1. Drop stale `attach_id`.
2. Drop `seq <= last_seq_seen`.
3. Apply only active attach stream.
4. Out-of-order live chunks are buffered briefly and emitted in order.

## 4.3 Sequence Model

- Use `u64` per-session monotonic `seq`.
- `seq` covers live stream events.
- Snapshot chunks have per-attach chunk sequence (`chunk_seq`) plus attach boundary.
- No retransmit protocol in initial cut; WebSocket/TCP ordering is sufficient for first release.
- Ack/resend remains optional future enhancement, not Day 1 scope.

## 4.4 Resize Model (Simplified)

Reasons retained:
- `attach_init`
- `geometry_change`
- `reconnect_sync`
- `detach_restore`
- `keyboard_overlay`

Rules:
1. `keyboard_overlay` never forwarded to PTY.
2. `detach_restore` only when last viewer leaves.
3. Keep epoch stale detection.
4. Remove jitter no-op redraw forcing.

## 4.5 Mobile Rendering Policy

Short-term target:
- Keep xterm.js + WebView.
- Remove gesture overlay and manual touch pan implementation.
- Use native viewport scrolling and simple at-bottom tracking.

Long-term track:
- run native-terminal feasibility spike in parallel after core stabilization.

## 4.6 Local Echo Policy

- Enable predictive local echo for safe text input paths.
- Auto-disable in high-risk TUI contexts.
- Provide runtime kill switch.
- Measure correction rate and disable if out of bounds.

---

## 5. Protocol v2 Specification (Concrete)

Server -> Client:

```rust
pub struct AttachBegin {
    pub session_id: String,
    pub attach_id: u64,
    pub runtime: String, // "tmux" | "pty"
    pub mode: String,    // "fresh" | "reconnect"
}

pub struct AttachClear {
    pub session_id: String,
    pub attach_id: u64,
}

pub struct AttachSnapshotChunk {
    pub session_id: String,
    pub attach_id: u64,
    pub chunk_seq: u32,
    pub total_chunks: u32,
    pub is_last: bool,
    pub data: String, // base64
}

pub struct AttachReady {
    pub session_id: String,
    pub attach_id: u64,
    pub last_live_seq: u64,
    pub cols: u16,
    pub rows: u16,
}

pub struct PtyChunk {
    pub session_id: String,
    pub attach_id: u64,
    pub seq: u64,
    pub data: String, // base64
    pub timestamp_ms: u64,
}
```

Client -> Server:

```rust
pub struct SubscribeV2 {
    pub session_id: String,
    pub last_seen_seq: Option<u64>,
    pub client_capabilities: u32,
}
```

Compatibility:
- v1 and v2 coexist during migration.
- server prefers v2 when client capability is present.

---

## 6. Attach State Machines

## 6.1 Server

```text
Subscribed
  -> AttachBeginSent
  -> ClearSent
  -> SnapshotStreaming
  -> AttachReadySent
  -> LiveStreaming
  -> Detached
```

Constraints:
- live forwarding for that viewer starts only after `attach_ready`.
- stale attach for same viewer is canceled atomically.

## 6.2 Client

```text
IDLE -> ATTACHING -> REPLAYING -> ACTIVE -> DETACHED
```

Rules:
- entering ATTACHING invalidates previous attach context.
- in REPLAYING, only snapshot chunks for active attach are rendered.
- in ACTIVE, duplicate and stale sequence chunks are dropped.

---

## 7. Deletion Map (Rip-Off Band-Aids)

Delete or retire in this order:

1. `mobile/assets/xterm.html`
- gesture layer div.
- touchstart/touchmove/touchend custom pan handlers.
- manual `scrollTop` manipulation for drag panning.
- pan-linked `preventDefault` interception.

2. `cli/src/pty_wrapper.rs`
- `jitter_resize_target()`
- `force_noop_refresh` redraw forcing path.
- non-essential resize workaround branches.

3. `cli/src/daemon.rs`
- deferred replay queue machinery tied to `pty_resized`.
- tmux-specific frame-render replay branches once v2 attach is stable.
- obsolete replay path branching after v2 is default.

4. `mobile/hooks/useSync.ts`
- v1 replay fallback state once v2 proves stable.
- suppression complexity that is replaced by attach/seq ordering.

Note:
- Keep `DesktopResizePolicy` compatibility until late cleanup, then decide by telemetry.

---

## 8. Workstreams

### WS-AB: Protocol + Replay Rewrite (Merged)
- Implement v2 messages and state machines.
- Implement attach_id + sequence ordering.
- Implement snapshot-first attach.
- Remove tmux deferred raw replay.

### WS-C: Resize Simplification
- Remove jitter/no-op forcing.
- Keep semantic reasons + epoch stale guard + last-viewer restore.

### WS-D: Mobile Scroll Rewrite
- Remove gesture layer and manual touch pan code.
- Implement native scroll state handling.

### WS-E: Local Echo
- Implement safe prediction path.
- Integrate confirmation/correction.
- Add telemetry and auto-disable policy.

### WS-F: Observability + Harness
- Structured logging for attach/seq/replay/resize decisions.
- Baseline + stress benchmark harness.

---

## 9. Phase Plan (Execution Sequence)

## Phase 0: Baseline + Guardrails (1-2 days)

Goals:
- establish baseline metrics,
- introduce feature flags,
- add observability early.

Required tasks:
- benchmark harness for attach/reconnect/latency.
- structured logs (`attach_id`, `seq`, replay source, resize decisions).
- feature flag plumbing.

Exit criteria:
- baseline report committed.
- harness runs in CI.
- rollback flags verified.

## Phase 1: WS-AB Protocol and Attach v2 Skeleton (3-4 days)

Goals:
- protocol types + negotiation,
- daemon and mobile state machines wired,
- v2 path operational behind flag.

Exit criteria:
- v2 attach works end-to-end in dev.
- v1 fallback still functional.

## Phase 2: WS-AB Replay Canonicalization (3-4 days)

Goals:
- snapshot-first attach for both runtimes,
- tmux deferred replay removed,
- deterministic handoff to live stream.

Exit criteria:
- 30 reconnect loop test: zero duplicate/blank incidents.

## Phase 3: WS-D Scroll Rewrite (2-3 days)

Goals:
- remove manual pan layer,
- preserve focus/tap behavior and smooth scroll.

Exit criteria:
- iOS + Android scroll matrix pass.

## Phase 4: WS-C Resize Simplification (2-3 days)

Goals:
- remove resize workarounds,
- enforce simplified semantic path.

Exit criteria:
- keyboard transitions generate zero PTY resizes.
- resize storm metrics improve materially.

## Phase 5: WS-E Local Echo (3-5 days)

Goals:
- safe local echo with correction logic and telemetry.

Exit criteria:
- perceived latency KPI met.
- correction rate within threshold.

## Phase 6: Hard Cleanup and Documentation (2-3 days)

Goals:
- remove obsolete v1 and workaround branches,
- finalize docs,
- full regression validation.

Exit criteria:
- cleanup complete,
- final gates all pass,
- release tag prepared.

Parallelization rule:
- Phase 4 and Phase 5 can overlap after Phase 2 is stable.

---

## 10. Integrated Task Matrix (Condensed)

### Phase 0
- P0-1.1 benchmark harness (Backend, 4h)
- P0-1.2 reconnect stress script (Backend, 3h)
- P0-1.3 baseline duplicate rate measurement (QA, 2h)
- P0-1.4 baseline input latency instrumentation (Mobile, 2h)
- P0-1.5 feature flags setup (Backend, 3h)
- P0-1.6 observability schema setup (Backend, 2h)

### Phase 1-2 (WS-AB)
- add v2 protocol structs and negotiation (Backend, 7h)
- implement daemon attach v2 path (Backend, 12h)
- implement mobile attach/seq state machine (Mobile, 11h)
- implement snapshot chunking and live handoff (Backend, 7h)
- conformance tests and compatibility tests (QA, 10h)

### Phase 3 (WS-D)
- delete gesture layer + pan handlers (Mobile, 4h)
- implement native scroll observer and at-bottom handling (Mobile, 6h)
- focus/tap regression fixes (Mobile, 3h)
- device scroll validation suite (QA, 8h)

### Phase 4 (WS-C)
- delete jitter and no-op resize forcing (Backend, 4h)
- simplify daemon resize guards (Backend, 5h)
- simplify wrapper resize path (Backend, 6h)
- resize matrix validation (QA, 6h)

### Phase 5 (WS-E)
- local echo controller implementation (Mobile, 12h)
- safe-input gating and TUI disable policy (Mobile, 5h)
- optional server-side echo correlation markers (Backend, 4h)
- echo validation and telemetry checks (QA, 8h)

### Phase 6
- remove obsolete v1/workaround branches (Backend+Mobile, 14h)
- update protocol and architecture docs (Team, 10h)
- full matrix + release validation (QA, 12h)

Total estimate:
- approximately 180-210 hours depending on local echo complexity and regression fixes.

---

## 11. Test and Validation Infrastructure

## 11.1 Automated Harness

Add integration tests for:
- attach/detach loop (30x minimum).
- attach during continuous output.
- sequence dedupe and stale attach rejection.
- resize reason behavior (keyboard overlay ignored).

## 11.2 Protocol Conformance

Client tests:
- stale `attach_id` rejection.
- duplicate `seq` drop.
- out-of-order buffering behavior.

Server tests:
- attach sequence order.
- snapshot completion before live forwarding for a viewer.

## 11.3 Device Matrix

Mandatory matrix:
- iPhone portrait/landscape.
- Android portrait/landscape.
- tablet split-screen.

Scenarios:
- keyboard spam,
- orientation churn,
- app background/foreground loops,
- reconnect loops under output load.

---

## 12. Metrics and Targets

Baseline captured in Phase 0, then tracked per phase:

1. Attach-to-first-stable-frame latency (P95 target: <300ms typical LAN).
2. Reconnect complete latency (P95 target: <500ms).
3. Duplicate frame incident rate (target: 0 in stress loops).
4. Blank attach rate (target: 0 in 30-loop run).
5. Keyboard-only PTY resize count (target: 0).
6. Perceived key feedback latency (target: <30ms with local echo enabled).
7. Memory growth under sustained output (bounded; no unbounded queue growth).

---

## 13. Feature Flags and Rollback

Temporary migration flags:
- `MOBILECLI_ATTACH_PROTOCOL=v1|v2`
- `MOBILECLI_ENABLE_LOCAL_ECHO=0|1`
- `MOBILECLI_RESIZE_SIMPLIFIED=0|1`
- `MOBILECLI_TMUX_SNAPSHOT_ONLY=0|1`

Emergency rollback order:

1. Force protocol v1:
```bash
export MOBILECLI_ATTACH_PROTOCOL=v1
```

2. Disable simplified resize:
```bash
export MOBILECLI_RESIZE_SIMPLIFIED=0
```

3. Disable local echo:
```bash
export MOBILECLI_ENABLE_LOCAL_ECHO=0
```

4. If mobile scroll regression persists, revert `mobile/assets/xterm.html` scroll rewrite commit.

Compatibility policy:
- no on-disk session format changes in overhaul phases.

---

## 14. Risk Register

1. tmux snapshot latency spikes.
- Mitigation: timeout + bounded retry + explicit fallback logging.

2. sequence handling bugs under reconnect churn.
- Mitigation: conformance tests + stress harness before merge.

3. local echo misprediction in TUI workflows.
- Mitigation: safe-input gating + auto-disable + telemetry threshold rollback.

4. mobile buffer growth under delayed callback attachment.
- Mitigation: bounded queues and drop policy with warnings.

5. regressions from broad deletion.
- Mitigation: phase gates and branch-level rollback points.

---

## 15. Code Reduction Targets

Post-overhaul goals:
- `cli/src/daemon.rs` below ~2500 lines.
- `cli/src/pty_wrapper.rs` below ~750 lines.
- `mobile/assets/xterm.html` below ~400 lines.
- `mobile/hooks/useSync.ts` replay-state surface reduced by at least 40%.

---

## 16. Team and Sprint Model

Recommended staffing:
- Backend: 2 engineers.
- Mobile: 2 engineers.
- QA: 1-2 engineers.

6-sprint outline:
1. Sprint 1: Phase 0 + Phase 1 start.
2. Sprint 2: Phase 1 finish + Phase 2 start.
3. Sprint 3: Phase 2 finish + Phase 3 start.
4. Sprint 4: Phase 3 finish + Phase 4 start.
5. Sprint 5: Phase 4 finish + Phase 5.
6. Sprint 6: Phase 6 + release validation.

---

## 17. Documentation Deliverables

Must exist by Phase 6 completion:
- `docs/PROTOCOL_V2.md`
- `docs/ATTACH_SEQUENCE.md`
- `docs/RESIZE_BEHAVIOR.md`
- `docs/LOCAL_ECHO.md`
- `docs/TROUBLESHOOTING.md`
- updated architecture reference docs.

---

## 18. Definition of Done

All must be true:
1. v2 attach/sequence path is default.
2. duplicate and blank attach classes are eliminated in validation matrix.
3. custom touch pan layer removed.
4. resize path simplified and keyboard overlays excluded from PTY.
5. local echo shipped with guardrails and KPI pass.
6. obsolete workaround branches removed.
7. documentation updated and accurate.

---

## 19. Immediate Next Actions (48 Hours)

1. Approve this final plan as canonical.
2. Start Phase 0 tasks:
- benchmark harness,
- observability schema,
- feature flag plumbing.
3. Open implementation PRs:
- PR-A: Phase 0 tooling/logging.
- PR-B: protocol v2 scaffolding.
4. Schedule daily triage on metrics dashboards during phases 1-2.

---

## 20. Operating Rules During Execution

1. No speculative fixes without explicit hypothesis and expected metric change.
2. No merge without phase exit criteria evidence.
3. No long-lived fallback branches after final cleanup phase.
4. Every incident must map to a missing or failing test gate.

---

## 21. Final Statement

This document is the single source of truth for the MobileCLI terminal overhaul.  
Any implementation change must trace back to a phase, task, and acceptance gate defined here.
