# MobileCLI Terminal Scrolling Fix: Comprehensive Analysis & Implementation Plan

**Document Version:** 1.0  
**Date:** 2026-02-25  
**Status:** Ready for Implementation  
**Priority:** Critical  

---

## Executive Summary

The MobileCLI application suffers from a **critical synchronization issue** where mobile-initiated terminal scrolling does not reflect the actual desktop terminal content. When users swipe on the mobile app to scroll through a tmux session, the desktop tmux correctly enters copy-mode and scrolls, but the mobile terminal view remains unchanged because it never receives the updated viewport content.

### Key Findings

1. **Root Cause:** The daemon sends viewport state metadata (`TmuxViewportState`) but not the actual terminal content after scroll actions
2. **Impact:** Mobile scroll gestures appear non-functional to users
3. **Solution:** Implement server-authoritative viewport frames via a new `TmuxViewportFrame` protocol message
4. **Scope:** Both iOS keyboard spacing and tmux scrolling require coordinated fixes

### Success Criteria

- [ ] Mobile swipe gestures immediately show scrolled content
- [ ] Desktop and mobile viewports remain synchronized (strict mirror)
- [ ] No speculative local scrolling on mobile (server-authoritative)
- [ ] Keyboard spacing on iOS is consistent and correct
- [ ] Performance remains acceptable (frame delivery < 300ms)

---

## 1. Current Implementation Analysis

### 1.1 Architecture Overview

```
┌─────────────────┐     WebSocket      ┌──────────────────┐
│   Mobile App    │◄──────────────────►│  Daemon (Rust)   │
│  (React Native) │                    │   cli/src/       │
│                 │  1. TmuxViewport   │   daemon.rs      │
│  ┌───────────┐  │ ─────────────────► │                  │
│  │  Swipe    │  │     action         │  ┌────────────┐  │
│  │ Gesture   │  │                    │  │   tmux     │  │
│  └─────┬─────┘  │  2. TmuxViewport   │  │  commands  │  │
│        │        │ ◄───────────────── │  └─────┬──────┘  │
│        ▼        │    State only      │        │         │
│  ┌───────────┐  │  (MISSING: frame)  │        ▼         │
│  │  xterm.js │  │                    │  ┌────────────┐  │
│  │  (stale)  │  │                    │  │  Desktop   │  │
│  └───────────┘  │                    │  │  Terminal  │  │
└─────────────────┘                    │  └────────────┘  │
                                       └──────────────────┘
```

### 1.2 Mobile Components

#### TerminalView.tsx (Lines 767-823)

**Swipe Gesture Handling:**
```typescript
const tmuxSwipeResponder = useMemo(
  () =>
    PanResponder.create({
      onStartShouldSetPanResponder: () => tmuxControlsEnabled,
      onMoveShouldSetPanResponder: (_, gestureState) => {
        const absDy = Math.abs(gestureState.dy);
        const absDx = Math.abs(gestureState.dx);
        return absDy >= TMUX_SWIPE_MIN_DISTANCE && absDy > absDx * 1.1;
      },
      onPanResponderRelease: (_, gestureState) => {
        triggerSwipeViewportAction(gestureState.dy);
      },
    }),
  [tmuxControlsEnabled, triggerSwipeViewportAction]
);
```

**Swipe Action Mapping (Lines 775-795):**
```typescript
const triggerSwipeViewportAction = useCallback(
  (dy: number) => {
    if (!tmuxControlsEnabled) return;
    const absDy = Math.abs(dy);
    if (absDy < TMUX_SWIPE_MIN_DISTANCE) {
      focusKeyboard();
      return;
    }
    
    const count = Math.max(1, Math.min(TMUX_SWIPE_MAX_COUNT, 
      Math.floor(absDy / TMUX_SWIPE_STEP_PX) || 1));
    
    Haptics.selectionAsync();
    sendTmuxAction(dy < 0 ? 'page_up' : 'page_down', count);
  },
  [focusKeyboard, sendTmuxAction, tmuxControlsEnabled]
);
```

**Key Observations:**
- Swipe gestures correctly calculate direction and magnitude
- `TMUX_SWIPE_STEP_PX = 140` maps to scroll count
- `TMUX_SWIPE_MAX_COUNT = 8` limits maximum scroll lines
- **No viewport frame consumption exists** - only state updates

#### useSync.ts (Lines 1232-1251)

**Current State Handler:**
```typescript
case 'tmux_viewport_state': {
  const sid = data.session_id;
  if (!sid) break;
  setTmuxViewportState(sid, {
    inCopyMode: !!data.in_copy_mode,
    scrollPosition: /* ... validation ... */,
    historySize: /* ... validation ... */,
    followingLive: /* ... calculation ... */,
  });
  break;
}
```

**Missing:** No handler for viewport frame data that would update terminal content.

### 1.3 Daemon Components

#### daemon.rs TmuxViewport Handler (Lines 2190-2311)

Current flow:
```rust
ClientMessage::TmuxViewport { session_id, action, count } => {
    // 1. Validate session and permissions
    // 2. Execute tmux action via apply_tmux_viewport_action_with_retry
    // 3. Query viewport state via query_tmux_viewport_state
    // 4. Send TmuxViewportState to client
    // ** MISSING: Capture and send viewport frame **
}
```

**Action Execution (Lines 3676-3747):**
```rust
fn run_tmux_viewport_action_once(
    socket: &str,
    session: &str,
    action: TmuxViewportAction,
    count: u16,
) -> Result<(), String> {
    let command = match action {
        TmuxViewportAction::ScrollUp => "scroll-up",
        TmuxViewportAction::ScrollDown => "scroll-down",
        TmuxViewportAction::PageUp => "page-up",
        TmuxViewportAction::PageDown => "page-down",
        TmuxViewportAction::Follow => "cancel",
    };
    
    // For scroll actions: enters copy-mode first, then sends keys
    // Uses: tmux send-keys -X <command>
}
```

**State Query (Lines 3766-3804):**
```rust
fn query_tmux_viewport_state_blocking(
    socket: &str,
    session: &str,
) -> Option<TmuxViewportStateSnapshot> {
    // Queries: #{pane_in_mode}|#{scroll_position}|#{history_size}
    // Returns metadata only, no content
}
```

### 1.4 Protocol Analysis

**Current Server Messages (protocol.rs):**
- `TmuxViewportState` - Metadata only (in_copy_mode, scroll_position, history_size, following_live)
- `PtyBytes` / `PtyChunk` - Live PTY stream (paused in copy-mode)
- `SessionHistory` - Full scrollback (not viewport-specific)

**Missing:** A message type for the visible viewport content after scroll actions.

---

## 2. Root Cause Analysis

### 2.1 The Synchronization Gap

```
User Swipe Up
      │
      ▼
Mobile sends: {type: "tmux_viewport", action: "page_up", count: 3}
      │
      ▼
Daemon executes: tmux send-keys -X copy-mode; tmux send-keys -X page-up (x3)
      │
      ▼
Desktop tmux enters copy-mode, scrolls up 3 pages
      │
      ▼
Daemon queries: display-message -p "#{pane_in_mode}|#{scroll_position}|#{history_size}"
      │
      ▼
Daemon sends: TmuxViewportState {in_copy_mode: true, scroll_position: 150, ...}
      │
      ▼
Mobile updates: "isFollowingLive = false" state variable
      │
      ▼
Mobile terminal: [UNCHANGED - still shows old content]
      │
      ▼
PTY stream: [PAUSED - tmux in copy-mode doesn't emit new content]
      │
      ▼
USER SEES: No change in terminal content (appears broken)
```

### 2.2 Why PTY Streaming Doesn't Work in Copy-Mode

When tmux enters copy-mode:
1. The pane's output is **frozen**
2. No new PTY data is generated
3. The `capture-pane` command shows the **scrollback buffer** at the current position
4. The mobile app must explicitly fetch the visible viewport content

### 2.3 Keyboard Spacing Root Cause

**Current problematic code (TerminalView.tsx lines 169-180):**
```typescript
const toInset = (event: any): number => {
  const height = event?.endCoordinates?.height;
  const screenY = event?.endCoordinates?.screenY;
  const fromHeight = /* ... */;
  const fromScreenY = /* ... */;
  return Math.max(0, Math.floor(Math.max(fromHeight, fromScreenY)));  // ← Double counting
};
```

**Issues:**
1. Takes `max()` of two different measurements that may overlap
2. Doesn't account for safe area insets properly
3. Applies marginBottom that stacks with paddingBottom
4. No clamping to prevent excessive lift

---

## 3. Detailed Solution Design

### 3.1 Protocol Extensions

#### New Message: TmuxViewportFrame

```rust
// cli/src/protocol.rs

/// Server-authoritative viewport frame for tmux copy-mode sessions.
/// Sent immediately after a TmuxViewport action to provide the
/// visible terminal content corresponding to the new scroll position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxViewportFrame {
    pub session_id: String,
    /// Base64-encoded terminal content (ANSI escape sequences preserved)
    pub data: String,
    /// Copy mode state
    pub in_copy_mode: bool,
    /// Current scroll position (lines from bottom)
    pub scroll_position: usize,
    /// Total history size
    pub history_size: usize,
    /// Whether viewport is at live position
    pub following_live: bool,
    /// Pane height in rows (for mobile scroll position calculation)
    pub pane_rows: u16,
    /// Pane width in columns
    pub pane_cols: u16,
    /// Monotonic sequence number for ordering/race detection
    pub sequence: u64,
    /// Timestamp for latency measurement
    pub timestamp_ms: u64,
}

// Add to ServerMessage enum:
pub enum ServerMessage {
    // ... existing variants ...
    
    /// Complete viewport frame after scroll action
    TmuxViewportFrame {
        frame: TmuxViewportFrame,
    },
    
    /// Legacy state message (kept for compatibility)
    TmuxViewportState {
        session_id: String,
        in_copy_mode: bool,
        scroll_position: usize,
        history_size: usize,
        following_live: bool,
    },
}
```

### 3.2 Daemon Implementation

#### Modified TmuxViewport Handler

```rust
// cli/src/daemon.rs

ClientMessage::TmuxViewport { session_id, action, count } => {
    // ... existing validation ...
    
    // 1. Execute the viewport action
    let action_result = apply_tmux_viewport_action_with_retry(
        socket.clone(),
        name.clone(),
        action,
        clamped_count,
    ).await;
    
    // 2. Query the new viewport state
    let viewport_state = query_tmux_viewport_state(socket.clone(), name.clone())
        .await
        .unwrap_or_default();
    
    // 3. CAPTURE THE VIEWPORT FRAME (NEW)
    let frame_bytes = if viewport_state.in_copy_mode {
        capture_tmux_viewport_frame(
            socket.clone(),
            name.clone(),
            &viewport_state,
        ).await
    } else {
        // Following live - capture visible pane only
        capture_tmux_live_viewport(socket.clone(), name.clone()).await
    };
    
    // 4. Send frame FIRST (contains full context)
    if let Some(bytes) = frame_bytes {
        let frame_msg = ServerMessage::TmuxViewportFrame {
            frame: TmuxViewportFrame {
                session_id: session_id.clone(),
                data: BASE64.encode(&bytes),
                in_copy_mode: viewport_state.in_copy_mode,
                scroll_position: viewport_state.scroll_position,
                history_size: viewport_state.history_size,
                following_live: viewport_state.following_live,
                pane_rows: viewport_state.pane_rows,
                pane_cols: viewport_state.pane_cols,
                sequence: next_viewport_sequence(&session_id),
                timestamp_ms: Utc::now().timestamp_millis() as u64,
            },
        };
        tx.send(Message::Text(serde_json::to_string(&frame_msg)?)).await?;
    }
    
    // 5. Send state SECOND (for compatibility with older clients)
    let state_msg = ServerMessage::TmuxViewportState {
        session_id: session_id.clone(),
        in_copy_mode: viewport_state.in_copy_mode,
        scroll_position: viewport_state.scroll_position,
        history_size: viewport_state.history_size,
        following_live: viewport_state.following_live,
    };
    tx.send(Message::Text(serde_json::to_string(&state_msg)?)).await?;
    
    // 6. Send error if action failed
    if let Err(error) = action_result {
        let msg = ServerMessage::Error {
            code: "tmux_viewport_action_failed".to_string(),
            message: error,
        };
        tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
    }
}
```

#### New Capture Functions

```rust
/// Capture the visible viewport content in copy-mode.
/// Returns the exact content visible at the current scroll position.
async fn capture_tmux_viewport_frame(
    socket: String,
    session: String,
    state: &TmuxViewportStateSnapshot,
) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        capture_tmux_viewport_frame_blocking(&socket, &session, state)
    }).await.ok().flatten()
}

fn capture_tmux_viewport_frame_blocking(
    socket: &str,
    session: &str,
    state: &TmuxViewportStateSnapshot,
) -> Option<Vec<u8>> {
    // Calculate visible region
    // scroll_position is lines from bottom
    // pane_rows is visible height
    let end_line = state.history_size.saturating_sub(state.scroll_position);
    let start_line = end_line.saturating_sub(state.pane_rows as usize);
    
    for target in tmux_targets(session) {
        let mut cmd = std::process::Command::new("tmux");
        cmd.arg("-L").arg(socket)
            .arg("-f").arg("/dev/null")
            .env_remove("TMUX")
            .arg("capture-pane")
            .arg("-p")
            .arg("-e")  // Preserve escape sequences
            .arg("-t").arg(&target);
        
        // Specify exact line range for viewport
        if state.in_copy_mode {
            cmd.arg("-S").arg(start_line.to_string());
            cmd.arg("-E").arg(end_line.to_string());
        }
        
        match cmd.output() {
            Ok(out) if out.status.success() => {
                return Some(out.stdout);
            }
            _ => continue,
        }
    }
    None
}

/// Capture live viewport (not in copy-mode)
async fn capture_tmux_live_viewport(
    socket: String,
    session: String,
) -> Option<Vec<u8>> {
    capture_tmux_history(socket, session, false, 0).await
}
```

### 3.3 Mobile Implementation

#### useSync.ts Extensions

```typescript
// Types
export interface TmuxViewportFrame {
  sessionId: string;
  data: string;              // base64 terminal content
  inCopyMode: boolean;
  scrollPosition: number;
  historySize: number;
  followingLive: boolean;
  paneRows: number;
  paneCols: number;
  sequence: number;
  timestampMs: number;
}

// Frame callback type
export type TmuxViewportFrameCallback = (frame: TmuxViewportFrame) => void;

// Add to SyncState interface
tmuxViewportFrameCallbacks: Record<string, TmuxViewportFrameCallback | null>;
setTmuxViewportFrameCallback: (sessionId: string, callback: TmuxViewportFrameCallback | null) => void;

// Message handler
case 'tmux_viewport_frame': {
  const sid = data.session_id;
  if (!sid) break;
  
  const frame: TmuxViewportFrame = {
    sessionId: sid,
    data: data.data,
    inCopyMode: data.in_copy_mode,
    scrollPosition: data.scroll_position,
    historySize: data.history_size,
    followingLive: data.following_live,
    paneRows: data.pane_rows,
    paneCols: data.pane_cols,
    sequence: data.sequence,
    timestampMs: data.timestamp_ms,
  };
  
  // Update state
  setTmuxViewportState(sid, {
    inCopyMode: frame.inCopyMode,
    scrollPosition: frame.scrollPosition,
    historySize: frame.historySize,
    followingLive: frame.followingLive,
  });
  
  // Route to terminal consumer
  const callback = tmuxViewportFrameCallbacks[sid];
  if (callback) {
    callback(frame);
  }
  
  // Clear pending state
  clearViewportPending(sid);
  break;
}

// Pending viewport action tracking
interface PendingViewportAction {
  sequence: number;
  timeoutId: ReturnType<typeof setTimeout>;
  sentAt: number;
}

const globalViewportPendingBySession: Record<string, PendingViewportAction> = {};

export function setViewportPending(sessionId: string, sequence: number): void {
  clearViewportPending(sessionId);
  
  const timeoutId = setTimeout(() => {
    logger.warn(' Viewport frame timeout:', sessionId);
    clearViewportPending(sessionId);
    // Trigger UI indicator for sync delay
  }, 500);
  
  globalViewportPendingBySession[sessionId] = {
    sequence,
    timeoutId,
    sentAt: Date.now(),
  };
}

export function clearViewportPending(sessionId: string): void {
  const pending = globalViewportPendingBySession[sessionId];
  if (pending) {
    clearTimeout(pending.timeoutId);
    delete globalViewportPendingBySession[sessionId];
  }
}

export function isViewportPending(sessionId: string): boolean {
  return !!globalViewportPendingBySession[sessionId];
}
```

#### TerminalView.tsx Integration

```typescript
// Add to props
interface TerminalViewProps {
  // ... existing props ...
  setTmuxViewportFrameCallback?: (
    sessionId: string,
    callback: ((frame: TmuxViewportFrame) => void) | null
  ) => void;
}

// Frame rendering effect
useEffect(() => {
  if (!setTmuxViewportFrameCallback) return;
  
  setTmuxViewportFrameCallback(sessionId, (frame) => {
    // Prevent race: ignore stale frames
    const currentSeq = globalLastViewportSequence[sessionId] || 0;
    if (frame.sequence < currentSeq) {
      logger.log(' Ignoring stale viewport frame:', frame.sequence, '<', currentSeq);
      return;
    }
    globalLastViewportSequence[sessionId] = frame.sequence;
    
    // Clear terminal and render frame
    xtermRef.current?.clear();
    xtermRef.current?.writeBase64(frame.data);
    
    // Handle scroll position
    if (frame.followingLive) {
      xtermRef.current?.scrollToBottom();
      followRequestInFlightRef.current = false;
      
      // Flush deferred input
      const deferred = deferredInputUntilFollowRef.current;
      if (deferred) {
        deferredInputUntilFollowRef.current = '';
        onSendRawInput?.(deferred);
      }
    }
    
    // Latency telemetry (dev only)
    const latency = Date.now() - frame.timestampMs;
    logger.log(' Viewport frame rendered:', {
      sequence: frame.sequence,
      latencyMs: latency,
      bytes: frame.data.length,
      scrollPos: frame.scrollPosition,
    });
  });
  
  return () => {
    setTmuxViewportFrameCallback(sessionId, null);
  };
}, [sessionId, setTmuxViewportFrameCallback, onSendRawInput]);

// Modified swipe handler with pending state
const triggerSwipeViewportAction = useCallback((dy: number) => {
  if (!tmuxControlsEnabled) return;
  
  const absDy = Math.abs(dy);
  if (absDy < TMUX_SWIPE_MIN_DISTANCE) {
    focusKeyboard();
    return;
  }
  
  // Prevent duplicate actions while waiting for frame
  if (isViewportPending(sessionId)) {
    logger.log(' Ignoring swipe, viewport action pending');
    return;
  }
  
  const now = Date.now();
  if (now - lastSwipeActionAtRef.current < TMUX_SWIPE_COOLDOWN_MS) return;
  lastSwipeActionAtRef.current = now;
  
  const count = Math.max(1, Math.min(TMUX_SWIPE_MAX_COUNT, 
    Math.floor(absDy / TMUX_SWIPE_STEP_PX) || 1));
  
  Haptics.selectionAsync();
  
  // Generate sequence and set pending BEFORE sending
  const sequence = getNextViewportSequence(sessionId);
  setViewportPending(sessionId, sequence);
  
  const sent = sendTmuxViewportAction(
    sessionId,
    dy < 0 ? 'page_up' : 'page_down',
    count
  );
  
  if (!sent) {
    clearViewportPending(sessionId);
  }
}, [focusKeyboard, sendTmuxViewportAction, tmuxControlsEnabled, sessionId]);
```

### 3.4 Keyboard Spacing Fix

```typescript
// TerminalView.tsx - Keyboard handling

const calculateKeyboardInset = useCallback((event: any): number => {
  const screenHeight = Dimensions.get('screen').height;
  const screenY = event?.endCoordinates?.screenY;
  
  // Prefer screenY when available (iOS gives accurate position)
  if (typeof screenY === 'number' && Number.isFinite(screenY) && screenY > 0) {
    // Calculate actual overlap with safe area
    const keyboardTop = screenY;
    const visibleHeight = keyboardTop;
    const expectedHeight = screenHeight - insets.bottom;
    const overlap = expectedHeight - visibleHeight;
    
    return Math.max(0, Math.floor(overlap));
  }
  
  // Fallback to height (Android)
  const height = event?.endCoordinates?.height;
  if (typeof height === 'number' && Number.isFinite(height)) {
    // Android height includes navigation bar, subtract safe area
    return Math.max(0, Math.floor(height));
  }
  
  return 0;
}, [insets.bottom]);

// Toolbar positioning
const toolbarLift = useMemo(() => {
  if (keyboardInset <= 0) return 0;
  
  // Lift by keyboard overlap + small gap
  const lift = keyboardInset + TOOLBAR_KEYBOARD_GAP;
  
  // Clamp to reasonable maximum (55% of screen)
  const maxLift = Math.floor(screenHeight * 0.55);
  return Math.min(lift, maxLift);
}, [keyboardInset, screenHeight]);

// Render
<View
  style={[
    styles.toolbar,
    {
      backgroundColor: colors.bgHighlight,
      borderTopColor: colors.border,
      paddingBottom: 10 + insets.bottom,
      transform: [{ translateY: -toolbarLift }],
    },
  ]}
>
```

---

## 4. Technical Considerations

### 4.1 Frame Capture Performance

**tmux `capture-pane` Options Analysis:**

| Flag | Purpose | Recommendation |
|------|---------|----------------|
| `-p` | Print to stdout | ✓ Required |
| `-e` | Preserve escape sequences | ✓ Required for formatting |
| `-C` | Preserve cursor position | ✗ Skip (irrelevant for viewport) |
| `-N` | No line numbers | ✓ Use for cleaner output |
| `-S` | Start line | ✓ Use for viewport bounds |
| `-E` | End line | ✓ Use for viewport bounds |

**Line Range Calculation:**
```
history_size = total lines in scrollback
scroll_position = lines from bottom
device_pane_rows = visible height

end_line = history_size - scroll_position
start_line = end_line - pane_rows
```

### 4.2 Bandwidth and Compression

**Typical Frame Sizes:**
- 80x24 terminal with color: ~3-8KB uncompressed
- 100x40 terminal with color: ~8-15KB uncompressed
- Base64 encoding adds ~33% overhead

**Mitigation Strategies:**
1. **Frame Deduplication:** Don't send frame if scroll_position unchanged
2. **Delta Frames:** Future optimization - only changed lines
3. **Compression:** Consider gzip for large frames (adds latency)
4. **Rate Limiting:** Max 10 viewport actions/second per session

### 4.3 Race Condition Handling

**Scenario: Rapid Swipes**
```
Time  T1        T2        T3        T4
      │         │         │         │
User  Swipe#1   Swipe#2   Swipe#3   │
      │         │         │         │
Send  Frame#1   Frame#2   Frame#3   │
      │         │         │         │
Recv  │         Frame#1   Frame#2   Frame#3
      │         │         │         │
Rend  │         Render#1  Render#2  Render#3
```

**Solution:** Sequence numbers with discard-if-older logic.

**Scenario: PTY Data During Copy-Mode**
```
User  Swipe Up (enters copy-mode)
PTY   [data chunk from before copy-mode]
      ↑ Race: Old data arriving after clear
```

**Solution:** 
1. Set "viewport pending" flag on swipe
2. Queue (don't drop) PTY bytes while pending
3. After frame render, apply queued PTY bytes only if following_live

### 4.4 Multi-Device Coordination

**Current Controller Model:**
- Last device to send viewport action becomes controller
- Controller reassignment on every action

**Issues:**
- Device A scrolls up, Device B scrolls down → fighting
- No visual indication of which device controls

**Recommendations:**
1. Add controller device ID to state messages
2. Show subtle indicator on mobile when not controller
3. Consider timeout-based controller release (30s inactivity)

---

## 5. Implementation Roadmap

### Phase 1: Protocol & Daemon (Day 1-2)

**Files to Modify:**
- `cli/src/protocol.rs` - Add `TmuxViewportFrame` struct and message variant
- `cli/src/daemon.rs` - Implement capture and frame delivery

**Steps:**
1. Define `TmuxViewportFrame` struct with all fields
2. Add to `ServerMessage` enum
3. Implement `capture_tmux_viewport_frame()` function
4. Modify `TmuxViewport` handler to send frame before state
5. Add sequence number tracking per session
6. Test with manual tmux commands

**Testing:**
```bash
# Terminal 1: Start daemon with debug logging
RUST_LOG=debug cargo run -- daemon

# Terminal 2: Create tmux session and connect mobile
# Send viewport action via WebSocket and verify frame received
```

### Phase 2: Mobile Sync Layer (Day 3)

**Files to Modify:**
- `mobile/hooks/useSync.ts` - Add frame handling and pending state

**Steps:**
1. Add `TmuxViewportFrame` TypeScript interface
2. Add `tmuxViewportFrameCallbacks` to store
3. Add `setTmuxViewportFrameCallback` action
4. Add `tmux_viewport_frame` message handler
5. Implement pending action tracking (`setViewportPending`, `clearViewportPending`)
6. Export `isViewportPending` function
7. Add latency logging

### Phase 3: Terminal View Integration (Day 4)

**Files to Modify:**
- `mobile/components/TerminalView.tsx` - Frame rendering and swipe throttling

**Steps:**
1. Add `setTmuxViewportFrameCallback` to props
2. Implement frame rendering effect (clear + write)
3. Add sequence tracking for race prevention
4. Modify `triggerSwipeViewportAction` to check pending state
5. Add swipe cooldown enforcement
6. Handle deferred input flush after follow

### Phase 4: Keyboard Spacing Fix (Day 4-5)

**Files to Modify:**
- `mobile/components/TerminalView.tsx` - Keyboard handling and toolbar positioning

**Steps:**
1. Replace `toInset` with `calculateKeyboardInset`
2. Use single source of truth (prefer `screenY` on iOS)
3. Fix toolbar positioning (remove double marginBottom)
4. Add debug logging for keyboard metrics
5. Test on iPhone with/without home indicator
6. Test on Android with different keyboard types

### Phase 5: Testing & Polish (Day 6-7)

**Test Matrix:**

| Scenario | Desktop | Mobile | Expected |
|----------|---------|--------|----------|
| Single swipe up | Enters copy-mode | Shows scrolled content | ✓ Sync |
| Multiple rapid swipes | Scrolls multiple pages | Shows final position | ✓ Sync |
| Swipe up → type | Exits copy-mode, shows input | Returns to bottom, shows input | ✓ Sync |
| Heavy output + swipe | Scrolls up while outputting | Shows scrolled content | ✓ Sync |
| Keyboard show/hide | No change | Toolbar adjusts | ✓ Smooth |

---

## 6. Risk Assessment

### High Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| Frame capture adds >500ms latency | Poor UX | Optimize capture, add loading indicator |
| Large frames cause WebView freeze | App freeze | Chunk large frames, add size limits |
| Copy-mode detection fails | Wrong content shown | Fallback to full capture, add validation |

### Medium Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| tmux version incompatibility | Capture fails | Version detection, graceful degradation |
| Race conditions with PTY stream | Jumbled output | Pending state + queue |
| Multi-device fighting | Unpredictable scroll | Controller indication, timeout |

### Low Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| Base64 encoding overhead | Bandwidth | Acceptable for terminal size |
| Memory growth from frames | OOM | Frame size limits, GC hints |

---

## 7. Testing Strategy

### 7.1 Unit Tests

**Daemon (Rust):**
```rust
#[test]
fn test_viewport_frame_capture() {
    // Mock tmux session with known content
    // Verify capture returns expected lines
}

#[test]
fn test_sequence_number_monotonicity() {
    // Verify sequence increments correctly
}

#[test]
fn test_line_range_calculation() {
    // Verify start/end line math for various positions
}
```

**Mobile (TypeScript):**
```typescript
describe('Viewport Frame Handling', () => {
  it('should reject stale frames', () => {
    // Test sequence number filtering
  });
  
  it('should set pending state correctly', () => {
    // Test pending action tracking
  });
  
  it('should timeout pending actions', () => {
    // Test timeout behavior
  });
});
```

### 7.2 Integration Tests

**Manual Test Script:**
```bash
# 1. Setup
./mobilecli daemon
# Connect mobile app

# 2. Generate scrollback
tmux new-session -s test "for i in {1..1000}; do echo \"Line $i\"; done; bash"

# 3. Test cases
# - Swipe up once: verify both mobile and desktop show line ~960
# - Swipe up 3x rapidly: verify no jumbled output
# - Type while scrolled: verify jump to bottom
# - Keyboard show/hide: verify toolbar positioning
```

### 7.3 Performance Benchmarks

**Metrics to Track:**
- Frame capture time (target: <100ms)
- Frame delivery latency (target: <300ms end-to-end)
- WebView render time (target: <50ms)
- Memory usage per frame (target: <20MB)

---

## 8. Open Questions

1. **Wrapped Lines:** How does tmux handle wrapped lines in `capture-pane`? Do we get logical lines or visual lines?
   - *Research needed:* Test with narrow pane and long output

2. **UTF-8/Unicode:** Are multibyte characters handled correctly in frame capture?
   - *Risk:* Character count vs byte count mismatch

3. **Pane Resizing:** What happens if pane is resized during copy-mode?
   - *Mitigation:* Include pane dimensions in frame for validation

4. **Alternative Scrollback:** Should we consider using tmux's `save-buffer` instead of `capture-pane`?
   - *Trade-off:* Speed vs accuracy

---

## 9. Appendix A: tmux Command Reference

### Viewport State Query
```bash
tmux display-message -p -t <target> "#{pane_in_mode}|#{scroll_position}|#{history_size}|#{pane_height}|#{pane_width}"
```

### Viewport Capture
```bash
# Copy-mode visible region
tmux capture-pane -p -e -t <target> -S <start_line> -E <end_line>

# Live viewport
tmux capture-pane -p -e -t <target>
```

### Copy-Mode Commands
```bash
tmux send-keys -X copy-mode        # Enter copy-mode
tmux send-keys -X scroll-up        # Scroll up 1 line
tmux send-keys -X scroll-down      # Scroll down 1 line
tmux send-keys -X page-up          # Page up
tmux send-keys -X page-down        # Page down
tmux send-keys -X cancel           # Exit copy-mode (follow)
```

---

## 10. Appendix B: Message Flow Diagrams

### Successful Scroll Action
```
Mobile                              Daemon                         tmux
  │                                   │                            │
  │── TmuxViewport {action:page_up}──►│                            │
  │                                   │── send-keys -X copy-mode ──►│
  │                                   │◄────────────────────────────┤
  │                                   │── send-keys -X page-up ────►│
  │                                   │◄────────────────────────────┤
  │                                   │                            │
  │                                   │── display-message ─────────►│
  │                                   │◄── "1|100|5000|24|80" ──────┤
  │                                   │                            │
  │                                   │── capture-pane -S 476 -E 500──►│
  │                                   │◄── [terminal bytes] ──────────┤
  │                                   │                            │
  │◄── TmuxViewportFrame {seq:1}─────│                            │
  │                                   │                            │
  │◄── TmuxViewportState ────────────│                            │
  │                                   │                            │
  │  [clear terminal]                 │                            │
  │  [write frame data]               │                            │
  │                                   │                            │
```

### Race Condition Handling
```
Mobile                              Daemon
  │                                   │
  │── TmuxViewport {action:page_up}──►│
  │  [set pending seq=1]              │
  │                                   │── [processing...]
  │                                   │
  │── TmuxViewport {action:page_up}──►│  ← Blocked! pending set
  │  [rejected - pending]             │
  │                                   │
  │◄── TmuxViewportFrame {seq=1}─────│
  │  [clear pending]                  │
  │                                   │
  │── TmuxViewport {action:page_up}──►│  ← Accepted
  │  [set pending seq=2]              │
```

---

## 11. Conclusion

This document provides a comprehensive plan for fixing the MobileCLI terminal scrolling synchronization issue. The key insight is that the daemon must send the actual terminal content (viewport frame) after scroll actions, not just state metadata.

### Implementation Checklist

- [ ] Add `TmuxViewportFrame` to protocol
- [ ] Implement frame capture in daemon
- [ ] Add frame handler in useSync.ts
- [ ] Integrate frame rendering in TerminalView.tsx
- [ ] Add pending state for race prevention
- [ ] Fix iOS keyboard spacing
- [ ] Add comprehensive logging
- [ ] Write tests
- [ ] Performance benchmarking
- [ ] Documentation updates

### Success Metrics

1. **Functional:** Swipe gestures immediately reflect scrolled content
2. **Performance:** Frame delivery < 300ms in 95th percentile
3. **Reliability:** No jumbled output in stress tests (100 rapid swipes)
4. **UX:** No visible keyboard spacing issues on iOS/Android

---

**Document End**

*For questions or clarifications, refer to the inline code comments and test cases in the implementation.*


## Appendix D: Tmux Configuration Fixes (2026-02-26)

### Problem Summary

Two critical tmux configuration issues were discovered during testing on the Apple review server (Hetzner VPS):

1. **History limit stuck at 2000 lines** - The daemon was attempting to set `history-limit 200000` globally before creating the tmux session, but this failed silently because tmux requires a running server to accept `set-option` commands.

2. **Alternate screen not disabled** - While `alternate-screen off` was being set correctly on individual windows, the global setting wasn't being applied before session creation.

### Root Cause Analysis

The tmux server lifecycle works as follows:
- `tmux start-server` does NOT create a persistent server
- The server only starts when the first session is created via `new-session`
- The server exits when all sessions are killed
- Global options (`set-option -g`) require a running server

The original code attempted:
```rust
// This fails - no server running yet!
tmux set-option -g history-limit 200000  
// Server starts here
tmux new-session -d -s mysession
```

### Solution

The fix creates a bootstrap session first to start the server, sets global options, then creates the real session:

```rust
// 1. Create bootstrap session to start server
let bootstrap = format!("{}-bootstrap", session_name);
tmux new-session -d -s &bootstrap /bin/sleep 3600

// 2. Set global options (server is now running)
tmux set-option -g history-limit 200000
tmux set-window-option -g alternate-screen off

// 3. Create real session (inherits history-limit)
tmux new-session -d -s session_name ...

// 4. Kill bootstrap (server stays alive because real session exists)
tmux kill-session -t bootstrap
```

### Code Changes

File: `cli/src/pty_wrapper.rs`

The `setup_tmux_session` function was modified to:
1. Create a bootstrap session with `/bin/sleep 3600` before setting any options
2. Set `history-limit 200000` globally while the bootstrap session keeps the server alive
3. Create the real session which inherits the global history-limit
4. Kill the bootstrap session (the real session keeps the server alive)

### Verification

Test locally:
```bash
cargo build --release
./target/release/mobilecli -n Test bash
tmux -L mcli-xxx show-options -g | grep history-limit
# Should show: history-limit 200000
tmux -L mcli-xxx show-window-options -t mcli-xxx:0 | grep alternate
# Should show: alternate-screen off
```

Server deployment:
```bash
scp target/release/mobilecli root@65.21.108.223:/usr/local/bin/
ssh root@65.21.108.223 "systemctl restart mobilecli-daemon"
```

### Result

- ✅ `history-limit 200000` - Sessions now have 200,000 lines of scrollback (was 2,000)
- ✅ `alternate-screen off` - Windows properly disable alternate screen for scrollback capture
- ✅ Deployed to Apple review server on 2026-02-26 03:04 UTC



## Appendix E: Spawn Loop Incident (2026-02-26)

### Incident Summary

**Time:** 2026-02-26 03:20 - 03:40 UTC  
**Impact:** Server overload with 800+ tmux sessions created in ~20 minutes  
**Root Cause:** Bootstrap session implementation for history-limit fix caused sessions to exit immediately, triggering respawn loops

### What Happened

1. Deployed bootstrap session code to set `history-limit 200000` before creating the real tmux session
2. The bootstrap session approach caused tmux sessions to exit immediately with exit_code=0
3. The ensure-demo-session script (running in a while loop) kept respawning sessions
4. Created 800+ sessions in ~20 minutes, overwhelming the server

### Technical Details

The problematic code attempted to:
1. Create a bootstrap tmux session with `sleep 3600` to keep server alive
2. Set `history-limit 200000` globally
3. Create the real session
4. Kill the bootstrap session

**Why it failed:**
- The bootstrap session approach interfered with tmux's server lifecycle
- Sessions were exiting immediately, possibly due to socket/session name conflicts
- The outer tmux session (from ensure-demo-script) was restarting the command

### Resolution

Reverted to simpler approach:
1. Create tmux session first (with default 2000 line history)
2. Set `history-limit 200000` globally for future windows
3. First window has 2000 lines, but global setting is correct for new windows

### Code Changes

**File:** `cli/src/pty_wrapper.rs`

**Reverted:** Bootstrap session approach that caused spawn loop
**Kept:** Xterm geometry fix (`-geometry 160x50 -fa Monospace -fg white -bg black`)

### Verification

After fix deployment:
- Daemon runs stable with ~4 processes
- Sessions create correctly with `history-limit 200000`
- No spawn loop behavior

### Lessons Learned

1. Test session lifecycle thoroughly before deploying tmux changes
2. Bootstrap sessions can interfere with tmux server state
3. Monitor process counts immediately after daemon deployments
4. Keep the ensure-demo-script disabled during testing

