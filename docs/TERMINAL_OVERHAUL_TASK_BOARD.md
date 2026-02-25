# Terminal Overhaul Task Board

**Generated from:** `TERMINAL_OVERHAUL_EXECUTION_MASTER_PLAN_2026-02-24.md`  
**Status:** Ready for Execution  
**Format:** Phase → Epic → Task with Owner & Estimate

---

## Legend

| Symbol | Meaning |
|--------|---------|
| 🔴 Blocker | Must complete before next phase |
| 🟡 Risk | High complexity or uncertainty |
| 🟢 Safe | Well-understood, low risk |
| ⚡ Quick | < 2 hours |
| 📊 Metrics | Has measurable outcome |

---

## Phase 0: Baseline Lock (1-2 days)

### Epic P0-1: Baseline Establishment
**Owner:** Team Lead  
**Goal:** Freeze current behavior and capture metrics

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P0-1.1 | Create benchmark harness for attach latency | Backend | 4h | 🔴 | Cargo benchmark setup |
| P0-1.2 | Create reconnect stress test (30x loop) | Backend | 3h | 🔴 | Automated script |
| P0-1.3 | Measure current duplicate frame rate | QA | 2h | 🔴 | Manual + automated |
| P0-1.4 | Measure input latency (no echo) | Mobile | 2h | 🔴 | Instrument logging |
| P0-1.5 | Document baseline metrics in docs/BASELINE.md | Team Lead | 2h | 🟢 | ⚡ |
| P0-1.6 | Set up feature flag infrastructure | Backend | 3h | 🔴 | MOBILECLI_* env vars |

**Phase 0 Exit Criteria:**
- [ ] Baseline report committed to `docs/BASELINE_2026-02-24.md`
- [ ] All feature flags can be toggled at runtime
- [ ] Stress test runs successfully against baseline

---

## Phase 1: Protocol & Attach V2 (3-4 days)

### Epic P1-1: Protocol Specification
**Owner:** Protocol Lead  
**Goal:** Define and implement v2 protocol

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P1-1.1 | Add v2 protocol types to `protocol.rs` | Backend | 4h | 🟡 | AttachBegin, AttachSnapshotChunk, etc. |
| P1-1.2 | Implement sequence number generator | Backend | 3h | 🟢 | Per-session atomic counter |
| P1-1.3 | Add protocol version negotiation | Backend | 3h | 🟡 | Backward compat shim |
| P1-1.4 | Add structured logging for protocol events | Backend | 2h | 🟢 | ⚡ |

### Epic P1-2: Server-Side Attach V2
**Owner:** Backend  
**Goal:** Implement new attach handshake in daemon

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P1-2.1 | Create `handle_subscribe_v2()` function | Backend | 6h | 🟡 | Parallel to v1 during migration |
| P1-2.2 | Implement attach state machine | Backend | 4h | 🟡 | Enum with state transitions |
| P1-2.3 | Add per-session sequence tracking | Backend | 3h | 🟢 | HashMap<session, SequenceState> |
| P1-2.4 | Implement `attach_begin` message | Backend | 2h | 🟢 | ⚡ |
| P1-2.5 | Implement `attach_ready` message | Backend | 2h | 🟢 | ⚡ |
| P1-2.6 | Add client acknowledgment handling | Backend | 3h | 🟡 | Optional for reliability |

### Epic P1-3: Mobile Protocol V2
**Owner:** Mobile  
**Goal:** Client-side v2 implementation

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P1-3.1 | Add v2 message types to TypeScript | Mobile | 3h | 🟢 | ⚡ |
| P1-3.2 | Implement client attach state machine | Mobile | 4h | 🟡 | IDLE → ATTACHING → REPLAYING → ACTIVE |
| P1-3.3 | Add sequence tracking and deduplication | Mobile | 4h | 🟡 | Ignore stale seq, buffer out-of-order |
| P1-3.4 | Add stale attach_id filtering | Mobile | 2h | 🟢 | ⚡ |
| P1-3.5 | Implement protocol version selection | Mobile | 2h | 🟢 | Use v2 if available, fallback v1 |

### Epic P1-4: Integration & Testing
**Owner:** QA + Protocol Lead  
**Goal:** Validate v2 protocol

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P1-4.1 | Write protocol conformance tests | QA | 6h | 🟢 | Unit tests for all message types |
| P1-4.2 | Run attach stress test (v2) | QA | 4h | 🟢 | 30x loops, measure improvement |
| P1-4.3 | Verify backward compatibility | QA | 2h | 🟢 | Old mobile + new daemon |
| P1-4.4 | Verify forward compatibility | QA | 2h | 🟢 | New mobile + old daemon |

**Phase 1 Exit Criteria:**
- [ ] All v2 message types implemented
- [ ] Attach stress test passes (no duplicates)
- [ ] Backward compatibility verified
- [ ] Protocol documentation updated

---

## Phase 2: Replay Rewrite (3-4 days)

### Epic P2-1: Simplify Scrollback Model
**Owner:** Backend  
**Goal:** Replace complex buffer with line-based storage

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P2-1.1 | Design LineBuffer struct | Backend | 3h | 🟡 | Ring buffer of Line structs |
| P2-1.2 | Implement LineBuffer with tests | Backend | 6h | 🟢 | Bounded memory, O(1) append |
| P2-1.3 | Replace VecDeque<u8> with LineBuffer | Backend | 4h | 🟡 | Refactor PtySession |
| P2-1.4 | Add line-based retrieval methods | Backend | 2h | 🟢 | last_n_lines(), etc. |

### Epic P2-2: tmux Snapshot Path
**Owner:** Backend  
**Goal:** Make tmux capture-pane canonical for tmux runtime

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|-------|
| P2-2.1 | Optimize tmux capture-pane command | Backend | 3h | 🟢 | Single command, timeout |
| P2-2.2 | Add tmux snapshot retry logic | Backend | 2h | 🟢 | 3 attempts with backoff |
| P2-2.3 | Remove deferred raw replay for tmux | Backend | 4h | 🔴 | DELETE: pending_tui_replay |
| P2-2.4 | Remove frame render heuristics for tmux | Backend | 3h | 🔴 | DELETE: frame_* fields |
| P2-2.5 | Implement snapshot chunking | Backend | 3h | 🟡 | For large scrollback |

### Epic P2-3: PTY Runtime Path
**Owner:** Backend  
**Goal:** Simplified replay for PTY runtime

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P2-3.1 | Implement bounded scrollback for PTY | Backend | 3h | 🟢 | Last 1000 lines |
| P2-3.2 | Simplify alt-screen detection for PTY | Backend | 2h | 🟢 | In-band only, no cross-chunk |
| P2-3.3 | Remove complex replay modes | Backend | 4h | 🔴 | Single path: clear → scrollback → live |

### Epic P2-4: Validation
**Owner:** QA  
**Goal:** Verify replay reliability

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P2-4.1 | Test attach during active output | QA | 3h | 🟢 | No duplicates, clean handoff |
| P2-4.2 | Test reconnect with 30x loops | QA | 4h | 🟢 | All pass, measure time |
| P2-4.3 | Test tmux snapshot fallback | QA | 2h | 🟡 | Kill tmux mid-capture |
| P2-4.4 | Test long scrollback (20K lines) | QA | 3h | 🟢 | Memory and time bounds |

**Phase 2 Exit Criteria:**
- [ ] 30 reconnect loops without blank/duplicated frames
- [ ] tmux snapshot is canonical path
- [ ] No deferred replay queues
- [ ] Memory usage reduced

---

## Phase 3: Resize Simplification (2-3 days)

### Epic P3-1: Remove Complex Resize Logic
**Owner:** Backend  
**Goal:** Strip jitter, forcing, and over-coordination

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P3-1.1 | Remove jitter resize target | Backend | 2h | 🔴 | DELETE: jitter_resize_target() |
| P3-1.2 | Remove force_noop_redraw path | Backend | 2h | 🔴 | DELETE: force_noop_refresh |
| P3-1.3 | Simplify resize reason handling | Backend | 3h | 🟢 | Keep semantics, strip guards |
| P3-1.4 | Remove viewer count complexity | Backend | 2h | 🟡 | Simplify to "has viewers" |
| P3-1.5 | Keep epoch for stale detection | Backend | 1h | 🟢 | ⚡ This stays |

### Epic P3-2: Wrapper Resize Cleanup
**Owner:** Backend  
**Goal:** Simplify pty_wrapper resize handling

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P3-2.1 | Remove DesktopResizePolicy | Backend | 3h | 🔴 | Or simplify significantly |
| P3-2.2 | Remove saved_local_size tracking | Backend | 2h | 🔴 | Simplify restore |
| P3-2.3 | Simplify resize to direct forward | Backend | 3h | 🟢 | Less defensive code |
| P3-2.4 | Test keyboard overlay exclusion | QA | 2h | 🟢 | Verify no PTY resize on keyboard |

### Epic P3-3: Mobile Resize Cleanup
**Owner:** Mobile  
**Goal:** Simplify resize debouncing

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P3-3.1 | Simplify resize debounce logic | Mobile | 3h | 🟢 | Single 300ms debounce |
| P3-3.2 | Remove keyboard special-casing | Mobile | 2h | 🟢 | Use semantic reason |
| P3-3.3 | Test resize matrix | QA | 4h | 🟢 | All scenarios pass |

**Phase 3 Exit Criteria:**
- [ ] Keyboard produces zero PTY resizes
- [ ] No resize storms during focus/keyboard churn
- [ ] Simpler code passes all resize scenarios

---

## Phase 4: Mobile Scroll Rewrite (2-3 days)

### Epic P4-1: Remove Gesture Layer
**Owner:** Mobile  
**Goal:** Delete custom touch handling

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P4-1.1 | Remove gesture-layer div from xterm.html | Mobile | 1h | 🔴 | ⚡ Simple deletion |
| P4-1.2 | Remove touchstart/touchmove/touchend handlers | Mobile | 2h | 🔴 | All manual pan code |
| P4-1.3 | Remove preventDefault scroll interception | Mobile | 1h | 🔴 | Let WebView handle it |
| P4-1.4 | Remove auto-follow suppression timers | Mobile | 2h | 🔴 | Simplify to boolean |
| P4-1.5 | Remove touchPan state tracking | Mobile | 1h | 🔴 | All touch-related refs |

### Epic P4-2: Native Scroll Implementation
**Owner:** Mobile  
**Goal:** Clean at-bottom detection

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P4-2.1 | Add native scroll listener (passive) | Mobile | 2h | 🟢 | on scroll event |
| P4-2.2 | Implement at-bottom detection | Mobile | 2h | 🟢 | scrollHeight math |
| P4-2.3 | Implement auto-scroll on new output | Mobile | 2h | 🟢 | Only when at bottom |
| P4-2.4 | Add scroll-to-bottom button | Mobile | 2h | 🟢 | When scrolled up |
| P4-2.5 | Preserve tap-to-focus behavior | Mobile | 2h | 🟡 | Without scroll conflict |

### Epic P4-3: Validation
**Owner:** QA  
**Goal:** Verify scroll behavior

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P4-3.1 | Test momentum scrolling (iOS) | QA | 2h | 🟢 | Smooth, no snap-back |
| P4-3.2 | Test momentum scrolling (Android) | QA | 2h | 🟢 | Same |
| P4-3.3 | Test scroll during output | QA | 2h | 🟡 | User scroll pauses auto-follow |
| P4-3.4 | Test rapid scroll up/down | QA | 2h | 🟢 | No jank |
| P4-3.5 | Test deep scrollback (10K lines) | QA | 3h | 🟢 | Performance acceptable |

**Phase 4 Exit Criteria:**
- [ ] Smooth momentum scrolling on both platforms
- [ ] No accidental snap-back while reading history
- [ ] Tap-to-focus still works
- [ ] xterm.html < 400 lines

---

## Phase 5: Local Echo (3-5 days)

### Epic P5-1: Local Echo Foundation
**Owner:** Mobile  
**Goal:** Implement optimistic echo

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P5-1.1 | Design LocalEchoController class | Mobile | 3h | 🟡 | Algorithm and data structures |
| P5-1.2 | Implement prediction queue | Mobile | 3h | 🟢 | Map<seq, string> |
| P5-1.3 | Implement safe character detection | Mobile | 2h | 🟢 | Regex for safe chars |
| P5-1.4 | Implement dimmed prediction display | Mobile | 4h | 🟡 | xterm.js styling |
| P5-1.5 | Add confirmation path | Mobile | 3h | 🟢 | Server matches prediction |
| P5-1.6 | Add correction path | Mobile | 4h | 🟡 | Backspace and rewrite |

### Epic P5-2: Server Confirmation
**Owner:** Backend  
**Goal:** Support echo confirmation in protocol

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P5-2.1 | Add seq tracking to PTY output | Backend | 2h | 🟢 | Echo input with same seq |
| P5-2.2 | Implement input/output correlation | Backend | 4h | 🟡 | Match input to PTY echo |
| P5-2.3 | Add echo confirmation message | Backend | 2h | 🟢 | Optional: explicit confirm |

### Epic P5-3: Safety & TUI Detection
**Owner:** Mobile  
**Goal:** Disable echo in problematic contexts

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P5-3.1 | Detect TUI mode from attach info | Mobile | 2h | 🟢 | Use server's alt-screen flag |
| P5-3.2 | Auto-disable echo in TUIs | Mobile | 2h | 🟢 | Safety first |
| P5-3.3 | Add manual echo toggle | Mobile | 2h | 🟢 | User can disable |
| P5-3.4 | Add correction telemetry | Mobile | 2h | 🟢 | Track misprediction rate |

### Epic P5-4: Validation
**Owner:** QA  
**Goal:** Verify echo correctness

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P5-4.1 | Measure perceived latency improvement | QA | 2h | 📊 | Target < 30ms perceived |
| P5-4.2 | Test echo in bash/zsh | QA | 3h | 🟢 | Should work well |
| P5-4.3 | Test echo in vim | QA | 2h | 🟢 | Should be disabled |
| P5-4.4 | Test echo in Codex/Claude | QA | 3h | 🟡 | Complex input handling |
| P5-4.5 | Measure correction rate | QA | 2h | 📊 | Should be < 5% |

**Phase 5 Exit Criteria:**
- [ ] Median perceived latency < 30ms
- [ ] Correction artifacts below threshold
- [ ] Auto-disabled in TUIs
- [ ] Toggleable by user

---

## Phase 6: Cleanup (2-3 days)

### Epic P6-1: Delete Obsolete Code
**Owner:** All  
**Goal:** Remove all v1 branches and flags

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P6-1.1 | Remove v1 protocol parsing | Backend | 3h | 🔴 | After v2 proven stable |
| P6-1.2 | Remove v1 replay paths | Backend | 4h | 🔴 | Delete deferred replay |
| P6-1.3 | Remove frame detection heuristics | Backend | 4h | 🔴 | frame_* fields |
| P6-1.4 | Remove cross-chunk alt tracking | Backend | 2h | 🔴 | alt_track_tail |
| P6-1.5 | Remove jitter/forcing code | Backend | 2h | 🔴 | All resize hacks |
| P6-1.6 | Remove migration flags | Backend | 2h | 🔴 | MOBILECLI_ATTACH_PROTOCOL, etc. |
| P6-1.7 | Remove v1 mobile paths | Mobile | 3h | 🔴 | Simplify useSync.ts |

### Epic P6-2: Documentation
**Owner:** Tech Writer / Team Lead  
**Goal:** Update all architecture docs

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P6-2.1 | Write PROTOCOL_V2.md | Tech Writer | 4h | 🟢 | Full message reference |
| P6-2.2 | Update ARCHITECTURE.md | Team Lead | 4h | 🟢 | New attach flow |
| P6-2.3 | Write RESIZE_BEHAVIOR.md | Backend | 2h | 🟢 | Simplified semantics |
| P6-2.4 | Write LOCAL_ECHO.md | Mobile | 3h | 🟢 | Algorithm details |
| P6-2.5 | Update TROUBLESHOOTING.md | Team Lead | 3h | 🟢 | New debug procedures |
| P6-2.6 | Write MIGRATION_GUIDE.md | Tech Writer | 3h | 🟢 | For contributors |

### Epic P6-3: Final Validation
**Owner:** QA + Team Lead  
**Goal:** Full regression suite

| ID | Task | Owner | Est. | Status | Notes |
|----|------|-------|------|--------|------|
| P6-3.1 | Run full validation matrix | QA | 8h | 🟢 | All CLI, device, scenario |
| P6-3.2 | Verify code reduction targets | Team Lead | 2h | 📊 | Compare to baseline |
| P6-3.3 | Performance regression test | QA | 4h | 📊 | All metrics must pass |
| P6-3.4 | Code review all changes | Team Lead | 4h | 🟢 | Final approval |
| P6-3.5 | Tag release v2.0.0 | Team Lead | 1h | 🟢 | ⚡ |

**Phase 6 Exit Criteria:**
- [ ] Code reduction targets achieved
- [ ] All documentation updated
- [ ] Full validation matrix passes
- [ ] No migration flags needed
- [ ] Tagged release

---

## Summary Statistics

| Metric | Value |
|--------|-------|
| **Total Tasks** | 90 |
| **Total Estimated Hours** | ~200 hours |
| **Phases** | 7 (0-6) |
| **Epics** | 22 |
| **Critical Path** | P0 → P1 → P2 → P4 → P6 |
| **Parallelizable** | P3 (resize), P5 (echo) |
| **Risky Tasks** | 18 |
| **Quick Wins** | 12 |

---

## Team Assignment Matrix

| Role | Primary Epics | Est. Hours |
|------|--------------|------------|
| Backend (2 people) | P0-1, P1-1/2, P2-1/2/3, P3-1/2, P5-2, P6-1 | ~80h |
| Mobile (2 people) | P1-3, P3-3, P4-1/2, P5-1/3 | ~70h |
| QA (1-2 people) | All validation epics | ~40h |
| Tech Writer (part-time) | P6-2 | ~15h |
| Team Lead | Coordination, review, docs | ~20h |

---

## Dependency Graph

```
Phase 0 (Baseline)
    │
    ▼
Phase 1 (Protocol V2) ───────────────────┐
    │                                      │
    ▼                                      │
Phase 2 (Replay Rewrite)                   │
    │                                      │
    ├──► Phase 3 (Resize) ◄──┐             │
    │       │                 │             │
    │       ▼                 │             │
    └──► Phase 4 (Scroll) ────┘             │
            │                               │
            ▼                               │
    Phase 5 (Local Echo) ──────────────────┘
            │                               │
            ▼                               │
    Phase 6 (Cleanup) ◄─────────────────────┘
```

**Note:** Phase 3 (Resize) and Phase 5 (Local Echo) can start once their prerequisites are met, but can also be done in parallel with later phases.

---

## Sprint Planning Recommendations

### Sprint 1 (Week 1): Baseline + Protocol
- P0-1: Baseline (all tasks)
- P1-1: Protocol specification
- P1-2: Server attach v2 (start)

### Sprint 2 (Week 2): Protocol + Replay
- P1-2: Server attach v2 (finish)
- P1-3: Mobile protocol v2
- P1-4: Protocol validation
- P2-1: LineBuffer (start)

### Sprint 3 (Week 3): Replay + Scroll
- P2-1: LineBuffer (finish)
- P2-2: tmux snapshot path
- P2-3: PTY replay path
- P2-4: Replay validation
- P4-1: Remove gesture layer (start)

### Sprint 4 (Week 4): Scroll + Resize + Echo
- P4-1: Remove gesture layer (finish)
- P4-2: Native scroll
- P4-3: Scroll validation
- P3-1: Resize cleanup (if time permits)
- P5-1: Local echo foundation (if time permits)

### Sprint 5 (Week 5): Echo + Cleanup
- P3-1/2/3: Resize cleanup (if not done)
- P5-1: Local echo (finish)
- P5-2: Server confirmation
- P5-3: Safety & TUI
- P5-4: Echo validation
- P6-1: Delete obsolete code (start)

### Sprint 6 (Week 6): Cleanup + Release
- P6-1: Delete obsolete code (finish)
- P6-2: Documentation
- P6-3: Final validation
- Release v2.0.0

---

## Definition of Done (Per Task)

Each task is done when:
1. Code implemented and tested
2. Unit tests pass (where applicable)
3. No regressions in validation matrix
4. Code reviewed
5. Documentation updated (if user-facing)

---

*Task board generated from master plan. Update estimates and status as work progresses.*
