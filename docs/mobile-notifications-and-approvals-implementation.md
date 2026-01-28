# Mobile Notifications + Tool Approval: Implementation Document

Date: 2026-01-28
Owner: MobileCLI
Target: mobile app (Expo) + desktop daemon (Tauri) integration

## 0. Purpose and scope
This document specifies how the mobile app should handle:
- Notifications when CLI agents are **done working** or **awaiting user response**.
- Tool-approval acceptance modals on mobile (approve/deny/always) with accurate CLI-specific input.

It is written to be **directly actionable** for implementation, and intentionally includes full context, current gaps, message schemas, and step-by-step tasks. The goal is a robust, unified notification + waiting-state system across the WS protocol, mobile UI, and desktop daemon.

## 1. Current state (as of 2026-01-28)

### 1.1 Desktop daemon emits waiting events and pushes
The desktop daemon (Tauri, `desktop/src-tauri/src/ws.rs`) listens to PTY/parser events and emits:
- `waiting-for-input` (with `waitType`, prompt content, cli type)
- `waiting-cleared`

It also triggers **push notifications** on `waiting-for-input` via `send_push_notifications(...)`.

Key locations:
- `desktop/src-tauri/src/pty.rs`: emits `waiting-for-input` after parser detects waiting state.
- `desktop/src-tauri/src/ws.rs`: forwards `waiting-for-input` to WS clients and also triggers push notifications.
- `desktop/src-tauri/src/lib.rs`: emits `waiting-cleared` on tool approval or raw input that looks like approval.

### 1.2 Mobile WS client is on legacy protocol
`mobile/hooks/useSync.ts` only handles:
- `sessions`, `session_info`, `session_renamed`, `session_ended`, `session_deleted`
- `pty_bytes`, `pty_resized`, `welcome`

The current WS server now emits:
- `session_created`, `session_resumed`, `session_closed`
- `waiting_for_input`, `waiting_cleared`
- `activity`, `input_state`, `new_message`

These are ignored in mobile today, blocking tool approval modals and waiting-state notifications.

### 1.3 Mobile push token never reaches server
`usePushNotifications` gets an Expo/APNS/FCM token, but the app **never sends it** to the desktop daemon (which expects `register_push_token`). As a result, push notifications never reach mobile.

### 1.4 Notification handler duplication
- `mobile/app/_layout.tsx` sets the notification handler at module load.
- `mobile/hooks/useNotifications.ts` has a lazy handler for iOS 26 compatibility.

This is inconsistent and can lead to subtle runtime issues and duplicate settings.

### 1.5 No tool-approval UI on mobile
The mobile app has **no approval modal** and no UI to present tool approvals. All approvals must be typed manually in the terminal, defeating the purpose of notification and modal workflows.

## 2. Design goals

1. **Reliable waiting-state visibility**
   - Mobile should know when a session is awaiting input, tool approval, or has a clarifying question.
   - Waiting state should be consistently synced across reconnects.

2. **Tool-approval modal parity**
   - Mobile must show a modal when `waitType` is `tool_approval` or `plan_approval`.
   - The modal should allow `Approve`, `Always`, and `Deny` (if supported by CLI).

3. **Notifications that fire at the right time**
   - Notify the user when a session requires input or approval.
   - Avoid spamming when the user is already viewing the session.
   - Support both local and push notifications.

4. **Protocol coherence**
   - Mobile should support the current WS message schema and stop using legacy names.

5. **Extensible waiting-state UX**
   - Future wait types should be handled without major rewrites.

## 3. Source-of-truth behavior (desktop)

### 3.1 Waiting detection and typing
Desktop parser emits `waiting-for-input` for:
- tool approval prompt
- clarifying question
- plan approval
- generic “awaiting response”

`waitType` mapping comes from prompt detection in `desktop/src-tauri/src/pty.rs`.

### 3.2 Clearing waiting state
Desktop emits `waiting-cleared` when:
- user sends tool approval response
- manual input is sent in approval-like patterns

### 3.3 Push notifications
Desktop sends push notifications on `waiting-for-input` with a message title/body based on:
- `waitType`
- `cliType`

This is **server-driven** and should remain the primary push mechanism.

## 4. Required changes (high-level)

1. Update mobile WS client to handle the new protocol events.
2. Add a waiting-state store on mobile (session-scoped).
3. Implement a **Tool Approval Modal** on mobile driven by waiting state.
4. Wire push token registration to the WS server.
5. Consolidate notification handler setup and toggle logic.

## 5. Protocol contract (authoritative for mobile)

### 5.1 Server → Client messages used by mobile
These are emitted by `desktop/src-tauri/src/ws.rs` and must be handled:

#### `sessions`
```json
{
  "type": "sessions",
  "sessions": [{
    "id": "...",
    "name": "...",
    "project_path": "...",
    "created_at": "...",
    "last_active_at": "...",
    "status": "active|idle|closed",
    "cli_type": "claude|gemini|opencode|codex"
  }]
}
```

#### `session_created` / `session_resumed` / `session_closed` / `session_renamed` / `session_deleted`
```json
{ "type": "session_created", "session": { ... } }
{ "type": "session_resumed", "session": { ... } }
{ "type": "session_closed", "session_id": "..." }
{ "type": "session_renamed", "session_id": "...", "new_name": "..." }
{ "type": "session_deleted", "session_id": "..." }
```

#### `waiting_for_input`
```json
{
  "type": "waiting_for_input",
  "session_id": "...",
  "timestamp": "2026-01-28T12:34:56Z",
  "prompt_content": "...",
  "wait_type": "tool_approval|plan_approval|clarifying_question|awaiting_response",
  "cli_type": "claude|gemini|opencode|codex"
}
```

#### `waiting_cleared`
```json
{
  "type": "waiting_cleared",
  "session_id": "...",
  "timestamp": "...",
  "response": "1|2|3|y|n|<esc sequences>"
}
```

#### `activity`
```json
{
  "type": "activity",
  "session_id": "...",
  "activity_type": "tool_start|tool_result|text|user_prompt|thinking|...",
  "content": "...",
  "tool_name": "Bash",
  "tool_params": "{...}",
  "file_path": "...",
  "is_streaming": false,
  "timestamp": "...",
  "uuid": "...",
  "source": "pty|jsonl"
}
```

#### `input_state`
```json
{
  "type": "input_state",
  "session_id": "...",
  "text": "...",
  "cursor_position": 12,
  "sender_id": "desktop|mobile-...",
  "timestamp": 123456789
}
```

#### `new_message`
```json
{
  "type": "new_message",
  "session_id": "...",
  "role": "user|assistant|tool",
  "content": "...",
  "tool_name": "...",
  "is_complete": true,
  "client_msg_id": "..."
}
```

### 5.2 Client → Server messages used by mobile

#### Register push token
```json
{
  "type": "register_push_token",
  "token": "...",
  "token_type": "expo|apns|fcm",
  "platform": "ios|android"
}
```

#### Tool approval (recommended approach)
Mobile can **either** send raw PTY input (`send_input`) or add a new explicit type.
Current server already supports **raw `send_input`**, and it can map approval inputs via `ApprovalResponse`.

Two options:
1) **Raw input** (simpler, existing):
```json
{
  "type": "send_input",
  "session_id": "...",
  "text": "1",
  "raw": true
}
```
2) **Add a new WS message for approval** (preferred for clarity):
```json
{
  "type": "tool_approval",
  "session_id": "...",
  "response": "yes|yes_always|no"
}
```
This second option does NOT exist in the WS protocol yet, but it’s easy to add (server already has `send_tool_approval`).

## 6. Mobile state model (recommended)

Add a `waitingStates` store keyed by `sessionId`:
```ts
interface WaitingState {
  sessionId: string;
  waitType: 'tool_approval' | 'plan_approval' | 'clarifying_question' | 'awaiting_response';
  promptContent?: string;
  cliType?: 'claude' | 'gemini' | 'opencode' | 'codex';
  timestamp: string;
}
```

Store should live in `useSync` or a sibling hook such as `useWaitingState`.

## 7. Tool approval modal design

### 7.1 Trigger
Show modal when:
- waiting state exists AND `waitType === tool_approval` (or `plan_approval`).
- session is active and user is viewing session.

### 7.2 Data to display
Minimal:
- Title: “Tool Approval Needed”
- Subtitle: CLI label (`cliType` if known)
- Prompt snippet (prompt_content)

Preferred:
- Show tool summary from latest `activity` event with `summary` (if available)
- Or show the latest tool_start content / tool_name + tool_params

### 7.3 Actions
Buttons:
- **Approve**
- **Always**
- **Deny**

If CLI lacks “Always” option (yes/no model), still allow Always but map to “Yes”.

### 7.4 Sending response
Option A (immediate):
- Send `send_input` raw using correct CLI model mapping.
- This triggers `waiting-cleared` on the server (already implemented).

Option B (cleaner):
- Add WS message `tool_approval` and let server map to correct input.
- Server will emit `waiting-cleared` and keep mobile in sync.

### 7.5 Modal dismissal
Modal closes when:
- `waiting_cleared` is received for that session
- Session changes or closes

## 8. Notifications

### 8.1 Push notifications (server-driven)
- Desktop daemon triggers push on `waiting-for-input`
- Mobile must send push token (`register_push_token`) on startup and when tokens rotate

### 8.2 Local notifications (mobile-driven)
- Mobile can optionally show local notifications for in-app events (e.g. foreground waiting state) but must avoid double-notify if push already happens.
- Prefer: only show local notification if app is foregrounded AND push is not delivered (or if user disabled push but kept local).

### 8.3 Suppression rules
Suppress notifications if:
- User is currently viewing the same session AND app is active

### 8.4 Settings toggle
The `notifications` setting should control:
- push token registration
- local notification display

## 9. Implementation tasks (step-by-step)

### 9.1 Update WS client handling in mobile
Files: `mobile/hooks/useSync.ts`

- Add handling for:
  - `session_created`, `session_resumed`, `session_closed`
  - `waiting_for_input`, `waiting_cleared`
  - `activity`, `input_state`, `new_message`
- Update session model to include `cliType`.
- Replace legacy names (`session_info`, `session_ended`) with current ones.

### 9.2 Add waiting state store
Create `mobile/hooks/useWaitingState.ts` or extend `useSyncStore`.

State:
- `waitingStates: Record<sessionId, WaitingState | null>`
- `setWaitingState(sessionId, state | null)`

### 9.3 Tool approval modal UI
File: `mobile/app/session/[id].tsx`

- Subscribe to waiting state for the active session.
- Render modal when `waitType` is tool_approval or plan_approval.
- Provide buttons with callbacks.
- Parse tool summary from latest `activity` list if available.

### 9.4 Send approval input to server
Option A: raw input
- Map response to correct CLI input (numbered / arrow / y/n).
- Send via `send_input` with `raw: true`.

Option B: add new WS client message
- Add `tool_approval` client message in WS protocol.
- Server should call `send_tool_approval`.

### 9.5 Push token registration
File: `mobile/app/_layout.tsx`

- On token received, send WS message:
```ts
send({ type: 'register_push_token', token, token_type, platform })
```
- Only do this when `settings.notifications === true`.

### 9.6 Consolidate notification handler
Pick **one** path:
- Either keep `_layout.tsx` module-level `Notifications.setNotificationHandler`,
- Or remove it and use the lazy version in `useNotifications`.

For iOS 26 safety, prefer **lazy init** and remove the module-level call.

### 9.7 Notification routing
- Ensure `notificationResponseListener` is registered **once**.
- Deduplicate logic in `usePushNotifications` vs `_layout`.

### 9.8 Update `shared/protocol.ts`
Optionally update the shared protocol file so it reflects the WS server now.
This helps mobile stay aligned with server changes.

## 10. Edge cases and correctness requirements

1. **Reconnect state**
   - On subscribe, server emits current waiting state (via `request-waiting-state`).
   - Mobile must listen to `waiting_for_input` and `waiting_cleared` and update state even if modal currently hidden.

2. **Session switching**
   - If user switches sessions, modal must show only for current session.
   - If waiting cleared for another session, do not close modal.

3. **Duplicate notifications**
   - Prevent multiple notification handlers and repeated event subscriptions.

4. **Push token rotation**
   - Re-register token when app becomes active.
   - Do not re-register if notifications disabled.

5. **CLI-specific approval mapping**
   - Use `cli_type` to select approval model.
   - Default to numbered options if unknown.

6. **App foreground state**
   - If app is active and user is in session, suppress notifications.

## 11. Suggested UI layout for modal

- Semi-transparent full-screen overlay.
- Title: “Tool Approval Required”
- Session name + CLI type label.
- Tool summary / prompt snippet in monospace block.
- Buttons in row: Approve (green), Always (blue), Deny (red).

## 12. Recommended data-flow summary (happy path)

1. CLI agent asks for tool permission.
2. Parser detects waiting state, emits `waiting-for-input`.
3. WS server forwards waiting event to mobile and pushes notification.
4. Mobile shows modal (if session is active) and notification (if not). 
5. User taps Approve → mobile sends response. 
6. Desktop emits `waiting-cleared` → mobile dismisses modal.

## 13. Implementation checklist

- [ ] Update `useSync` to parse new WS message types.
- [ ] Add waiting state store and wire `waiting_for_input` / `waiting_cleared`.
- [ ] Tool approval modal UI in session screen.
- [ ] Map approval response to CLI-specific input.
- [ ] Send approval response over WS.
- [ ] Register push token on mobile.
- [ ] Remove duplicate notification handlers.
- [ ] Gate all notifications by user settings.
- [ ] Manual test: approval flow for Claude, Codex, OpenCode.

## 14. Notes for Claude (implementation guidance)

- Prefer minimal changes to the WS protocol if possible, but avoid raw input hacks if a clean `tool_approval` message is trivial to add.
- Keep modal logic in session screen so it’s scoped by session.
- Log all waiting-state transitions in dev builds to debug state transitions.

---

End of document.
