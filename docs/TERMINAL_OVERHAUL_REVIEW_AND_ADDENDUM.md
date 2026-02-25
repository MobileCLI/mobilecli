# Terminal Overhaul Master Plan - Review & Addendum

**Review Date:** 2026-02-24  
**Original Plan:** `TERMINAL_OVERHAUL_EXECUTION_MASTER_PLAN_2026-02-24.md`  
**Status:** Approved with additions

---

## Executive Summary

The master plan is comprehensive and well-structured. This addendum provides:

1. **Additional research** on sequence-numbered protocols from production systems
2. **Protocol specification details** for the attach v2 handshake
3. **Testing infrastructure recommendations** for the validation matrix
4. **Rollback strategy** for risk mitigation
5. **Documentation requirements** for post-overhaul maintenance
6. **Dependency mapping** to inform deletion order

---

## 1. Additional Research: Sequence-Numbered Stream Protocols

### 1.1 How Production Systems Handle Ordered Streams

**Mosh SSP (State Synchronization Protocol)**
```
Packet Structure:
- Sequence number (64-bit): Strictly monotonic per direction
- Acknowledgment number (64-bit): Last received in-order seq
- Payload: Complete screen state or delta

Key Insight: SSP sends the *entire visible state* on every update,
not incremental diffs. This eliminates replay/live overlap entirely.

Trade-off: Higher bandwidth (~10KB per frame) but absolute consistency.
```

**tmux Control Mode %extended-output**
```
Format: %extended-output <pane-id> <age> : <data>

The <age> field is a generation counter per pane.
Clients can detect gaps and request retransmission.

Flow control: Server pauses when buffer > 8192 bytes per client.
```

**Quake 3 Network Protocol** (Surprisingly relevant)
```
- Sequence numbers on every packet
- Client acknowledges last received
- Server resends unacknowledged sequences
- Client interpolates between states for smoothness

Lesson: For terminal streaming, we don't need interpolation,
but the ack/resend pattern is useful for reliability.
```

### 1.2 Recommended Protocol Design for MobileCLI

Based on research, here's a concrete specification:

```rust
// Protocol v2 message types

// Server -> Client
pub struct AttachBegin {
    pub session_id: String,
    pub attach_id: u64,        // Monotonic per session
    pub runtime: RuntimeType,  // Tmux | Pty
    pub mode: AttachMode,      // Fresh | Reconnect
}

pub struct AttachSnapshotChunk {
    pub attach_id: u64,
    pub chunk_seq: u32,        // Sequence within this snapshot
    pub total_chunks: u32,
    pub data: String,          // base64 encoded
    pub is_last: bool,
}

pub struct AttachReady {
    pub session_id: String,
    pub attach_id: u64,
    pub last_live_seq: u64,    // Live stream starts here + 1
    pub cols: u16,
    pub rows: u16,
}

pub struct PtyChunk {
    pub session_id: String,
    pub attach_id: u64,
    pub seq: u64,              // Global session sequence
    pub data: String,          // base64 encoded
    pub timestamp_ms: u64,     // For latency measurement
}

pub struct PtyResized {
    pub session_id: String,
    pub attach_id: u64,
    pub cols: u16,
    pub rows: u16,
    pub reason: ResizeReason,
    pub epoch: u64,            // Keep for stale detection
}

// Client -> Server
pub struct SubscribeV2 {
    pub session_id: String,
    pub last_seen_seq: Option<u64>,  // For reconnection hint
    pub client_capabilities: u32,     // Feature flags
}

pub struct AckChunk {
    pub attach_id: u64,
    pub up_to_seq: u64,        // Acknowledge receipt through this seq
}
```

### 1.3 Sequence Number Management

```rust
// Daemon-side sequence generator
pub struct SessionSequence {
    current: AtomicU64,
    per_attach: DashMap<u64, u64>,  // attach_id -> last_sent_seq
}

impl SessionSequence {
    fn next_global(&self) -> u64 {
        self.current.fetch_add(1, Ordering::SeqCst)
    }
    
    fn record_sent(&self, attach_id: u64, seq: u64) {
        self.per_attach.insert(attach_id, seq);
    }
    
    fn get_snapshot_start_seq(&self) -> u64 {
        // Snapshot content gets seq numbers reserved before sending
        self.current.fetch_add(snapshot_estimate, Ordering::SeqCst)
    }
}
```

---

## 2. Detailed Protocol State Machine

### 2.1 Server Attach State Machine

```
                    ┌─────────────────┐
                    │   Subscribed    │
                    └────────┬────────┘
                             │ SubscribeV2
                             ▼
                    ┌─────────────────┐
         ┌─────────│  AttachBegin    │◄─────┐
         │         └────────┬────────┘      │
         │                  │               │
         │     send         │               │
         │ AttachBegin      ▼               │
         │         ┌─────────────────┐      │
         │         │  Send Clear     │      │
         │         └────────┬────────┘      │
         │                  │               │
         │     capture      ▼               │
         │   snapshot ┌─────────────────┐   │
         │            │ SnapshotChunk 1 │   │
         │            └────────┬────────┘   │
         │                     │ ...        │
         │                     ▼            │
         │            ┌─────────────────┐   │
         │            │ SnapshotChunk N │   │
         │            └────────┬────────┘   │
         │                     │           │
         │                     ▼           │
         │         ┌─────────────────┐     │
         └────────►│   AttachReady   │─────┘
                   └────────┬────────┘
                            │
                            ▼
                   ┌─────────────────┐
              ┌───│   Live Stream   │◄──── New PTY output
              │    └─────────────────┘
              │             │
              │    Unsubscribe │
              │             ▼
              │    ┌─────────────────┐
              └────│    Detached     │
                   └─────────────────┘
```

### 2.2 Client Handling State Machine

```
┌─────────────────────────────────────────────────────────────┐
│                      Client States                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  IDLE ──► ATTACHING ──► REPLAYING ──► ACTIVE ──► DETACHED  │
│                           │                              │
│                           └─► Cancels stale attach_id      │
│                                                             │
│ State Rules:                                                │
│ - In ATTACHING: Drop all data for previous attach_id       │
│ - In REPLAYING: Buffer live data, apply after ready        │
│ - In ACTIVE: Apply immediately, check seq continuity       │
│ - Gap detected: Log warning, continue (best effort)        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Testing Infrastructure Recommendations

### 3.1 Automated Stress Test Harness

Create a dedicated test harness that runs in CI:

```rust
// cli/tests/attach_stress_test.rs

#[tokio::test]
async fn test_attach_detach_stress() {
    let harness = TestHarness::new().await;
    
    // Scenario: 30 rapid attach/detach cycles
    for i in 0..30 {
        let attach_id = harness.subscribe(session_id).await;
        
        // Wait for ready
        harness.wait_for_attach_ready(attach_id, Duration::from_secs(5)).await;
        
        // Verify no duplicates in captured output
        let output = harness.capture_output(attach_id).await;
        assert_no_duplicate_sequences(&output);
        
        // Detach
        harness.unsubscribe(session_id).await;
        
        // Small random delay to simulate real usage
        tokio::time::sleep(Duration::from_millis(rand::random::<u64>() % 100)).await;
    }
}

#[tokio::test]
async fn test_replay_live_overlap() {
    let harness = TestHarness::new().await;
    
    // Start generating output
    harness.start_continuous_output(session_id, "lines").await;
    
    // Subscribe while output is active
    let attach_id = harness.subscribe(session_id).await;
    
    // Should receive snapshot THEN live, with no overlap
    let events = harness.capture_events(attach_id, Duration::from_secs(3)).await;
    
    // Verify ordering
    let snapshot_end = find_snapshot_end(&events);
    let live_start = find_live_start(&events);
    
    assert!(snapshot_end < live_start, "Snapshot and live must not overlap");
    assert_sequential(&events);
}
```

### 3.2 Protocol Conformance Tests

```typescript
// mobile/__tests__/protocol-v2.test.ts

describe('Protocol V2 Conformance', () => {
  it('should reject stale attach_id data', () => {
    const handler = new AttachHandler();
    
    handler.beginAttach({ attach_id: 1 });
    handler.beginAttach({ attach_id: 2 }); // New attach
    
    // Data from attach 1 should be ignored
    const result = handler.handleChunk({ attach_id: 1, seq: 5 });
    expect(result.accepted).toBe(false);
  });
  
  it('should detect and drop duplicate sequences', () => {
    const handler = new AttachHandler();
    handler.beginAttach({ attach_id: 1 });
    handler.onAttachReady({ attach_id: 1, last_live_seq: 10 });
    
    // First delivery
    handler.handleChunk({ attach_id: 1, seq: 11 });
    expect(handler.renderedSequences).toContain(11);
    
    // Duplicate
    handler.handleChunk({ attach_id: 1, seq: 11 });
    expect(handler.renderedSequences.filter(s => s === 11).length).toBe(1);
  });
  
  it('should handle out-of-order delivery gracefully', () => {
    const handler = new AttachHandler();
    handler.beginAttach({ attach_id: 1 });
    handler.onAttachReady({ attach_id: 1, last_live_seq: 10 });
    
    // Receive 13 before 11, 12
    handler.handleChunk({ attach_id: 1, seq: 13 });
    handler.handleChunk({ attach_id: 1, seq: 11 });
    handler.handleChunk({ attach_id: 1, seq: 12 });
    
    // Should buffer and apply in order
    expect(handler.renderedSequences).toEqual([11, 12, 13]);
  });
});
```

### 3.3 Device QA Checklist Tool

Create a simple test runner app for manual QA:

```typescript
// mobile/components/qa/TerminalQARunner.tsx

interface QATest {
  id: string;
  name: string;
  steps: string[];
  expected: string;
  run: () => Promise<boolean>;
}

const ATTACH_TESTS: QATest[] = [
  {
    id: 'attach-01',
    name: 'Fresh Attach',
    steps: [
      'Start new session on desktop',
      'Open session on mobile',
      'Verify terminal appears',
    ],
    expected: 'Clean terminal, no duplicate output',
    run: async () => { /* automated */ }
  },
  {
    id: 'attach-02', 
    name: 'Attach During Output',
    steps: [
      'Run "yes" on desktop (continuous output)',
      'Open session on mobile',
      'Wait 3 seconds',
      'Stop "yes"',
    ],
    expected: 'No duplicate lines, clean stop',
    run: async () => { /* automated */ }
  },
  {
    id: 'reconnect-01',
    name: 'Rapid Reconnect (30x)',
    steps: ['Automated rapid background/foreground'],
    expected: 'All reconnects succeed, no duplicates',
    run: rapidReconnectTest
  }
];
```

---

## 4. Rollback Strategy

### 4.1 Phase-Based Rollback Points

| Phase | Rollback Point | Data Loss Risk | Procedure |
|-------|---------------|----------------|-----------|
| 0-1 (Protocol) | Revert to v1 protocol | None | Mobile falls back to v1 parsing |
| 2 (Replay) | Use v1 deferred replay | Low | Re-enable old replay path via flag |
| 3 (Resize) | Re-enable jitter/forcing | None | Toggle `MOBILECLI_RESIZE_SIMPLIFIED=0` |
| 4 (Scroll) | Restore gesture layer | None | Revert xterm.html changes |
| 5 (Local Echo) | Disable local echo | None | Toggle `MOBILECLI_ENABLE_LOCAL_ECHO=0` |
| 6 (Cleanup) | Cannot rollback | N/A | Full forward-only |

### 4.2 Emergency Rollback Procedure

```bash
# If critical issue detected in production:

# 1. Immediate mitigation - disable v2 protocol
export MOBILECLI_ATTACH_PROTOCOL=v1

# 2. If resize issues
export MOBILECLI_RESIZE_SIMPLIFIED=0

# 3. If scroll issues
# Revert mobile/assets/xterm.html to pre-overhaul version

# 4. If local echo issues
export MOBILECLI_ENABLE_LOCAL_ECHO=0
```

### 4.3 Data Compatibility

- **Protocol v1/v2**: Bidirectionally compatible (v1 clients work with v2 server)
- **Scrollback format**: No changes to on-disk format
- **Session persistence**: No changes to session.json format
- **Config files**: New optional flags only

---

## 5. Documentation Requirements

### 5.1 Post-Overhaul Documentation

These docs must be updated/created:

| Document | Purpose | Owner |
|----------|---------|-------|
| `docs/ARCHITECTURE.md` | Overall system design | Post-Phase 6 |
| `docs/PROTOCOL_V2.md` | Message formats, state machines | WS-A |
| `docs/ATTACH_SEQUENCE.md` | Attach handshake flow | WS-B |
| `docs/RESIZE_BEHAVIOR.md` | Resize semantics and rules | WS-C |
| `docs/LOCAL_ECHO.md` | Local echo algorithm | WS-E |
| `docs/TROUBLESHOOTING.md` | Debug procedures | WS-F |
| `docs/MIGRATION_GUIDE.md` | For external contributors | Phase 6 |

### 5.2 Code Documentation Standards

```rust
// Every complex state machine needs ASCII diagram

/// Attach v2 state machine
///
/// ```text
///                    ┌─────────────┐
///         ┌─────────│  Subscribed │
///         │         └──────┬──────┘
///         │                │
///         │     subscribe  │
///         │                ▼
///         │         ┌─────────────┐
///         └────────►│   Active    │
///                   └─────────────┘
/// ```
pub async fn handle_subscribe_v2(/* ... */) { }
```

---

## 6. Dependency Mapping for Deletion

### 6.1 Frame Detection Deletion Impact

```
Fields to delete:
- PtySession.frame_cursor_pos_count
- PtySession.frame_erase_line_count  
- PtySession.frame_render_mode

Impacted code:
├─ daemon.rs:update_frame_render_state() [DELETE]
├─ daemon.rs:should_treat_as_tui_for_mobile() [SIMPLIFY]
├─ daemon.rs:subscribe path TUI detection [USE TMUX QUERY]
└─ detection.rs [KEEP but simplify]

Safe to delete because:
- Tmux runtime: Query tmux directly for visible pane state
- Pty runtime: Simpler heuristic or none (assume text mode)
```

### 6.2 Alt-Screen Cross-Chunk Tracking Deletion

```
Fields to delete:
- PtySession.alt_track_tail

Impacted code:
├─ daemon.rs:update_alt_screen_state() [SIMPLIFY]
├─ daemon.rs:ALT_ENTER_SEQS, ALT_LEAVE_SEQS [KEEP for simple check]
└─ daemon.rs:subscribe alt-screen decision [USE TMUX QUERY]

Safe to delete because:
- Tmux: alternate-screen is disabled globally, in_alt_screen always false
- Pty: Simple in-band detection sufficient, no cross-chunk needed
```

### 6.3 Deferred Replay Deletion

```
Fields to delete:
- DaemonState.pending_tui_replay
- clear_pending_tui_replay_for_session()

Impacted code:
├─ daemon.rs:process_client_msg Subscribe [SIMPLIFY]
├─ daemon.rs:process_client_msg PtyResize [REMOVE deferred logic]
└─ daemon.rs:handle_pty_session cleanup [REMOVE]

Safe to delete because:
- New protocol: snapshot is atomic, no deferred phase
- Wait for snapshot completion instead of deferring
```

### 6.4 Resize Coordination Deletion

```
Fields to delete:
- PtySession.last_resize_epoch [CONSIDER KEEPING for stale detection]
- ResizeRequest.reason complexity [SIMPLIFY to essential]

Impacted code:
├─ daemon.rs:is_stale_resize_epoch() [KEEP - still useful]
├─ daemon.rs:should_ignore_resize_without_viewers() [SIMPLIFY]
├─ daemon.rs:should_ignore_restore_resize() [SIMPLIFY]
├─ pty_wrapper.rs:jitter_resize_target() [DELETE]
├─ pty_wrapper.rs:force_noop_refresh logic [DELETE]
└─ TerminalView.tsx:resizeDebounce [KEEP but simplify]

Safe to delete because:
- Jitter/forcing was workaround for rendering issues fixed by protocol v2
- Viewer counting can be simpler
```

---

## 7. Performance Targets

### 7.1 Baseline Measurements

Before starting, capture these metrics:

```bash
# Build benchmark harness
cargo build --release --manifest-path cli/Cargo.toml --features benchmark

# Capture baseline
./target/release/mobilecli benchmark --scenario attach_latency
./target/release/mobilecli benchmark --scenario reconnect_stress
./target/release/mobilecli benchmark --scenario scrollback_memory
```

### 7.2 Target Metrics

| Metric | Current (Est.) | Target | Measurement |
|--------|---------------|--------|-------------|
| Attach to first frame | 500-1500ms | <300ms | Instrumented timing |
| Reconnect time | 1-3s | <500ms | 30x loop average |
| Input latency (no echo) | 100-300ms | <100ms | Key-to-PTY-echo |
| Input latency (with echo) | N/A | <30ms perceived | Local display |
| Snapshot size (tmux) | Variable | <100KB typical | capture-pane |
| Memory per session | ~10MB | <5MB | heaptrack |
| Duplicate frame rate | >0% | 0% | Automated detection |
| Resize event storm | 5-20/sec | <2/sec | Logging |

---

## 8. Risk Mitigation Additions

### 8.1 New Risks Identified

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| tmux snapshot latency | Medium | High | Parallel snapshot + timeout fallback |
| Sequence number overflow | Low | High | Use u64, reset on session restart |
| Mobile WebView seq buffer | Medium | Medium | Bounded buffer, drop old on overflow |
| Reconnection seq gap | Medium | Medium | Client detects gap, server retransmits |
| Local echo in TUIs | High | Medium | Auto-disable in detected TUIs |

### 8.2 Detection and Monitoring

Add structured logging for:

```rust
// In daemon
info!(
    target: "attach_v2",
    session_id,
    attach_id,
    snapshot_chunks,
    snapshot_bytes,
    snapshot_ms,
    "attach_completed"
);

warn!(
    target: "sequence",
    session_id,
    expected_seq,
    received_seq,
    gap_size,
    "sequence_gap_detected"
);

// In mobile
logger.info('[LocalEcho] prediction_confirmed', { char, latency_ms });
logger.warn('[LocalEcho] prediction_corrected', { expected, actual });
```

---

## 9. Additional Workstream Details

### 9.1 WS-A: Protocol Implementation Details

**Step-by-step:**

1. **Add protocol types** (`protocol.rs`)
   - Add v2 message structs alongside v1
   - Implement serialization/deserialization

2. **Server sequence generator** (`daemon.rs`)
   - Add `SessionSequence` struct
   - Integrate into `PtySession`

3. **Attach handshake** (`daemon.rs`)
   - New `handle_subscribe_v2()` function
   - Parallel to v1 during migration

4. **Client parser** (`useSync.ts`)
   - Add v2 message types
   - State machine implementation

### 9.2 WS-B: Snapshot Implementation

**tmux snapshot optimization:**

```rust
async fn capture_tmux_snapshot(
    socket: &str,
    session: &str,
    max_lines: usize,
) -> Result<Vec<u8>, Error> {
    // Capture visible screen + scrollback in one command
    let cmd = format!(
        "capture-pane -t {session} -S -{max_lines} -p",
        session = session,
        max_lines = max_lines
    );
    
    // Run with timeout
    match tokio::time::timeout(
        Duration::from_millis(500),
        run_tmux_command(socket, &cmd)
    ).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => {
            warn!("tmux capture failed: {}", e);
            // Fallback to daemon scrollback
            Err(e)
        }
        Err(_) => {
            warn!("tmux capture timeout");
            Err(Error::Timeout)
        }
    }
}
```

### 9.3 WS-E: Local Echo Algorithm

**Detailed local echo implementation:**

```typescript
class LocalEchoController {
  private predictions: Map<number, string> = new Map();
  private nextSeq = 0;
  private confirmedSeq = -1;
  private terminal: Terminal;
  
  // Characters safe to predict (no escape sequences)
  private static SAFE_CHARS = /^[a-zA-Z0-9 !@#$%^&*()_+\-=\[\]{};':"\\|,.<>\/?]*$/;
  
  onInput(data: string): void {
    // Only predict simple text input
    if (!LocalEchoController.SAFE_CHARS.test(data)) {
      this.sendToServer(data);
      return;
    }
    
    const seq = this.nextSeq++;
    this.predictions.set(seq, data);
    
    // Show dimmed prediction immediately
    this.terminal.write(data, { dim: true });
    
    // Send to server with seq
    this.sendToServer(data, seq);
    
    // Set timeout for confirmation
    setTimeout(() => this.checkConfirmation(seq), 100);
  }
  
  onServerOutput(data: string, seq?: number): void {
    if (seq !== undefined && this.predictions.has(seq)) {
      const predicted = this.predictions.get(seq)!;
      this.predictions.delete(seq);
      
      if (data === predicted) {
        // Confirmation: make solid
        this.confirmPrediction(seq);
      } else {
        // Correction: replace
        this.correctPrediction(seq, data);
      }
    } else {
      // New server output
      this.terminal.write(data);
    }
  }
  
  private confirmPrediction(seq: number): void {
    // Remove dim attribute from predicted text
    // xterm.js specific: refresh the line
  }
  
  private correctPrediction(seq: number, actual: string): void {
    // Backspace over prediction, write actual
    const predicted = this.predictions.get(seq);
    if (predicted) {
      this.terminal.write('\b'.repeat(predicted.length));
      this.terminal.write(actual);
    }
  }
}
```

---

## 10. Integration with Original Plan

### 10.1 Complementary Additions

This addendum complements the master plan with:

1. **Concrete protocol specification** (Section 1-2)
2. **Test infrastructure details** (Section 3)
3. **Safety mechanisms** (Section 4)
4. **Documentation requirements** (Section 5)
5. **Dependency analysis** (Section 6)
6. **Performance baselines** (Section 7)
7. **Additional risk coverage** (Section 8)
8. **Implementation details** (Section 9)

### 10.2 Suggested Integration

Add to original plan:
- **Section 21**: Protocol Specification (reference this doc)
- **Section 22**: Testing Infrastructure Requirements
- **Section 23**: Rollback Procedures
- **Appendix A**: Dependency Deletion Map
- **Appendix B**: Performance Benchmarks

---

## 11. Final Recommendations

### 11.1 Execution Priority Adjustments

Based on this analysis, minor adjustments to original plan:

1. **WS-A and WS-B can be combined** - Protocol changes and replay rewrite are tightly coupled
2. **Add performance harness in Phase 0** - Need baseline before changes
3. **WS-F (observability) should start in Phase 0** - Need logging for debugging

### 11.2 Success Criteria Clarification

Add quantitative metrics to original "Definition of Done":

```
8. Performance targets met:
   - Attach latency P95 < 300ms
   - Reconnect time P95 < 500ms
   - Perceived input latency < 30ms (with echo)
   - Zero duplicate frames in 100 attach/detach cycles
   
9. Code reduction targets achieved:
   - daemon.rs < 2500 lines
   - pty_wrapper.rs < 750 lines
   - xterm.html < 400 lines
   
10. Test coverage:
    - Protocol conformance: 100% of message types
    - Attach scenarios: 100% of validation matrix
    - Unit tests: >80% for new code
```

---

## Summary

The master plan is excellent and ready for execution. This addendum provides:

1. **Concrete protocol design** based on proven systems (Mosh, tmux, Q3)
2. **Detailed test infrastructure** for automated validation
3. **Rollback safety** at each phase boundary
4. **Documentation requirements** for maintainability
5. **Dependency mapping** to guide deletion order
6. **Performance targets** with measurement plan
7. **Implementation details** for complex areas (local echo, snapshots)

**Recommendation**: Proceed with original plan, incorporating this addendum as reference material for implementation details.
