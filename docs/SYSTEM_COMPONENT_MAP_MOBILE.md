# Mobile System Component Map

Last updated: 2026-02-14
Scope: `mobile/` Expo React Native app (routing, session terminal UX, premium/paywall, file browser).

## 1. App Layers

- Routing/UI layer: `mobile/app/**`
- Reusable UI components: `mobile/components/**`
- State + side-effect hooks: `mobile/hooks/**`
- Utilities/protocol normalization: `mobile/utils/**`

## 2. Navigation Surface

- `app/_layout.tsx`
  - Root stack setup, push notification handler bootstrap, global push token registration callbacks.
- `app/(tabs)/index.tsx`
  - Sessions home, spawn flows, connect/discovery affordances.
- `app/session/[id].tsx`
  - Live terminal session screen, subscribe/unsubscribe lifecycle, attachment upload bridge.
- `app/(tabs)/files.tsx` -> `components/files/FileBrowser.tsx`
  - Premium-gated file browser root.
- `app/file/[...path].tsx`
  - File viewer/editor with mime-aware rendering strategy.
- `app/file/search.tsx`
  - Name/content search UI.
- `app/folder-picker.tsx`
  - Folder selection for spawn working directory.
- `app/paywall.tsx`
  - RevenueCat purchase + restore UX.
- `app/(tabs)/settings.tsx`
  - Connection management, multi-device selection, premium status/actions.

## 3. Core Hook Responsibilities

## `hooks/useSync.ts`
- Owns websocket lifecycle + reconnect backoff + heartbeat.
- Maintains session list and waiting states in global Zustand store.
- Bridges all file-system request/response correlation (`sendFileSystemRequest`).
- Maintains push token register/unregister state.
- Exposes session operations: subscribe/unsubscribe, send input, resize, spawn session.

## `hooks/useFileSystem.ts`
- Files UI state (current path, entries, selection, clipboard, history, cache).
- Executes filesystem RPCs via `useSync` layer.
- Supports full reads, chunked reads, partial reads, writes, copy/cut/paste, watcher updates.

## `hooks/usePremium.ts`
- RevenueCat initialization and entitlement state.
- Package discovery for yearly + lifetime plans.
- Purchase paths: annual, lifetime, restore.
- Listener management for customer info updates.
- Computes `hasFileAccess` (entitlement or valid trial window).

## `hooks/useDevices.ts` + `hooks/useActiveConnection.ts`
- Multi-device storage and active device selection.
- Legacy settings migration to linked-device model.
- Resolves effective `serverUrl` + `authToken` source.

## `hooks/usePushNotifications.ts`
- Token acquisition and response handling for expo/apns/fcm.

## 4. Terminal + Attachment Pipeline

## Entry points
- `components/TerminalView.tsx`
- `app/session/[id].tsx` (`handleUploadAttachment`)

## Flow
1. User taps paperclip in terminal toolbar.
2. Premium gate:
   - if locked -> route to `/paywall?autoclose=1`
3. User chooses camera/photo/files picker.
4. Local size precheck + base64 read with hard 50MB cap.
5. `onUploadAttachment` sends `upload_file` RPC through `sendFileSystemRequest`.
6. Daemon returns saved desktop path.
7. Path is inserted into terminal input stream.

## Hardening now in place
- Size precheck before read where metadata available.
- Post-read size check fallback.
- Upload timeout scales with payload size (up to 300s).
- Explicit upload failures surfaced via alerts/haptics.

## 5. File Viewer Strategy (`app/file/[...path].tsx`)

- Detects file type using entry mime/name.
- Paths:
  - Text/code/markdown -> utf8 read + editor/viewer paths.
  - Image/PDF -> base64 read + dedicated viewer.
  - Large binary/PDF -> partial or chunked reads with guarded UX.
- Edit constraints:
  - Only utf8 + <= 5MB editable.
  - Save writes through filesystem RPC.
  - Undo/redo and dirty-state guards.

## 6. Premium/Paywall Flow

## Gate points
- File browser/search/viewer routes.
- Attachment action in terminal toolbar.

## Paywall handlers (`app/paywall.tsx` + `hooks/usePremium.ts`)
- `startSubscription`
- `buyLifetime`
- `restorePurchases`

## Audit updates in this pass
- Added robust package discovery across all RevenueCat offerings (not only `current`).
- Added fallback refresh before purchase when package is not cached.
- Added already-owned/subscribed handling:
  - sync purchases
  - refresh entitlements
  - clear success path if entitlement present
  - explicit mapping warning if purchase exists but entitlement does not.
- Paywall buttons now reflect package availability instead of silent no-op behavior.

## 7. High-Signal Function Index

## `hooks/useSync.ts`
- `sendFileSystemRequest`, `subscribeToFileChanges`, `subscribeToConnectionState`
- `setGlobalPushToken`, `clearGlobalPushToken`
- `useSync`, `getConnectedDevice`

## `hooks/useFileSystem.ts`
- `initialize`, `navigate`, `refresh`, `goBack`, `goForward`, `goToParent`, `goHome`
- `createFile`, `createFolder`, `rename`, `deletePath`, `deleteSelected`, `paste`
- `readFile`, `readFileChunked`, `readFilePartial`, `writeFile`, `getFileInfo`, `search`

## `hooks/usePremium.ts`
- `initialize`, `refreshEntitlements`, `startSubscription`, `buyLifetime`, `restorePurchases`
- `usePremium`

## `components/TerminalView.tsx`
- `uploadAttachment`, `readAttachmentBase64`, `attachFromCamera`, `attachFromPhotoLibrary`, `attachFromFiles`, `handleAttachmentPress`

## 8. Current Risks / Follow-ups

- Expo doctor reports dependency/version drift vs SDK 54 expected versions.
- Non-CNG app config sync warning: native dirs present + app.json native properties.
- `react-native-markdown-display` transitively depends on vulnerable `markdown-it` advisory (moderate, no upstream fix available currently).

