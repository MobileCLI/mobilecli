# MobileCLI File Browser & Editor - Implementation Plan

> **Status**: Drafting
> **Owner**: Codex (implementation)
> **Last Updated**: 2026-01-30
> **Source Spec**: `docs/FILE_BROWSER_SPECIFICATION.md`

## 0. Goals (from spec)
- Full remote file browser/editor in MobileCLI: browse, view, edit, and manage files on the desktop from mobile.
- P0: directory browsing, file viewing, code editing, file operations.
- P1: search, real-time sync.
- P2: git status indicators, offline caching.
- Security model: sandboxed file access, deny sensitive files, safe symlink handling.
- Performance targets: low-latency listing/read/write; 60 FPS scrolling.

## 1. Current State Summary
- Daemon WebSocket server exists (`cli/src/daemon.rs`) with a JSON protocol for sessions only (`cli/src/protocol.rs`).
- Mobile app uses a global WebSocket (`mobile/hooks/useSync.ts`) for sessions; no file browser features exist.
- No filesystem module in CLI or mobile store/UI.

## 2. Implementation Strategy (phased)

### Phase 1 - Foundation (protocol + daemon core)
**Goal:** Implement secure file system operations and protocol wiring, minimal mobile plumbing.

**Daemon (Rust)**
- Add `cli/src/filesystem/` module tree:
  - `config.rs`, `security.rs`, `operations.rs`, `search.rs`, `watcher.rs`, `mime.rs`, `platform.rs`.
- Implement secure path resolution:
  - Separate **existing-path** validation vs **new-path** resolution (create/rename targets).
  - Enforce allowed roots, deny patterns, and symlink escape prevention.
- Implement core ops: list, read, write, create dir, delete, rename, copy, get info.
- Implement MIME detection and text/binary detection.
- Add audit logging hooks for file ops.

**Protocol**
- Extend `ClientMessage`/`ServerMessage` for filesystem requests/responses.
- Add request correlation ID to every FS request/response.
- Define chunked read message types and wiring.

**Mobile**
- Create a shared WebSocket request layer for filesystem messages.
- Add a minimal file system store (Zustand) with list + read + write support.

**Acceptance Criteria**
- `list_directory`, `read_file`, `write_file` functional via WebSocket.
- Path validation prevents traversal and sensitive file access.

---

### Phase 2 - Core UX (file browser tab + basic editors)
**Goal:** Full browsing and basic editing on mobile.

**Mobile UI**
- Add `Files` tab (`mobile/app/(tabs)/files.tsx`) and navigation.
- Implement components:
  - `FileBrowser`, `BreadcrumbNav`, `FileList`, `FileListItem`, `CreateModal`, `RenameModal`, `DeleteConfirm`, `FileInfoSheet`, `SortMenu`, `EmptyState`, `LoadingState`.
- Implement text viewer + lightweight editor (plain text).

**Daemon**
- Support directory sorting fields + hidden files.
- Return `FileEntry` metadata (size, mtime, is_directory, mime).

**Acceptance Criteria**
- Browse directories, open file viewer, edit and save text files.
- File operations: create, rename, delete, copy/move.

---

### Phase 3 - Enhanced Viewing
**Goal:** Specialized viewers and improved editing.

- Markdown viewer
- Image viewer (zoom)
- PDF viewer
- Hex viewer for binary files
- Monaco editor via WebView (or alternative if constraints require)

**Acceptance Criteria**
- Viewer chosen based on MIME/ext.
- Large or binary files handled gracefully (read-only, hex).

---

### Phase 4 - Advanced Features
**Goal:** Search, watching, clipboard, multi-select.

**Daemon**
- Search: filename + content, with limits and truncation.
- Watch: per-directory watcher with debouncing and event delivery.

**Mobile**
- Search screen + results UI.
- Clipboard (copy/cut/paste), multi-select mode.
- Real-time updates applied to lists.

**Acceptance Criteria**
- Search works with depth + size limits.
- File changes appear without manual refresh.

---

### Phase 5 - Polish and Performance
**Goal:** Caching, optimizations, accessibility, tests, docs.

- Offline cache with size/age limits and invalidation.
- Performance tuning: FlashList, memoization, batching.
- Accessibility: labels, focus announcements.
- Tests: daemon unit tests, mobile hook tests, and E2E smoke.

---

## 3. Dependencies
**Rust**
- `walkdir`, `ignore`, `notify`, `notify-debouncer-mini`, `path_jail`, `infer`, `memmap2`, `dashmap`, `rayon`, `base64`, `md5`, `glob-match`, `chrono`.

**Mobile**
- `@shopify/flash-list`, `react-native-webview`, `react-native-pdf`, `react-native-markdown-display`, `expo-haptics`, `@d11/react-native-fast-image` (or alt if Expo constraints require).

## 4. Key Decisions & Risks
- **Protocol envelope vs legacy**: FS ops will add request/response correlation; keep session protocol unchanged to avoid breaking existing apps.
- **Symlink handling**: Must ensure symlink target validation for both existing and new paths.
- **Monaco**: WebView + CDN may be heavy; consider bundling or alternative editor if offline requirements are strict.
- **Watchers**: Platform differences and event noise; implement debounced + path-filtered events.

## 5. Definition of Done
- All P0/P1 features implemented and functional across macOS/Linux/Windows.
- Security model enforced, with denied patterns and path sandboxing.
- Mobile UX meets navigation, selection, and editing requirements.
- Performance targets reasonably met in typical directories (≤1k files).
