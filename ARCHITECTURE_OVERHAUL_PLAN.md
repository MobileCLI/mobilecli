# MobileCLI Architecture Overhaul Plan

## Executive Summary

This document outlines the complete restructuring of MobileCLI to remove tmux runtime and implement direct PTY management with proper mobile/desktop priority switching.

**Goal**: Termius-quality terminal experience - reliable, responsive, and natively terminal-focused without CLI-specific detection.

---

## Current Architecture (Problematic)

```
┌─────────────────────────────────────────────────────────────┐
│                    DESKTOP TERMINAL                          │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐    │
│  │  mobilecli  │────►│    tmux     │◄────│  Codex/Gem  │    │
│  │ pty-wrapper │     │   session   │     │   (running) │    │
│  └─────────────┘     └─────────────┘     └─────────────┘    │
│         │                   │                                │
│         │              capture-pane                         │
│         │               (problematic)                       │
│         ▼                   ▼                                │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                       DAEMON                         │   │
│  │  ┌──────────────┐    ┌──────────────┐              │   │
│  │  │  Scrollback  │◄───│   tmux       │              │   │
│  │  │   Buffer     │    │  capture     │              │   │
│  │  └──────────────┘    └──────────────┘              │   │
│  │         │                                          │   │
│  └─────────┼──────────────────────────────────────────┘   │
│            │                                                │
│            ▼ WebSocket                                      │
│     MOBILE CLIENT ( receives cursor-positioned frames )     │
└─────────────────────────────────────────────────────────────┘
```

### Problems with Current Architecture

1. **tmux capture-pane includes cursor positioning** (CSI H/f sequences)
2. **Replaying at mobile dimensions creates jumbled content**
3. **Complex runtime switching** (tmux vs pty modes)
4. **CLI detection required** to decide scrollback behavior
5. **Keyboard resize suppression hacks** needed to prevent redraw pollution

---

## Target Architecture (Clean)

```
┌─────────────────────────────────────────────────────────────┐
│                    DESKTOP TERMINAL                          │
│  ┌─────────────┐     ┌─────────────────────────────────┐    │
│  │  mobilecli  │────►│         DAEMON-MANAGED PTY      │    │
│  │   wrapper   │     │  ┌──────────────┐ ┌──────────┐  │    │
│  │  (viewer)   │     │  │   PTY Master │ │ Command  │  │    │
│  └─────────────┘     │  │  (one size)  │ │ (codex)  │  │    │
│         ▲            │  └──────────────┘ └──────────┘  │    │
│         │            │           │                     │    │
│         │            └───────────┼─────────────────────┘    │
│    (passive view)                │                          │
│                                  │ (resize SIGWINCH)        │
│                                  ▼                          │
│     MOBILE CLIENT (primary, controls size)                  │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Principles

1. **One PTY per session**, daemon-managed
2. **Mobile has priority** - resizes PTY when connected
3. **Desktop is passive viewer** - sees content at mobile size or its own
4. **No cursor-positioned content replay** - clear on connect, let app redraw
5. **Plain text scrollback only** - safe to replay at any size
6. **No CLI detection** - terminal is terminal regardless of what's running

---

## Detailed Implementation Plan

### Phase 1: Remove tmux Runtime

#### 1.1 Remove tmux module and dependencies
**Files to modify:**
- `cli/src/tmux.rs` → DELETE entire file
- `cli/src/pty_wrapper.rs` → Remove tmux-related code
- `cli/src/daemon.rs` → Remove tmux_socket, tmux_session fields

**Specific removals:**
```rust
// DELETE from pty_wrapper.rs:
- RuntimeMode enum (Tmux variant)
- TmuxContext struct
- tmux_base_command() function
- run_tmux_checked() function
- setup_tmux_session() function
- cleanup_tmux_session() function
- All tmux-related logic in run_wrapped()

// DELETE from daemon.rs:
- PtySession.tmux_socket field
- PtySession.tmux_session field
- All capture_tmux_history* functions
- should_include_tmux_scrollback_for_mobile() function
- hints_ai_cli_label() function
```

#### 1.2 Simplify PtySession struct
**Before:**
```rust
pub struct PtySession {
    pub session_id: String,
    pub runtime: String,              // "tmux" or "pty"
    pub tmux_socket: Option<String>,  // ← DELETE
    pub tmux_session: Option<String>, // ← DELETE
    pub name: String,
    pub command: String,
    // ... other fields
}
```

**After:**
```rust
pub struct PtySession {
    pub session_id: String,
    pub name: String,
    pub command: String,
    pub project_path: String,
    pub started_at: DateTime<Utc>,
    pub input_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub resize_tx: mpsc::UnboundedSender<ResizeRequest>,
    pub scrollback: VecDeque<u8>,      // Daemon-managed, plain text only
    pub scrollback_max_bytes: usize,
    pub in_alt_screen: bool,           // Track from PTY output
    pub alt_track_tail: Vec<u8>,
    pub live_seq: u64,
    pub last_applied_size: Option<(u16, u16)>,
    pub last_requested_size: Option<(u16, u16)>,
    pub last_resize_epoch: u64,
    pub raw_input_tail: Vec<u8>,
    // REMOVE: cli_tracker, last_wait_hash, tmux fields
}
```

### Phase 2: Implement Safe Scrollback Replay

#### 2.1 Create terminal-native detection
**New file: `cli/src/terminal.rs`**

```rust
//! Terminal-native content analysis
//! Pure escape sequence detection - no CLI-specific logic

/// Detect cursor positioning sequences that make content unsafe to replay
/// at different terminal sizes.
pub fn contains_cursor_positioning(data: &[u8]) -> bool {
    // CSI sequences that indicate cursor positioning:
    // - ESC [ row ; col H  (Cursor Position)
    // - ESC [ row ; col f  (Horizontal Vertical Position)
    // - ESC [ ? 1049 h     (Enter alternate screen)
    // - ESC [ ? 1047 h     (Enter alternate screen - xterm)
    // - ESC [ ? 47 h       (Enter alternate screen - legacy)
    
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0x1b && data[i+1] == b'[' {
            if let Some(seq_type) = detect_csi_type(&data[i..]) {
                match seq_type {
                    CsiType::CursorPosition => return true,
                    CsiType::AlternateScreen => return true,
                    _ => {}
                }
            }
        }
        i += 1;
    }
    false
}

enum CsiType {
    CursorPosition,    // H or f
    AlternateScreen,   // ?1049h, ?1047h, ?47h
    Other,
}

fn detect_csi_type(data: &[u8]) -> Option<CsiType> {
    // Parse CSI sequence and return type
    // Implementation details...
}

/// Check if scrollback is safe to replay (plain text only)
pub fn is_safe_to_replay(data: &[u8]) -> bool {
    !contains_cursor_positioning(data)
}
```

#### 2.2 Update scrollback handling
**In `daemon.rs`:**

```rust
/// Get scrollback that's safe to replay at mobile dimensions
fn get_safe_scrollback(session: &PtySession) -> Option<Vec<u8>> {
    if session.scrollback.is_empty() {
        return None;
    }
    
    let scrollback: Vec<u8> = session.scrollback.iter().copied().collect();
    
    if is_safe_to_replay(&scrollback) {
        Some(scrollback)
    } else {
        // Contains cursor positioning - not safe to replay
        None
    }
}
```

### Phase 3: Simplify Subscribe Flow

#### 3.1 Unified subscribe handler
**Current flow has multiple branches for tmux vs pty, CLI types, etc.**

**New unified flow:**

```rust
async fn handle_subscribe(
    session_id: String,
    addr: SocketAddr,
    client_capabilities: Option<u32>,
    state: &SharedState,
    tx: &mut WebSocketSender,
) -> Result<()> {
    // 1. Generate attach ID
    let attach_id = {
        let mut st = state.write().await;
        let id = st.next_attach_id;
        st.next_attach_id += 1;
        id
    };
    
    // 2. Send AttachBegin
    send(AttachBegin {
        session_id: session_id.clone(),
        attach_id,
        // Remove runtime - not needed for mobile behavior
    }).await?;
    
    // 3. ALWAYS clear terminal first
    send(AttachClear {
        session_id: session_id.clone(),
        attach_id,
    }).await?;
    
    // 4. Get safe scrollback (if any)
    let safe_scrollback = {
        let st = state.read().await;
        st.sessions.get(&session_id)
            .and_then(get_safe_scrollback)
    };
    
    // 5. Send scrollback in chunks (if safe to replay)
    if let Some(data) = safe_scrollback {
        for chunk in chunk_snapshot_payload(&data) {
            send(AttachSnapshotChunk {
                session_id: session_id.clone(),
                attach_id,
                data: chunk,
                // ... other fields
            }).await?;
        }
    }
    
    // 6. Get current live sequence number
    let live_seq = {
        let st = state.read().await;
        st.sessions.get(&session_id)
            .map(|s| s.live_seq)
            .unwrap_or(0)
    };
    
    // 7. Send AttachReady - live chunks start flowing
    send(AttachReady {
        session_id: session_id.clone(),
        attach_id,
        last_live_seq: live_seq,
    }).await?;
    
    // 8. Register viewer for session view count
    // (needed for priority switching)
    register_viewer(state, addr, session_id).await;
    
    Ok(())
}
```

### Phase 4: Mobile/Desktop Priority Switching

#### 4.1 Priority logic
**In `daemon.rs`:**

```rust
/// Handle PTY resize with mobile/desktop priority
async fn handle_pty_resize(
    session_id: &str,
    cols: u16,
    rows: u16,
    epoch: Option<u64>,
    reason: PtyResizeReason,
    sender_addr: SocketAddr,
    state: &SharedState,
) {
    let viewer_count = get_viewer_count(state, session_id).await;
    let sender_is_mobile = is_mobile_client(state, sender_addr).await;
    
    match reason {
        PtyResizeReason::DetachRestore => {
            // Mobile disconnected - restore to desktop size
            if viewer_count == 0 {
                restore_desktop_size(state, session_id).await;
            }
        }
        _ if sender_is_mobile => {
            // Mobile resize - always apply (mobile has priority)
            apply_pty_resize(state, session_id, cols, rows, epoch).await;
        }
        _ => {
            // Desktop resize - only if no mobile viewers
            if viewer_count == 0 {
                apply_pty_resize(state, session_id, cols, rows, epoch).await;
            }
        }
    }
}

async fn restore_desktop_size(state: &SharedState, session_id: &str) {
    // Get the desktop terminal's reported size
    // If desktop is still connected, ask it for its current size
    // Otherwise, use a reasonable default (80x24 or last known desktop size)
}
```

#### 4.2 Wrapper-side size management
**In `pty_wrapper.rs`:**

The wrapper needs to:
1. Report its current terminal size to daemon on connect
2. Handle resize messages from daemon (mobile priority)
3. Save its original size before mobile connects
4. Restore original size when mobile disconnects

```rust
// In run_wrapped() main loop:
some("resize") => {
    let cols = msg["cols"].as_u64().unwrap_or(0) as u16;
    let rows = msg["rows"].as_u64().unwrap_or(0) as u16;
    
    if cols == 0 && rows == 0 {
        // Mobile disconnected - restore desktop size
        if let Some((orig_cols, orig_rows)) = saved_local_size {
            resize_pty(&master, orig_cols, orig_rows)?;
            if desktop_resize_policy == DesktopResizePolicy::Mirror {
                request_terminal_resize(orig_cols, orig_rows);
            }
        }
    } else {
        // Mobile connected - save current size if first time
        if saved_local_size.is_none() {
            saved_local_size = Some(get_terminal_size());
        }
        // Resize to mobile dimensions
        resize_pty(&master, cols, rows)?;
        if desktop_resize_policy == DesktopResizePolicy::Mirror {
            request_terminal_resize(cols, rows);
        }
    }
}
```

### Phase 5: Remove Local Echo

**In `mobile/components/TerminalView.tsx`:**

Remove entire local echo system:
- `LOCAL_ECHO_ENABLED` constant
- `localEchoPendingBytesRef`
- `localEchoAutoDisabledRef`
- `localEchoStatsRef`
- `isSafeLocalEchoInput()`
- `canUseLocalEcho()`
- `clearLocalEchoState()`
- `maybePredictLocalEcho()`
- `reconcileLocalEcho()`

Simplify to:
```typescript
// Just send input to daemon
const queueRawInput = useCallback((data: string, flushNow: boolean = false) => {
    if (!data || !onSendRawInput) return;
    inputBufferRef.current += data;
    if (flushNow || data.includes('\r')) {
        flushRawInput();
        return;
    }
    if (!inputFlushTimerRef.current) {
        inputFlushTimerRef.current = setTimeout(() => {
            inputFlushTimerRef.current = null;
            flushRawInput();
        }, 2);
    }
}, [flushRawInput, onSendRawInput]);

// In PTY callback - just write directly
setPtyBytesCallback(sessionId, (base64) => {
    xtermRef.current?.writeBase64(base64);
    // No reconciliation needed
});
```

### Phase 6: Remove Keyboard Resize Suppression

**In `mobile/components/TerminalView.tsx`:**

Remove:
- `keyboardVisibleRef`
- `keyboardTransitionUntilRef`
- `keyboardRefitTimersRef`
- Keyboard show/hide listeners
- Row-only resize suppression logic

Keep simple resize handling:
```typescript
onResize={(cols, rows) => {
    if (resizeDebounceRef.current) {
        clearTimeout(resizeDebounceRef.current);
    }
    resizeDebounceRef.current = setTimeout(() => {
        if (!resizeActiveRef.current) return;
        
        const last = lastSentDimsRef.current;
        if (last && last.cols === cols && last.rows === rows) {
            return; // Skip duplicate
        }
        
        // Always send resize - no keyboard special cases
        lastSentDimsRef.current = { cols, rows };
        sendResizeIntent(cols, rows, 'geometry_change');
    }, 120);
}}
```

### Phase 7: Mobile Protocol Cleanup

#### 7.1 Remove cli_type from types
**`mobile/hooks/useSync.ts`:**

```typescript
// REMOVE from Session interface:
interface Session {
    id: string;
    name: string;
    projectPath: string;
    createdAt: string;
    lastActiveAt: string;
    status: string;
    // REMOVE: cliType?: string;
    // KEEP (for now): runtime?: string; 
}
```

#### 7.2 Remove from session parsing
```typescript
// REMOVE from setSessions:
setSessions((data.sessions || []).map((s: any) => ({
    id: s.id || s.session_id,
    name: s.name,
    projectPath: s.project_path || s.projectPath || '',
    createdAt: s.created_at || s.createdAt || new Date().toISOString(),
    lastActiveAt: s.last_active_at || s.lastActiveAt || new Date().toISOString(),
    status: s.status || 'active',
    // REMOVE: cliType: s.cli_type || s.cliType,
    // REMOVE: runtime: s.runtime,
})));
```

#### 7.3 Remove from TerminalView props
```typescript
// REMOVE from TerminalViewProps:
interface TerminalViewProps {
    sessionId: string;
    onSendRawInput?: (text: string) => boolean | Promise<boolean>;
    // ... other props ...
    // REMOVE: sessionCliType?: string;
    // REMOVE: sessionRuntime?: string;
}
```

### Phase 8: Keep Notification Detection (Separate)

Move notification-related code to separate file, don't wire to terminal:

**`cli/src/notification.rs`** (from detection.rs):
- Keep `WaitEvent`, `WaitType`
- Keep `detect_wait_event()`
- Keep ANSI stripping utilities
- Remove CLI-specific detection (CliType, CliTracker)

**Don't wire to terminal behavior** - only use for:
- Push notifications (future)
- Session status indicators (optional)

---

## Testing Plan

### Unit Tests

1. **Terminal detection tests**
   ```rust
   #[test]
   fn detects_cursor_positioning() {
       let data = b"Hello\x1b[10;5HWorld";
       assert!(contains_cursor_positioning(data));
   }
   
   #[test]
   fn plain_text_is_safe() {
       let data = b"Hello World\nLine 2\n";
       assert!(!contains_cursor_positioning(data));
   }
   
   #[test]
   fn alternate_screen_detected() {
       let data = b"\x1b[?1049hContent\x1b[?1049l";
       assert!(contains_cursor_positioning(data));
   }
   ```

2. **Scrollback safety tests**
   ```rust
   #[test]
   fn safe_scrollback_returned() {
       // Plain text scrollback
       let session = create_session_with_scrollback(b"Hello\nWorld\n");
       assert!(get_safe_scrollback(&session).is_some());
   }
   
   #[test]
   fn unsafe_scrollback_skipped() {
       // Cursor-positioned scrollback
       let session = create_session_with_scrollback(b"\x1b[2J\x1b[HFrame");
       assert!(get_safe_scrollback(&session).is_none());
   }
   ```

### Integration Tests

1. **Codex session lifecycle**
   - Start Codex session
   - Generate some output
   - Connect mobile
   - Verify clear + redraw (no jumbling)
   - Disconnect mobile
   - Verify desktop returns to original size

2. **Gemini session lifecycle**
   - Same as Codex

3. **Vim session**
   - Open file in vim
   - Connect mobile
   - Verify vim redraws correctly
   - Scroll within vim
   - Disconnect
   - Verify desktop vim still works

4. **Plain terminal session**
   - Run `ls -la` multiple times
   - Connect mobile
   - Verify scrollback replays correctly (plain text)
   - Disconnect
   - Continue using terminal

5. **Rapid connect/disconnect**
   - Connect mobile
   - Immediately disconnect
   - Reconnect within 5 seconds
   - Verify no duplication or jumbling

### Manual Tests

1. **Visual inspection**
   - Open Codex
   - Type "tell me a story"
   - Wait for output
   - Scroll up - should see clean history
   - Background app
   - Foreground app - should see brief clear then clean redraw
   - Scroll up - should NOT see duplicated/jumbled content

2. **Input responsiveness**
   - Type quickly in terminal
   - Characters should appear smoothly (even without local echo)
   - Test on local WiFi and Tailscale

3. **Resize behavior**
   - Connect mobile
   - Rotate phone (landscape/portrait)
   - Terminal should resize smoothly
   - App (vim/codex) should redraw
   - No keyboard suppression hacks needed

---

## Rollback Plan

If major issues arise:

1. **Database/schema changes**: None (session storage is unchanged)
2. **Protocol changes**: Version negotiation can support fallback
3. **Code rollback**: Git revert to pre-overhaul state
4. **Gradual rollout**: Can gate behind feature flag if needed

---

## Success Criteria

| Criterion | Target |
|-----------|--------|
| Reconnect duplication | Zero occurrences |
| Scroll-up jumbling | Zero occurrences |
| Input latency | <100ms local, <300ms Tailscale |
| Code complexity | -30% lines of code |
| CLI detection | Zero CLI-specific code in terminal path |
| tmux dependency | Removed entirely |

---

## Timeline Estimate

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1: Remove tmux | 1-2 days | None |
| Phase 2: Safe scrollback | 1 day | Phase 1 |
| Phase 3: Simplify subscribe | 1 day | Phase 2 |
| Phase 4: Priority switching | 2 days | Phase 3 |
| Phase 5: Remove local echo | 1 day | None (mobile) |
| Phase 6: Remove keyboard suppression | 0.5 day | Phase 5 |
| Phase 7: Mobile cleanup | 1 day | Phase 5, 6 |
| Phase 8: Testing | 2-3 days | All |
| **Total** | **10-14 days** | - |

---

## File Checklist

### Backend (Rust)

- [ ] `cli/src/tmux.rs` - DELETE
- [ ] `cli/src/pty_wrapper.rs` - Remove tmux, simplify
- [ ] `cli/src/daemon.rs` - Remove tmux, CLI detection
- [ ] `cli/src/terminal.rs` - NEW: Safe replay detection
- [ ] `cli/src/notification.rs` - NEW: Separated notification code
- [ ] `cli/src/protocol.rs` - Remove cli_type, runtime from messages
- [ ] `cli/src/lib.rs` - Update module exports

### Mobile (TypeScript)

- [ ] `mobile/components/TerminalView.tsx` - Remove local echo, keyboard suppression
- [ ] `mobile/components/XTermView.tsx` - No changes (generic terminal)
- [ ] `mobile/app/session/[id].tsx` - Remove cli_type props
- [ ] `mobile/hooks/useSync.ts` - Remove cli_type from types/parsing

### Testing

- [ ] `cli/src/terminal_tests.rs` - NEW: Unit tests
- [ ] Manual testing plan execution

---

## Notes

- **Notification detection**: Keep code but don't wire to terminal behavior
- **Runtime field**: Remove from protocol - mobile doesn't need to know
- **Alt screen tracking**: Keep in daemon (from PTY output) - used for clear sequences
- **Scrollback buffer**: Keep daemon-managed, but filter before replay

---

## Approval

This plan requires approval before implementation begins due to its scope.

**Approved by:** _______________ **Date:** _______________

**Implementation start:** _______________ **Target completion:** _______________
