# TUI Rendering Bug: Desktop Screen Clearing & Content Loss During Mobile Resize

## TL;DR

MobileCLI lets you view terminal sessions simultaneously on desktop (Konsole) and mobile (React Native xterm.js). TUI apps like **Codex, Kimi, OpenCode** that use the alternate screen buffer (`\x1b[?1049h`) suffer from:

1. **Ghost duplicate UI elements** on the desktop terminal (Konsole reflow artifacts)
2. **Content disappearing** when keyboard shows/hides on mobile
3. **Chat history getting chopped** — earlier messages vanish, blank space appears at top

**Claude Code and Gemini work fine** because they use the main screen buffer (streaming text + ANSI colors), not alt-screen TUI rendering.

Multiple fix attempts over several sessions have failed. The core tension: we need `\x1b[2J\x1b[H` (clear screen) to prevent Konsole reflow ghosts on width changes, but that same clear wipes TUI content between the clear and the app's SIGWINCH redraw. The latest attempt (gate clear on width-only changes) did NOT fix it.

---

## Architecture Overview

```
┌─────────────┐     WebSocket      ┌──────────┐     WebSocket      ┌─────────────────┐
│  Mobile App  │ ◄────────────────► │  Daemon   │ ◄────────────────► │  PTY Wrapper     │
│  (xterm.js)  │  resize/input/pty  │ (Rust)    │  register/output   │  (Rust, spawns   │
│  React Native│  subscribe/ack     │ port 9847 │                    │   child process) │
└─────────────┘                     └──────────┘                     └────────┬────────┘
                                         │                                    │
                                    alt-screen                           ┌────▼────┐
                                    tracking                             │ PTY     │
                                    scrollback                           │ master  │
                                    buffer (64KB)                        │         │
                                                                         └────┬────┘
                                                                              │
                                                                    ┌────────▼────────┐
                                                                    │  Child Process   │
                                                                    │  (codex, kimi,   │
                                                                    │   opencode, etc) │
                                                                    └─────────────────┘
                                                                              │
                                                                    Desktop terminal (Konsole)
                                                                    also shows PTY output
                                                                    (simultaneous viewing)
```

**Key constraint: Desktop and mobile MUST show the session simultaneously.** The PTY wrapper writes all output to both stdout (desktop terminal) AND the daemon (which forwards to mobile). When mobile connects, the desktop terminal is physically resized to match mobile dimensions via `request_terminal_resize()` so both views render identically.

---

## File Locations

| File | Purpose | Key Lines |
|------|---------|-----------|
| `cli/src/pty_wrapper.rs` | PTY spawn, resize handling, screen clear logic | L97-106 (request_terminal_resize), L314-315 (saved_local_size, last_mobile_cols), L364-416 (resize handler) |
| `cli/src/daemon.rs` | WebSocket hub, alt-screen tracking, scrollback, SubscribeAck | L119-137 (PtySession struct), L617-638 (alt-screen detection), L1248-1297 (Subscribe + SubscribeAck handler), L1325-1348 (PtyResize forwarding) |
| `cli/src/protocol.rs` | WebSocket message types | ClientMessage::PtyResize, ServerMessage::SubscribeAck |
| `mobile/components/TerminalView.tsx` | xterm.js wrapper, resize debounce, alt-screen entry | L149-151 (debounce refs), L221-237 (late subscribe_ack fallback), L622-646 (onReady + alt-screen fast path), L652-670 (onResize 300ms debounce) |
| `mobile/hooks/useSync.ts` | Zustand store, WS connection, pty_bytes suppression | L161-162 (altScreenSessions, suppressPtyUntilResize), L558-567 (subscribe_ack handler), L569-582 (pty_bytes suppression), L925-931 (sendPtyResize) |
| `mobile/app/session/[id].tsx` | Session screen, subscribe/unsubscribe lifecycle | L91-101 (resize 0,0 on background, re-subscribe on resume) |

---

## The Resize Data Flow

```
1. Mobile keyboard shows/hides
   → React Native KeyboardAvoidingView changes layout
   → XTermView.handleLayout fires (has its own 150ms debounce, filters >100px height changes)
   → refitTerminal() calls fitAddon.fit() + notifySizeChange()

2. TerminalView.onResize fires
   → 300ms debounce timer
   → Deduplicates: skips if cols+rows unchanged from last send
   → Calls onPtyResize(sessionId, cols, rows)

3. useSync sends WebSocket message: { type: "pty_resize", session_id, cols, rows }

4. Daemon receives PtyResize
   → Checks session has active viewers
   → Forwards via session.resize_tx channel

5. PTY wrapper receives resize in event loop (pty_wrapper.rs L364)
   → If cols=0, rows=0: mobile disconnected → restore saved desktop size
   → If non-zero: mobile active →
     a. Save desktop size (first time only)
     b. [BUG AREA] Optionally clear screen (\x1b[2J\x1b[H)
     c. request_terminal_resize(cols, rows) → sends \x1b[8;rows;cols;t + TIOCSWINSZ ioctl
     d. master.resize(PtySize) → child gets SIGWINCH → TUI redraws

6. TUI app (Codex etc) receives SIGWINCH
   → Queries new terminal size
   → Redraws entire screen at new dimensions
   → Output flows through PTY → pty_wrapper → stdout (desktop) + daemon → mobile
```

---

## What's Been Tried (All Failed or Partially Failed)

### Attempt 1: Suppress desktop stdout when mobile is connected
- **What:** Stop writing PTY output to desktop stdout while mobile is viewing
- **Result:** REJECTED. User requires simultaneous desktop+mobile viewing. Also didn't fix duplicates.

### Attempt 2: SubscribeAck + alt-screen suppression + timing fix
- **What:**
  - Daemon sends `SubscribeAck { in_alt_screen }` after `Subscribe`
  - Mobile suppresses stale PTY bytes until resize is sent (prevents desktop-sized bytes garbling xterm.js)
  - Mobile enters alt-screen locally (`\x1b[?1049h`) before bytes flow
  - Fixed timing: clear suppression AFTER resize sent, not before
- **Result:** Improved mobile rendering. Desktop still shows ghost duplicates in Konsole.

### Attempt 3: Clear screen before every resize (`\x1b[2J\x1b[H`)
- **What:** Before `request_terminal_resize()`, send clear screen + cursor home to desktop stdout
- **Result:** Fixed Konsole ghost duplicates BUT content disappears. Every keyboard show/hide triggers clear, wiping the screen. Between the clear and the SIGWINCH redraw, the terminal is blank. With rapid keyboard toggles, the TUI keeps getting wiped.

### Attempt 4: Width-only clear tracking (current state, `last_mobile_cols`)
- **What:** Track `last_mobile_cols: Option<u16>`. Only clear screen when WIDTH changes. Keyboard show/hide only changes height → skip clear.
- **Result:** STILL BROKEN. User reports "still fucked." The clear-on-width-change fires on first connect (correct) but something else is still causing the rendering issues.

---

## Current State of the Code (git diff)

The only uncommitted change is in `cli/src/pty_wrapper.rs`:

```diff
+    let mut last_mobile_cols: Option<u16> = None;

     // Mobile disconnect path:
+    last_mobile_cols = None;
+    if std::io::stdout().is_terminal() {
         let _ = stdout.write_all(b"\x1b[2J\x1b[H");
         let _ = stdout.flush();
+    }

     // Mobile active path:
+    if last_mobile_cols != Some(c) {
+        if std::io::stdout().is_terminal() {
+            let _ = stdout.write_all(b"\x1b[2J\x1b[H");
+            let _ = stdout.flush();
+        }
+        last_mobile_cols = Some(c);
+    }
```

---

## Screenshots Documenting the Bugs

All screenshots are from 2026-02-22, testing with OpenAI Codex (v0.104.0) via `mobilecli codex` on Konsole (KDE, Linux).

### Bug 1: Ghost Duplicate Chat Boxes (Desktop/Konsole)

**`~/Pictures/Screenshot_20260222_221914.png`** — New session opened. "Implement {feature}" prompt box appears TWICE immediately. Two chat input boxes rendered, identical, stacked vertically. 100% context left on both. This is a fresh session — no prior interaction.

**`~/Pictures/Screenshot_20260222_221846.png`** — Existing session with history. Multiple prompts visible (okay testing, test, hmmmm, Explain this codebase). "Explain this codebase" appears TWICE — duplicate chat box. Header shows OpenAI Codex banner + Connected message still visible at top.

**`~/Pictures/Screenshot_20260222_221813.png`** — Same session. Duplicate "Explain this codebase" entries. Left edge shows single-character fragments from a previous wider render bleeding through (reflow artifacts: partial characters like "l", "t", "d", etc. visible in left gutter).

**`~/Pictures/Screenshot_20260222_221830.png`** — Scrolled view. Shows the duplicate accumulation: "Explain this codebase" appears three times total after multiple re-entries.

### Bug 2: Content Disappearing After Keyboard Toggle (Desktop/Konsole)

**`~/Pictures/Screenshot_20260222_223430.png`** — After extended use. Chat history is "chopped off" — only the Codex header and a single "test" input box visible. Previous prompts that were sent are gone. Large blank area in the middle of the screen.

**`~/Pictures/Screenshot_20260222_223535.png`** — New session opened in fresh Konsole tab. Shows full Codex UI correctly (header, tip, "Implement {feature}" prompt). This is the "before" state.

**`~/Pictures/Screenshot_20260222_223658.png`** — SAME session as above, after simply collapsing the mobile keyboard. Most of the Codex UI has vanished. Only the shell prompt (`codex`), a thin border fragment, and the "Implement {feature}" box remain. The entire header, tip text, and model info disappeared.

**`~/Pictures/Screenshot_20260222_223946.png`** — After further chatting (test, test 2, tell me a story). The Codex UI is rendering but the header is gone — content starts from a thin border at the top. Chat history is present from "test" onward but earlier content is lost.

### Bug 3: Mobile View — Blank Space at Top

**`~/Pictures/photo_2026-02-22_22-40-16.jpg`** — Mobile (iPhone) view of the same session. Shows the session title "Shell", connection indicators (ws + active). Large blank/dark area at the top of the terminal. Chat content starts far down the screen with the "A mechanic found an old flip phone..." story response. The prompt history before this point is gone. The blank space suggests the terminal buffer has been cleared/corrupted above the visible content.

---

## Why Previous Fixes Are Insufficient

The fundamental problem is the interaction between THREE systems:

1. **Konsole's reflow behavior**: When the terminal window physically resizes (via `\x1b[8;r;c;t` or TIOCSWINSZ), Konsole reflows existing content. For alt-screen TUI apps with absolute-positioned elements (box drawing chars at specific row/col), this reflow creates ghost duplicates because Konsole tries to word-wrap content that was never meant to wrap.

2. **`\x1b[2J\x1b[H` timing**: The clear screen prevents reflow ghosts, but there's a window between the clear and the TUI app's SIGWINCH redraw where the screen is blank. If this happens during rapid resize events (keyboard animations), the user sees content vanishing.

3. **Mobile keyboard resize cascade**: Each keyboard show/hide can trigger 1-2 resize events (after debouncing). The 300ms debounce helps but doesn't eliminate the problem. Each resize flows through the full chain: mobile → daemon → PTY wrapper → terminal clear → PTY resize → SIGWINCH → TUI redraw.

### Ideas NOT Yet Tried

- **Don't physically resize the desktop terminal at all** — only resize the PTY. Desktop would show narrower content in a wider window but no reflow artifacts. (Downside: desktop output may not match mobile exactly)
- **Use `\x1b[?1049h` / `\x1b[?1049l` (alt-screen toggle) instead of `\x1b[2J`** — switch to alt screen before resize, switch back after. This is how real terminal multiplexers (tmux/screen) handle it.
- **Buffer PTY output during resize** — hold output for ~50ms after resize to let the TUI finish redrawing, then flush. Prevents the blank-then-redraw flicker.
- **Send SIGWINCH without clearing** — just resize the PTY and let the TUI handle it. Accept that Konsole may show brief reflow artifacts that get overwritten by the TUI's own redraw.
- **Detect if the child is in alt-screen** from the PTY wrapper side (not just daemon) and only clear for alt-screen sessions. For main-screen apps (Claude Code), never clear.
- **Rate-limit resizes in the PTY wrapper** — collapse rapid resize events into one. The mobile side debounces at 300ms but the PTY wrapper processes every resize it gets.

---

## Reproducing the Bug

1. Start daemon: `mobilecli daemon` (or it auto-starts)
2. In Konsole: `mobilecli codex` (or `mobilecli --name "Test" codex`)
3. Connect from mobile app → subscribe to the session
4. On mobile: show keyboard → type something → hide keyboard → show keyboard → hide keyboard
5. Observe desktop Konsole: content disappears/reappears, duplicate boxes accumulate
6. On mobile: scroll up — blank space at top, chat history truncated

---

## Key Constraints

- Desktop and mobile MUST view the session simultaneously (not either/or)
- Desktop terminal is physically resized to match mobile dimensions for identical output
- The daemon's alt-screen tracking + SubscribeAck + mobile suppression logic is CORRECT and should be kept
- The scrollback replay for non-alt-screen apps (Claude Code) works correctly
- The 300ms debounce on mobile resize is already in place
- The `is_terminal()` guard prevents escape sequences in headless/daemon mode
- Solution must work for any TUI app, not just Codex specifically
