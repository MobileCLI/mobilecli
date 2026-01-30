# MobileCLI File Browser & Editor - Complete Technical Specification

> **Version**: 1.0.0
> **Status**: Planning
> **Last Updated**: January 29, 2026
> **Authors**: MobileCLI Team

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Architecture Overview](#2-architecture-overview)
3. [Protocol Specification](#3-protocol-specification)
4. [Rust Daemon Implementation](#4-rust-daemon-implementation)
5. [Mobile Client Implementation](#5-mobile-client-implementation)
6. [File Viewers & Editors](#6-file-viewers--editors)
7. [Security Model](#7-security-model)
8. [Performance Optimization](#8-performance-optimization)
9. [Cross-Platform Considerations](#9-cross-platform-considerations)
10. [Edge Cases & Error Handling](#10-edge-cases--error-handling)
11. [UX/UI Design Guidelines](#11-uxui-design-guidelines)
12. [Accessibility](#12-accessibility)
13. [Testing Strategy](#13-testing-strategy)
14. [Implementation Phases](#14-implementation-phases)
15. [Appendices](#15-appendices)

---

## 1. Executive Summary

### 1.1 Vision

Transform MobileCLI into a complete remote development environment by adding a professional-grade file browser and editor that rivals dedicated apps like Working Copy, Termux, and mobile IDEs. Users will be able to browse, view, edit, and manage files on their desktop from their mobile device with the same fluidity as using a native file manager.

### 1.2 Key Features

| Feature | Description | Priority |
|---------|-------------|----------|
| **Directory Browsing** | Navigate file system with breadcrumb navigation | P0 |
| **File Viewing** | View text, code, images, PDFs, markdown | P0 |
| **Code Editing** | VS Code-style syntax highlighting and editing | P0 |
| **File Operations** | Create, delete, rename, copy, move files | P0 |
| **Search** | Global file search with content matching | P1 |
| **Real-time Sync** | Live updates when files change on desktop | P1 |
| **Git Integration** | Show file status indicators | P2 |
| **Offline Support** | Cache recently viewed files | P2 |

### 1.3 Success Metrics

- **Performance**: Directory listing < 100ms for 1,000 files
- **Responsiveness**: 60 FPS scrolling in file lists
- **Reliability**: Zero data loss in file operations
- **Usability**: Complete core tasks in < 3 taps

---

## 2. Architecture Overview

### 2.1 System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Mobile App (React Native)                 │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │   Sessions  │  │    Files    │  │        Settings         │ │
│  │     Tab     │  │     Tab     │  │          Tab            │ │
│  └─────────────┘  └──────┬──────┘  └─────────────────────────┘ │
│                          │                                       │
│  ┌───────────────────────┴───────────────────────────────────┐ │
│  │                    File System Store                       │ │
│  │  (Zustand: entries, cache, history, clipboard, watchers)  │ │
│  └───────────────────────┬───────────────────────────────────┘ │
│                          │                                       │
│  ┌───────────────────────┴───────────────────────────────────┐ │
│  │                   WebSocket Client                         │ │
│  │  (Reconnection, Message Queue, Binary Support)            │ │
│  └───────────────────────┬───────────────────────────────────┘ │
└──────────────────────────┼───────────────────────────────────────┘
                           │ WebSocket (ws://host:9847)
                           │ JSON + Binary frames
┌──────────────────────────┼───────────────────────────────────────┐
│                          │                                       │
│  ┌───────────────────────┴───────────────────────────────────┐ │
│  │                   WebSocket Server                         │ │
│  │  (tokio-tungstenite, message routing)                     │ │
│  └───────────────────────┬───────────────────────────────────┘ │
│                          │                                       │
│  ┌───────────────────────┴───────────────────────────────────┐ │
│  │                 File System Handler                        │ │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐   │ │
│  │  │   Security  │  │    Ops      │  │    Watcher      │   │ │
│  │  │  (path_jail)│  │ (tokio::fs) │  │    (notify)     │   │ │
│  │  └─────────────┘  └─────────────┘  └─────────────────┘   │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                                  │
│                    Rust Daemon (mobilecli)                       │
└──────────────────────────────────────────────────────────────────┘
                           │
                           ▼
            ┌──────────────────────────────┐
            │      Operating System        │
            │   (Linux / macOS / Windows)  │
            │         File System          │
            └──────────────────────────────┘
```

### 2.2 Data Flow

```
User Action → UI Component → Store Action → WebSocket Message
     ↓
Server Receives → Security Validation → File System Operation
     ↓
Result/Error → WebSocket Response → Store Update → UI Re-render
```

### 2.3 Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| State Management | Zustand | Lightweight, TypeScript-native, no boilerplate |
| List Rendering | FlashList | 10x faster than FlatList, eliminates blank cells |
| Code Editor | Monaco via WebView | Full VS Code feature parity |
| File Watching | notify crate | Cross-platform, battle-tested |
| Path Security | path_jail crate | Zero-dependency sandbox |
| Binary Transfer | Base64 in JSON | Simplicity over raw binary frames |

---

## 3. Protocol Specification

### 3.1 Message Types

#### 3.1.1 Client → Server Messages

```typescript
// TypeScript type definitions (mobile client)
type FileSystemRequest =
  | { type: 'list_directory'; path: string; include_hidden?: boolean; sort_by?: SortField; sort_order?: SortOrder }
  | { type: 'read_file'; path: string; offset?: number; length?: number; encoding?: 'utf8' | 'base64' }
  | { type: 'write_file'; path: string; content: string; encoding?: 'utf8' | 'base64'; create_parents?: boolean }
  | { type: 'create_directory'; path: string; recursive?: boolean }
  | { type: 'delete_path'; path: string; recursive?: boolean }
  | { type: 'rename_path'; old_path: string; new_path: string }
  | { type: 'copy_path'; source: string; destination: string; recursive?: boolean }
  | { type: 'get_file_info'; path: string }
  | { type: 'search_files'; path: string; pattern: string; content_pattern?: string; max_depth?: number; max_results?: number }
  | { type: 'watch_directory'; path: string }
  | { type: 'unwatch_directory'; path: string }
  | { type: 'get_home_directory' }
  | { type: 'get_allowed_roots' };

type SortField = 'name' | 'size' | 'modified' | 'type';
type SortOrder = 'asc' | 'desc';
```

```rust
// Rust type definitions (daemon)
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileSystemRequest {
    ListDirectory {
        path: String,
        #[serde(default)]
        include_hidden: bool,
        #[serde(default)]
        sort_by: Option<SortField>,
        #[serde(default)]
        sort_order: Option<SortOrder>,
    },
    ReadFile {
        path: String,
        #[serde(default)]
        offset: Option<u64>,
        #[serde(default)]
        length: Option<u64>,
        #[serde(default)]
        encoding: FileEncoding,
    },
    WriteFile {
        path: String,
        content: String,
        #[serde(default)]
        encoding: FileEncoding,
        #[serde(default)]
        create_parents: bool,
    },
    CreateDirectory {
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    DeletePath {
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    RenamePath {
        old_path: String,
        new_path: String,
    },
    CopyPath {
        source: String,
        destination: String,
        #[serde(default)]
        recursive: bool,
    },
    GetFileInfo {
        path: String,
    },
    SearchFiles {
        path: String,
        pattern: String,
        #[serde(default)]
        content_pattern: Option<String>,
        #[serde(default)]
        max_depth: Option<u32>,
        #[serde(default = "default_max_results")]
        max_results: u32,
    },
    WatchDirectory {
        path: String,
    },
    UnwatchDirectory {
        path: String,
    },
    GetHomeDirectory,
    GetAllowedRoots,
}

fn default_max_results() -> u32 { 1000 }
```

#### 3.1.2 Server → Client Messages

```rust
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileSystemResponse {
    DirectoryListing {
        path: String,
        entries: Vec<FileEntry>,
        total_count: usize,
        truncated: bool,
    },
    FileContent {
        path: String,
        content: String,
        encoding: FileEncoding,
        mime_type: String,
        size: u64,
        modified: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        truncated_at: Option<u64>,
    },
    FileInfo {
        path: String,
        entry: FileEntry,
    },
    OperationSuccess {
        operation: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    OperationError {
        operation: String,
        path: String,
        error: FileSystemError,
    },
    SearchResults {
        query: String,
        path: String,
        matches: Vec<SearchMatch>,
        truncated: bool,
    },
    FileChanged {
        path: String,
        change_type: ChangeType,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_entry: Option<FileEntry>,
    },
    HomeDirectory {
        path: String,
    },
    AllowedRoots {
        roots: Vec<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: u64,
    pub modified: u64,  // Unix timestamp milliseconds
    pub created: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<GitStatus>,
}

#[derive(Debug, Serialize)]
pub struct SearchMatch {
    pub path: String,
    pub entry: FileEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_matches: Option<Vec<ContentMatch>>,
}

#[derive(Debug, Serialize)]
pub struct ContentMatch {
    pub line_number: u32,
    pub line_content: String,
    pub match_start: u32,
    pub match_end: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
    Renamed { from: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemError {
    NotFound { path: String },
    PermissionDenied { path: String, reason: String },
    PathTraversal { attempted_path: String },
    NotADirectory { path: String },
    NotAFile { path: String },
    AlreadyExists { path: String },
    NotEmpty { path: String },
    FileTooLarge { path: String, size: u64, max_size: u64 },
    IoError { message: String },
    InvalidEncoding { path: String },
    OperationCancelled,
    RateLimited { retry_after_ms: u64 },
}
```

### 3.2 Chunked File Transfer

For files exceeding 1MB, use chunked transfer:

```rust
// Request chunks
#[derive(Debug, Deserialize)]
pub struct ChunkedReadRequest {
    pub path: String,
    pub chunk_index: u64,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: u64,  // Default 256KB
}

fn default_chunk_size() -> u64 { 262144 } // 256KB

// Response chunks
#[derive(Debug, Serialize)]
pub struct ChunkedReadResponse {
    pub path: String,
    pub chunk_index: u64,
    pub total_chunks: u64,
    pub total_size: u64,
    pub data: String,  // Base64 encoded
    pub checksum: String,  // MD5 of this chunk
    pub is_last: bool,
}
```

### 3.3 Rate Limiting

```rust
pub struct RateLimiter {
    requests_per_second: u32,      // Default: 100
    burst_size: u32,               // Default: 50
    large_file_cooldown_ms: u64,   // Default: 1000 (1 second between large file reads)
}
```

---

## 4. Rust Daemon Implementation

### 4.1 Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...

# File system
walkdir = "2.5"
ignore = "0.4"           # gitignore-aware walking
notify = "6.1"           # File watching
notify-debouncer-mini = "0.4"
path_jail = "0.3"        # Path sandboxing
infer = "0.15"           # MIME type detection
memmap2 = "0.9"          # Memory-mapped files (optional, for large files)

# Concurrency
dashmap = "5.5"          # Concurrent HashMap for watchers
rayon = "1.10"           # Parallel iteration for search

# Utilities
base64 = "0.22"
md5 = "0.7"              # Chunk checksums
chrono = "0.4"           # Timestamps
```

### 4.2 Module Structure

```
cli/src/
├── main.rs
├── daemon.rs            # Existing daemon code
├── protocol.rs          # Existing + new FileSystem messages
├── filesystem/
│   ├── mod.rs           # Module exports
│   ├── handler.rs       # Request routing
│   ├── operations.rs    # File operations implementation
│   ├── security.rs      # Path validation, sandboxing
│   ├── watcher.rs       # File watching with notify
│   ├── search.rs        # File search implementation
│   ├── mime.rs          # MIME type detection
│   └── config.rs        # File system configuration
└── ...
```

### 4.3 Security Implementation

```rust
// cli/src/filesystem/security.rs

use path_jail::PathJail;
use std::path::{Path, PathBuf};

/// Configuration for file system access
#[derive(Debug, Clone)]
pub struct FileSystemConfig {
    /// Allowed root directories (default: home directory)
    pub allowed_roots: Vec<PathBuf>,

    /// Denied file patterns (glob)
    pub denied_patterns: Vec<String>,

    /// Maximum file size for read operations (bytes)
    pub max_read_size: u64,

    /// Maximum file size for write operations (bytes)
    pub max_write_size: u64,

    /// Whether to follow symlinks
    pub follow_symlinks: bool,

    /// Read-only paths (can read but not modify)
    pub read_only_patterns: Vec<String>,
}

impl Default for FileSystemConfig {
    fn default() -> Self {
        Self {
            allowed_roots: vec![dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))],
            denied_patterns: vec![
                "**/.ssh/*".to_string(),
                "**/*.pem".to_string(),
                "**/*.key".to_string(),
                "**/id_rsa*".to_string(),
                "**/.gnupg/*".to_string(),
                "**/.aws/credentials".to_string(),
                "**/.env".to_string(),
                "**/.env.*".to_string(),
            ],
            max_read_size: 50 * 1024 * 1024,  // 50MB
            max_write_size: 50 * 1024 * 1024, // 50MB
            follow_symlinks: false,
            read_only_patterns: vec![
                "/etc/**".to_string(),
                "/usr/**".to_string(),
                "/bin/**".to_string(),
                "/sbin/**".to_string(),
            ],
        }
    }
}

/// Validates and sanitizes paths before any file operation
pub struct PathValidator {
    config: FileSystemConfig,
    jails: Vec<PathJail>,
}

impl PathValidator {
    pub fn new(config: FileSystemConfig) -> Self {
        let jails = config.allowed_roots
            .iter()
            .filter_map(|root| PathJail::new(root).ok())
            .collect();

        Self { config, jails }
    }

    /// Validate a path is safe to access
    pub fn validate(&self, path: &str) -> Result<PathBuf, FileSystemError> {
        let path = Path::new(path);

        // 1. Canonicalize to resolve any ../ or symlinks
        let canonical = if path.is_absolute() {
            path.canonicalize()
                .map_err(|e| FileSystemError::IoError { message: e.to_string() })?
        } else {
            return Err(FileSystemError::PathTraversal {
                attempted_path: path.display().to_string(),
            });
        };

        // 2. Check against allowed roots using path_jail
        let is_allowed = self.jails.iter().any(|jail| {
            jail.contains(&canonical).unwrap_or(false)
        });

        if !is_allowed {
            return Err(FileSystemError::PermissionDenied {
                path: canonical.display().to_string(),
                reason: "Path is outside allowed directories".to_string(),
            });
        }

        // 3. Check denied patterns
        for pattern in &self.config.denied_patterns {
            if glob_match::glob_match(pattern, canonical.to_string_lossy().as_ref()) {
                return Err(FileSystemError::PermissionDenied {
                    path: canonical.display().to_string(),
                    reason: format!("Path matches denied pattern: {}", pattern),
                });
            }
        }

        // 4. Check symlink safety if not following symlinks
        if !self.config.follow_symlinks && canonical.is_symlink() {
            let target = std::fs::read_link(&canonical)
                .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

            // Ensure symlink target is also within allowed roots
            let target_canonical = target.canonicalize()
                .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

            let target_allowed = self.jails.iter().any(|jail| {
                jail.contains(&target_canonical).unwrap_or(false)
            });

            if !target_allowed {
                return Err(FileSystemError::PermissionDenied {
                    path: canonical.display().to_string(),
                    reason: "Symlink target is outside allowed directories".to_string(),
                });
            }
        }

        Ok(canonical)
    }

    /// Check if path is writable (not in read-only patterns)
    pub fn is_writable(&self, path: &Path) -> bool {
        for pattern in &self.config.read_only_patterns {
            if glob_match::glob_match(pattern, path.to_string_lossy().as_ref()) {
                return false;
            }
        }
        true
    }
}
```

### 4.4 File Operations

```rust
// cli/src/filesystem/operations.rs

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct FileOperations {
    validator: PathValidator,
    config: FileSystemConfig,
}

impl FileOperations {
    /// List directory contents
    pub async fn list_directory(
        &self,
        path: &str,
        include_hidden: bool,
        sort_by: Option<SortField>,
        sort_order: Option<SortOrder>,
    ) -> Result<DirectoryListing, FileSystemError> {
        let path = self.validator.validate(path)?;

        if !path.is_dir() {
            return Err(FileSystemError::NotADirectory {
                path: path.display().to_string(),
            });
        }

        let mut entries = Vec::new();
        let mut read_dir = fs::read_dir(&path).await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        while let Some(entry) = read_dir.next_entry().await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?
        {
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files if not requested
            if !include_hidden && name.starts_with('.') {
                continue;
            }

            // Get metadata
            let metadata = entry.metadata().await
                .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

            let file_entry = self.build_file_entry(&entry.path(), &metadata, &name).await;
            entries.push(file_entry);
        }

        // Sort entries
        self.sort_entries(&mut entries, sort_by, sort_order);

        // Always put directories first
        entries.sort_by(|a, b| b.is_directory.cmp(&a.is_directory));

        let total_count = entries.len();
        let truncated = total_count > 10000;
        if truncated {
            entries.truncate(10000);
        }

        Ok(DirectoryListing {
            path: path.display().to_string(),
            entries,
            total_count,
            truncated,
        })
    }

    /// Read file contents
    pub async fn read_file(
        &self,
        path: &str,
        offset: Option<u64>,
        length: Option<u64>,
        encoding: FileEncoding,
    ) -> Result<FileContent, FileSystemError> {
        let path = self.validator.validate(path)?;

        if !path.is_file() {
            return Err(FileSystemError::NotAFile {
                path: path.display().to_string(),
            });
        }

        let metadata = fs::metadata(&path).await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        let size = metadata.len();

        // Check size limit
        if size > self.config.max_read_size {
            return Err(FileSystemError::FileTooLarge {
                path: path.display().to_string(),
                size,
                max_size: self.config.max_read_size,
            });
        }

        // Read file
        let mut file = fs::File::open(&path).await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        // Handle offset
        if let Some(offset) = offset {
            use tokio::io::AsyncSeekExt;
            file.seek(std::io::SeekFrom::Start(offset)).await
                .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;
        }

        // Read content
        let read_length = length.unwrap_or(size).min(size);
        let mut buffer = vec![0u8; read_length as usize];
        let bytes_read = file.read(&mut buffer).await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;
        buffer.truncate(bytes_read);

        // Detect MIME type
        let mime_type = infer::get(&buffer)
            .map(|t| t.mime_type().to_string())
            .unwrap_or_else(|| self.guess_mime_from_extension(&path));

        // Encode content
        let (content, actual_encoding) = match encoding {
            FileEncoding::Utf8 => {
                match String::from_utf8(buffer.clone()) {
                    Ok(s) => (s, FileEncoding::Utf8),
                    Err(_) => (base64::encode(&buffer), FileEncoding::Base64),
                }
            }
            FileEncoding::Base64 => (base64::encode(&buffer), FileEncoding::Base64),
        };

        let modified = metadata.modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64)
            .unwrap_or(0);

        Ok(FileContent {
            path: path.display().to_string(),
            content,
            encoding: actual_encoding,
            mime_type,
            size,
            modified,
            truncated_at: if bytes_read < size as usize { Some(bytes_read as u64) } else { None },
        })
    }

    /// Write file contents
    pub async fn write_file(
        &self,
        path: &str,
        content: &str,
        encoding: FileEncoding,
        create_parents: bool,
    ) -> Result<(), FileSystemError> {
        let path = self.validator.validate(path)?;

        if !self.validator.is_writable(&path) {
            return Err(FileSystemError::PermissionDenied {
                path: path.display().to_string(),
                reason: "Path is read-only".to_string(),
            });
        }

        // Decode content
        let bytes = match encoding {
            FileEncoding::Utf8 => content.as_bytes().to_vec(),
            FileEncoding::Base64 => base64::decode(content)
                .map_err(|e| FileSystemError::InvalidEncoding {
                    path: path.display().to_string(),
                })?,
        };

        // Check size limit
        if bytes.len() as u64 > self.config.max_write_size {
            return Err(FileSystemError::FileTooLarge {
                path: path.display().to_string(),
                size: bytes.len() as u64,
                max_size: self.config.max_write_size,
            });
        }

        // Create parent directories if requested
        if create_parents {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await
                    .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;
            }
        }

        // Write atomically using temp file + rename
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, &bytes).await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        fs::rename(&temp_path, &path).await
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        Ok(())
    }

    /// Search for files
    pub async fn search_files(
        &self,
        path: &str,
        pattern: &str,
        content_pattern: Option<&str>,
        max_depth: Option<u32>,
        max_results: u32,
    ) -> Result<SearchResults, FileSystemError> {
        let path = self.validator.validate(path)?;

        // Use ignore crate for gitignore-aware walking
        let walker = ignore::WalkBuilder::new(&path)
            .max_depth(max_depth.map(|d| d as usize))
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build_parallel();

        let matches = Arc::new(Mutex::new(Vec::new()));
        let pattern = glob::Pattern::new(pattern)
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        walker.run(|| {
            let matches = Arc::clone(&matches);
            let pattern = pattern.clone();
            let content_pattern = content_pattern.map(|s| s.to_string());

            Box::new(move |entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                // Check if we've hit the limit
                if matches.lock().unwrap().len() >= max_results as usize {
                    return ignore::WalkState::Quit;
                }

                let path = entry.path();
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Match filename against pattern
                if !pattern.matches(&name) {
                    return ignore::WalkState::Continue;
                }

                // Content search if requested
                let content_matches = if let Some(ref content_pat) = content_pattern {
                    if path.is_file() {
                        search_file_content(path, content_pat).ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Build match entry
                if let Ok(metadata) = std::fs::metadata(path) {
                    let file_entry = build_file_entry_sync(path, &metadata, &name);
                    matches.lock().unwrap().push(SearchMatch {
                        path: path.display().to_string(),
                        entry: file_entry,
                        content_matches,
                    });
                }

                ignore::WalkState::Continue
            })
        });

        let matches = Arc::try_unwrap(matches).unwrap().into_inner().unwrap();
        let truncated = matches.len() >= max_results as usize;

        Ok(SearchResults {
            query: pattern.to_string(),
            path: path.display().to_string(),
            matches,
            truncated,
        })
    }
}
```

### 4.5 File Watching

```rust
// cli/src/filesystem/watcher.rs

use notify::{Watcher, RecursiveMode, Event};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use dashmap::DashMap;
use tokio::sync::broadcast;

pub struct FileWatcher {
    watchers: DashMap<String, notify::RecommendedWatcher>,
    event_tx: broadcast::Sender<FileChanged>,
    debounce_ms: u64,
}

impl FileWatcher {
    pub fn new(debounce_ms: u64) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            watchers: DashMap::new(),
            event_tx,
            debounce_ms,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FileChanged> {
        self.event_tx.subscribe()
    }

    pub fn watch(&self, path: &str) -> Result<(), FileSystemError> {
        if self.watchers.contains_key(path) {
            return Ok(());  // Already watching
        }

        let path_buf = PathBuf::from(path);
        let event_tx = self.event_tx.clone();

        let mut debouncer = new_debouncer(
            std::time::Duration::from_millis(self.debounce_ms),
            move |res: Result<Vec<DebouncedEvent>, _>| {
                if let Ok(events) = res {
                    for event in events {
                        let change = match event.kind {
                            DebouncedEventKind::Any => {
                                // Determine actual change type
                                let path = event.path.display().to_string();
                                if event.path.exists() {
                                    if event.path.metadata()
                                        .map(|m| m.created().ok())
                                        .flatten()
                                        .map(|t| t.elapsed().map(|d| d.as_secs() < 2).unwrap_or(false))
                                        .unwrap_or(false)
                                    {
                                        FileChanged { path, change_type: ChangeType::Created, new_entry: None }
                                    } else {
                                        FileChanged { path, change_type: ChangeType::Modified, new_entry: None }
                                    }
                                } else {
                                    FileChanged { path, change_type: ChangeType::Deleted, new_entry: None }
                                }
                            }
                            _ => continue,
                        };

                        let _ = event_tx.send(change);
                    }
                }
            },
        ).map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        debouncer.watcher()
            .watch(&path_buf, RecursiveMode::NonRecursive)
            .map_err(|e| FileSystemError::IoError { message: e.to_string() })?;

        self.watchers.insert(path.to_string(), debouncer.into());
        Ok(())
    }

    pub fn unwatch(&self, path: &str) -> Result<(), FileSystemError> {
        self.watchers.remove(path);
        Ok(())
    }
}
```

### 4.6 MIME Type Detection

```rust
// cli/src/filesystem/mime.rs

use infer::Infer;

/// Detect MIME type from file content and extension
pub fn detect_mime_type(buffer: &[u8], filename: &str) -> String {
    // Try content-based detection first (magic bytes)
    if let Some(kind) = infer::get(buffer) {
        return kind.mime_type().to_string();
    }

    // Fall back to extension-based detection
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Programming languages
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "js" => "application/javascript",
        "ts" => "text/typescript",
        "tsx" => "text/typescript-jsx",
        "jsx" => "text/javascript-jsx",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
        "rb" => "text/x-ruby",
        "php" => "text/x-php",
        "swift" => "text/x-swift",
        "kt" | "kts" => "text/x-kotlin",
        "scala" => "text/x-scala",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "ps1" => "text/x-powershell",

        // Markup & data
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "scss" | "sass" => "text/x-scss",
        "less" => "text/x-less",
        "xml" => "application/xml",
        "json" => "application/json",
        "yaml" | "yml" => "text/x-yaml",
        "toml" => "text/x-toml",
        "md" | "markdown" => "text/markdown",
        "rst" => "text/x-rst",
        "tex" => "text/x-tex",

        // Config files
        "ini" | "cfg" | "conf" => "text/x-ini",
        "env" => "text/x-env",
        "dockerfile" => "text/x-dockerfile",
        "makefile" => "text/x-makefile",

        // Documents
        "txt" => "text/plain",
        "log" => "text/x-log",
        "csv" => "text/csv",

        // Images (if not detected by magic bytes)
        "svg" => "image/svg+xml",

        // Default
        _ => "application/octet-stream",
    }.to_string()
}

/// Check if MIME type is text-based (safe to display as text)
pub fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/") ||
    mime == "application/json" ||
    mime == "application/javascript" ||
    mime == "application/xml" ||
    mime.ends_with("+xml") ||
    mime.ends_with("+json")
}

/// Get language identifier for syntax highlighting
pub fn mime_to_language(mime: &str) -> Option<&'static str> {
    match mime {
        "text/x-rust" => Some("rust"),
        "text/x-python" => Some("python"),
        "application/javascript" | "text/javascript-jsx" => Some("javascript"),
        "text/typescript" | "text/typescript-jsx" => Some("typescript"),
        "text/x-go" => Some("go"),
        "text/x-java" => Some("java"),
        "text/x-c" => Some("c"),
        "text/x-c++" => Some("cpp"),
        "text/x-ruby" => Some("ruby"),
        "text/x-php" => Some("php"),
        "text/x-swift" => Some("swift"),
        "text/x-kotlin" => Some("kotlin"),
        "text/x-shellscript" => Some("bash"),
        "text/html" => Some("html"),
        "text/css" => Some("css"),
        "text/x-scss" => Some("scss"),
        "application/json" => Some("json"),
        "text/x-yaml" => Some("yaml"),
        "text/x-toml" => Some("toml"),
        "text/markdown" => Some("markdown"),
        "application/xml" => Some("xml"),
        "text/x-dockerfile" => Some("dockerfile"),
        "text/x-makefile" => Some("makefile"),
        _ => None,
    }
}
```

---

## 5. Mobile Client Implementation

### 5.1 State Management

```typescript
// mobile/stores/fileSystemStore.ts

import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import AsyncStorage from '@react-native-async-storage/async-storage';

interface FileEntry {
  name: string;
  path: string;
  isDirectory: boolean;
  isSymlink: boolean;
  isHidden: boolean;
  size: number;
  modified: number;
  created?: number;
  mimeType?: string;
  permissions?: string;
  symlinkTarget?: string;
  gitStatus?: 'modified' | 'added' | 'deleted' | 'untracked' | 'ignored';
}

interface CachedFile {
  path: string;
  content: string;
  encoding: 'utf8' | 'base64';
  mimeType: string;
  size: number;
  modified: number;
  cachedAt: number;
}

interface ClipboardState {
  paths: string[];
  operation: 'copy' | 'cut';
}

interface FileSystemState {
  // Navigation
  currentPath: string;
  entries: FileEntry[];
  loading: boolean;
  error: string | null;

  // History
  history: string[];
  historyIndex: number;

  // View settings
  viewMode: 'list' | 'grid';
  sortBy: 'name' | 'size' | 'modified' | 'type';
  sortOrder: 'asc' | 'desc';
  showHidden: boolean;

  // Selection
  selectedPaths: Set<string>;
  selectionMode: boolean;

  // Clipboard
  clipboard: ClipboardState | null;

  // Cache
  fileCache: Map<string, CachedFile>;
  maxCacheSize: number;
  maxCacheAge: number;

  // Watchers
  watchedPaths: Set<string>;

  // Home directory
  homeDirectory: string | null;
  allowedRoots: string[];
}

interface FileSystemActions {
  // Navigation
  navigate: (path: string) => Promise<void>;
  refresh: () => Promise<void>;
  goBack: () => void;
  goForward: () => void;
  goToParent: () => void;
  goHome: () => void;

  // View
  setViewMode: (mode: 'list' | 'grid') => void;
  setSortBy: (field: 'name' | 'size' | 'modified' | 'type') => void;
  setSortOrder: (order: 'asc' | 'desc') => void;
  toggleHidden: () => void;

  // Selection
  selectPath: (path: string, multi?: boolean) => void;
  selectAll: () => void;
  deselectAll: () => void;
  toggleSelectionMode: () => void;

  // Clipboard
  copy: () => void;
  cut: () => void;
  paste: () => Promise<void>;

  // File operations
  createFile: (name: string, content?: string) => Promise<void>;
  createFolder: (name: string) => Promise<void>;
  rename: (oldPath: string, newName: string) => Promise<void>;
  deleteSelected: () => Promise<void>;
  deletePath: (path: string) => Promise<void>;

  // File reading
  readFile: (path: string) => Promise<CachedFile>;
  writeFile: (path: string, content: string, encoding?: 'utf8' | 'base64') => Promise<void>;

  // Search
  search: (query: string, contentSearch?: boolean) => Promise<FileEntry[]>;

  // Watching
  watchCurrentDirectory: () => void;
  unwatchCurrentDirectory: () => void;
  handleFileChange: (change: FileChange) => void;

  // Cache management
  clearCache: () => void;
  getCachedFile: (path: string) => CachedFile | undefined;

  // Initialization
  initialize: () => Promise<void>;
}

export const useFileSystemStore = create<FileSystemState & FileSystemActions>()(
  persist(
    (set, get) => ({
      // Initial state
      currentPath: '',
      entries: [],
      loading: false,
      error: null,
      history: [],
      historyIndex: -1,
      viewMode: 'list',
      sortBy: 'name',
      sortOrder: 'asc',
      showHidden: false,
      selectedPaths: new Set(),
      selectionMode: false,
      clipboard: null,
      fileCache: new Map(),
      maxCacheSize: 50 * 1024 * 1024, // 50MB
      maxCacheAge: 30 * 60 * 1000, // 30 minutes
      watchedPaths: new Set(),
      homeDirectory: null,
      allowedRoots: [],

      // Navigation actions
      navigate: async (path: string) => {
        const { currentPath, history, historyIndex } = get();

        set({ loading: true, error: null });

        try {
          const response = await sendFileSystemRequest({
            type: 'list_directory',
            path,
            include_hidden: get().showHidden,
            sort_by: get().sortBy,
            sort_order: get().sortOrder,
          });

          if (response.type === 'directory_listing') {
            // Update history
            const newHistory = history.slice(0, historyIndex + 1);
            newHistory.push(path);

            set({
              currentPath: path,
              entries: response.entries,
              loading: false,
              history: newHistory,
              historyIndex: newHistory.length - 1,
              selectedPaths: new Set(),
              selectionMode: false,
            });

            // Watch the new directory
            get().watchCurrentDirectory();
          }
        } catch (error) {
          set({
            loading: false,
            error: error instanceof Error ? error.message : 'Failed to load directory',
          });
        }
      },

      refresh: async () => {
        const { currentPath } = get();
        if (currentPath) {
          await get().navigate(currentPath);
        }
      },

      goBack: () => {
        const { history, historyIndex } = get();
        if (historyIndex > 0) {
          const newIndex = historyIndex - 1;
          set({ historyIndex: newIndex });
          get().navigate(history[newIndex]);
        }
      },

      goForward: () => {
        const { history, historyIndex } = get();
        if (historyIndex < history.length - 1) {
          const newIndex = historyIndex + 1;
          set({ historyIndex: newIndex });
          get().navigate(history[newIndex]);
        }
      },

      goToParent: () => {
        const { currentPath } = get();
        const parentPath = currentPath.split('/').slice(0, -1).join('/') || '/';
        get().navigate(parentPath);
      },

      goHome: () => {
        const { homeDirectory } = get();
        if (homeDirectory) {
          get().navigate(homeDirectory);
        }
      },

      // File reading with caching
      readFile: async (path: string) => {
        const { fileCache, maxCacheAge } = get();

        // Check cache
        const cached = fileCache.get(path);
        if (cached && Date.now() - cached.cachedAt < maxCacheAge) {
          return cached;
        }

        // Fetch from server
        const response = await sendFileSystemRequest({
          type: 'read_file',
          path,
          encoding: 'utf8',
        });

        if (response.type === 'file_content') {
          const cachedFile: CachedFile = {
            path: response.path,
            content: response.content,
            encoding: response.encoding,
            mimeType: response.mime_type,
            size: response.size,
            modified: response.modified,
            cachedAt: Date.now(),
          };

          // Update cache with LRU eviction
          const newCache = new Map(fileCache);
          newCache.set(path, cachedFile);

          // Evict old entries if cache is too large
          let cacheSize = 0;
          for (const [, file] of newCache) {
            cacheSize += file.content.length;
          }

          const { maxCacheSize } = get();
          while (cacheSize > maxCacheSize && newCache.size > 1) {
            const oldestKey = newCache.keys().next().value;
            const oldest = newCache.get(oldestKey);
            if (oldest) {
              cacheSize -= oldest.content.length;
              newCache.delete(oldestKey);
            }
          }

          set({ fileCache: newCache });
          return cachedFile;
        }

        throw new Error('Failed to read file');
      },

      // Handle real-time file changes
      handleFileChange: (change: FileChange) => {
        const { currentPath, entries, fileCache } = get();

        // Check if change is in current directory
        const changePath = change.path;
        const changeDir = changePath.substring(0, changePath.lastIndexOf('/'));

        if (changeDir !== currentPath) {
          return; // Not in current directory
        }

        switch (change.change_type) {
          case 'created':
            if (change.new_entry) {
              set({ entries: [...entries, change.new_entry] });
            }
            break;

          case 'modified':
            // Invalidate cache
            if (fileCache.has(changePath)) {
              const newCache = new Map(fileCache);
              newCache.delete(changePath);
              set({ fileCache: newCache });
            }
            // Update entry
            if (change.new_entry) {
              set({
                entries: entries.map(e =>
                  e.path === changePath ? change.new_entry! : e
                ),
              });
            }
            break;

          case 'deleted':
            set({
              entries: entries.filter(e => e.path !== changePath),
            });
            // Remove from cache
            if (fileCache.has(changePath)) {
              const newCache = new Map(fileCache);
              newCache.delete(changePath);
              set({ fileCache: newCache });
            }
            break;
        }
      },

      // Initialize
      initialize: async () => {
        try {
          // Get home directory
          const homeResponse = await sendFileSystemRequest({ type: 'get_home_directory' });
          if (homeResponse.type === 'home_directory') {
            set({ homeDirectory: homeResponse.path });
          }

          // Get allowed roots
          const rootsResponse = await sendFileSystemRequest({ type: 'get_allowed_roots' });
          if (rootsResponse.type === 'allowed_roots') {
            set({ allowedRoots: rootsResponse.roots });
          }

          // Navigate to home
          const { homeDirectory } = get();
          if (homeDirectory) {
            await get().navigate(homeDirectory);
          }
        } catch (error) {
          set({ error: 'Failed to initialize file system' });
        }
      },

      // ... (other actions implemented similarly)
    }),
    {
      name: 'file-system-storage',
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({
        viewMode: state.viewMode,
        sortBy: state.sortBy,
        sortOrder: state.sortOrder,
        showHidden: state.showHidden,
      }),
    }
  )
);
```

### 5.2 Component Structure

```
mobile/app/
├── (tabs)/
│   ├── index.tsx          # Sessions tab
│   ├── files.tsx          # NEW: Files tab (main file browser)
│   └── settings.tsx       # Settings tab
├── file/
│   ├── [path].tsx         # File viewer/editor screen
│   └── search.tsx         # Search screen
└── _layout.tsx            # Tab navigator

mobile/components/files/
├── FileBrowser.tsx        # Main file browser container
├── BreadcrumbNav.tsx      # Path breadcrumb navigation
├── FileList.tsx           # FlashList of files
├── FileListItem.tsx       # Individual file row
├── FileGridItem.tsx       # Grid view item
├── FileIcon.tsx           # File type icons
├── SelectionBar.tsx       # Multi-select action bar
├── CreateModal.tsx        # New file/folder modal
├── RenameModal.tsx        # Rename modal
├── DeleteConfirm.tsx      # Delete confirmation
├── FileInfoSheet.tsx      # File details bottom sheet
├── SearchBar.tsx          # Search input
├── SortMenu.tsx           # Sort options menu
├── EmptyState.tsx         # Empty directory state
└── LoadingState.tsx       # Skeleton loading

mobile/components/viewers/
├── TextViewer.tsx         # Plain text viewer
├── CodeEditor.tsx         # Monaco-based code editor
├── MarkdownViewer.tsx     # Markdown preview
├── ImageViewer.tsx        # Zoomable image viewer
├── PdfViewer.tsx          # PDF viewer
├── HexViewer.tsx          # Binary/hex viewer
└── UnsupportedViewer.tsx  # Fallback for unsupported types
```

---

## 6. File Viewers & Editors

### 6.1 Code Editor (Monaco via WebView)

```typescript
// mobile/components/viewers/CodeEditor.tsx

import React, { useCallback, useRef, useState } from 'react';
import { View, StyleSheet, ActivityIndicator } from 'react-native';
import { WebView } from 'react-native-webview';
import { useColorScheme } from 'react-native';

interface CodeEditorProps {
  content: string;
  language: string;
  readOnly?: boolean;
  onChange?: (content: string) => void;
  onSave?: (content: string) => void;
}

const MONACO_VERSION = '0.45.0';

export function CodeEditor({
  content,
  language,
  readOnly = false,
  onChange,
  onSave,
}: CodeEditorProps) {
  const colorScheme = useColorScheme();
  const webViewRef = useRef<WebView>(null);
  const [isLoading, setIsLoading] = useState(true);

  const theme = colorScheme === 'dark' ? 'vs-dark' : 'vs';

  const html = `
<!DOCTYPE html>
<html>
<head>
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    html, body { width: 100%; height: 100%; overflow: hidden; }
    #editor { width: 100%; height: 100%; }
    .loading {
      display: flex;
      align-items: center;
      justify-content: center;
      height: 100%;
      color: ${colorScheme === 'dark' ? '#fff' : '#000'};
    }
  </style>
</head>
<body>
  <div id="editor"></div>
  <script src="https://cdn.jsdelivr.net/npm/monaco-editor@${MONACO_VERSION}/min/vs/loader.js"></script>
  <script>
    require.config({
      paths: { vs: 'https://cdn.jsdelivr.net/npm/monaco-editor@${MONACO_VERSION}/min/vs' }
    });

    require(['vs/editor/editor.main'], function() {
      const editor = monaco.editor.create(document.getElementById('editor'), {
        value: ${JSON.stringify(content)},
        language: '${language}',
        theme: '${theme}',
        readOnly: ${readOnly},
        automaticLayout: true,
        fontSize: 14,
        lineNumbers: 'on',
        minimap: { enabled: false },
        wordWrap: 'on',
        scrollBeyondLastLine: false,
        renderWhitespace: 'selection',
        tabSize: 2,
        insertSpaces: true,
        folding: true,
        lineDecorationsWidth: 10,
        glyphMargin: false,
        quickSuggestions: false,
        suggestOnTriggerCharacters: false,
        acceptSuggestionOnEnter: 'off',
        tabCompletion: 'off',
        parameterHints: { enabled: false },
      });

      // Notify React Native when ready
      window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'ready' }));

      // Send content changes
      let debounceTimer;
      editor.onDidChangeModelContent(() => {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => {
          window.ReactNativeWebView.postMessage(JSON.stringify({
            type: 'change',
            content: editor.getValue()
          }));
        }, 300);
      });

      // Handle save command (Cmd/Ctrl+S)
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        window.ReactNativeWebView.postMessage(JSON.stringify({
          type: 'save',
          content: editor.getValue()
        }));
      });

      // Handle external commands
      window.setContent = (content) => {
        editor.setValue(content);
      };

      window.getContent = () => {
        return editor.getValue();
      };
    });
  </script>
</body>
</html>
  `;

  const handleMessage = useCallback((event: any) => {
    try {
      const message = JSON.parse(event.nativeEvent.data);

      switch (message.type) {
        case 'ready':
          setIsLoading(false);
          break;
        case 'change':
          onChange?.(message.content);
          break;
        case 'save':
          onSave?.(message.content);
          break;
      }
    } catch (error) {
      console.error('CodeEditor message error:', error);
    }
  }, [onChange, onSave]);

  return (
    <View style={styles.container}>
      {isLoading && (
        <View style={styles.loadingOverlay}>
          <ActivityIndicator size="large" />
        </View>
      )}
      <WebView
        ref={webViewRef}
        source={{ html }}
        style={styles.webview}
        onMessage={handleMessage}
        javaScriptEnabled
        domStorageEnabled
        originWhitelist={['*']}
        scrollEnabled={false}
        bounces={false}
        overScrollMode="never"
      />
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
  },
  webview: {
    flex: 1,
    backgroundColor: 'transparent',
  },
  loadingOverlay: {
    ...StyleSheet.absoluteFillObject,
    justifyContent: 'center',
    alignItems: 'center',
    backgroundColor: 'rgba(0,0,0,0.3)',
    zIndex: 1,
  },
});
```

### 6.2 Language Detection

```typescript
// mobile/utils/languageDetection.ts

const MIME_TO_LANGUAGE: Record<string, string> = {
  'text/x-rust': 'rust',
  'text/x-python': 'python',
  'application/javascript': 'javascript',
  'text/typescript': 'typescript',
  'text/typescript-jsx': 'typescriptreact',
  'text/javascript-jsx': 'javascriptreact',
  'text/x-go': 'go',
  'text/x-java': 'java',
  'text/x-c': 'c',
  'text/x-c++': 'cpp',
  'text/x-ruby': 'ruby',
  'text/x-php': 'php',
  'text/x-swift': 'swift',
  'text/x-kotlin': 'kotlin',
  'text/x-scala': 'scala',
  'text/x-shellscript': 'shell',
  'text/x-powershell': 'powershell',
  'text/html': 'html',
  'text/css': 'css',
  'text/x-scss': 'scss',
  'text/x-less': 'less',
  'application/xml': 'xml',
  'application/json': 'json',
  'text/x-yaml': 'yaml',
  'text/x-toml': 'toml',
  'text/markdown': 'markdown',
  'text/x-dockerfile': 'dockerfile',
  'text/x-makefile': 'makefile',
  'text/x-ini': 'ini',
  'text/plain': 'plaintext',
};

const EXTENSION_TO_LANGUAGE: Record<string, string> = {
  rs: 'rust',
  py: 'python',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  ts: 'typescript',
  tsx: 'typescriptreact',
  jsx: 'javascriptreact',
  go: 'go',
  java: 'java',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp',
  rb: 'ruby',
  php: 'php',
  swift: 'swift',
  kt: 'kotlin',
  kts: 'kotlin',
  scala: 'scala',
  sh: 'shell',
  bash: 'shell',
  zsh: 'shell',
  ps1: 'powershell',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'scss',
  sass: 'scss',
  less: 'less',
  xml: 'xml',
  json: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  md: 'markdown',
  markdown: 'markdown',
  dockerfile: 'dockerfile',
  makefile: 'makefile',
  ini: 'ini',
  cfg: 'ini',
  conf: 'ini',
  txt: 'plaintext',
  log: 'plaintext',
};

export function detectLanguage(filename: string, mimeType?: string): string {
  // Try MIME type first
  if (mimeType && MIME_TO_LANGUAGE[mimeType]) {
    return MIME_TO_LANGUAGE[mimeType];
  }

  // Fall back to extension
  const ext = filename.split('.').pop()?.toLowerCase() || '';

  // Handle special filenames
  const basename = filename.split('/').pop()?.toLowerCase() || '';
  if (basename === 'dockerfile' || basename.startsWith('dockerfile.')) {
    return 'dockerfile';
  }
  if (basename === 'makefile' || basename === 'gnumakefile') {
    return 'makefile';
  }
  if (basename === '.gitignore' || basename === '.dockerignore') {
    return 'ignore';
  }
  if (basename === '.env' || basename.startsWith('.env.')) {
    return 'dotenv';
  }

  return EXTENSION_TO_LANGUAGE[ext] || 'plaintext';
}
```

---

## 7. Security Model

### 7.1 Threat Model

| Threat | Mitigation |
|--------|------------|
| Path traversal (`../`) | Canonicalize paths, validate against allowed roots |
| Symlink escape | Check symlink targets, optionally disable following |
| Sensitive file access | Deny patterns for `.ssh`, `.env`, credentials |
| DoS via large files | Size limits on read/write operations |
| DoS via deep recursion | Depth limits on search/walk operations |
| Code injection | No shell execution, parameterized operations |
| MITM attacks | TLS for remote connections (Tailscale) |

### 7.2 Default Security Configuration

```rust
FileSystemConfig {
    // Only allow access within home directory
    allowed_roots: vec![dirs::home_dir()],

    // Block sensitive files
    denied_patterns: vec![
        "**/.ssh/*",
        "**/*.pem",
        "**/*.key",
        "**/id_rsa*",
        "**/.gnupg/*",
        "**/.aws/credentials",
        "**/.env",
        "**/.env.*",
        "**/secrets.*",
        "**/*.secret",
        "**/token*",
        "**/.npmrc",
        "**/.pypirc",
    ],

    // 50MB limits
    max_read_size: 50 * 1024 * 1024,
    max_write_size: 50 * 1024 * 1024,

    // Don't follow symlinks by default
    follow_symlinks: false,

    // System directories are read-only
    read_only_patterns: vec![
        "/etc/**",
        "/usr/**",
        "/bin/**",
        "/sbin/**",
        "/System/**",      // macOS
        "/Library/**",     // macOS
        "C:\\Windows\\**", // Windows
    ],
}
```

### 7.3 Audit Logging

```rust
#[derive(Debug, Serialize)]
pub struct AuditEvent {
    pub timestamp: u64,
    pub operation: String,
    pub path: String,
    pub success: bool,
    pub error: Option<String>,
    pub client_id: String,
}

// Log all file operations for security review
fn audit_log(event: AuditEvent) {
    tracing::info!(
        operation = %event.operation,
        path = %event.path,
        success = event.success,
        error = ?event.error,
        "file_operation"
    );
}
```

---

## 8. Performance Optimization

### 8.1 Rust Daemon Optimizations

#### Large Directory Performance

```rust
// Use parallel iteration for large directories
use rayon::prelude::*;

fn sort_entries_parallel(entries: &mut Vec<FileEntry>, sort_by: SortField) {
    entries.par_sort_unstable_by(|a, b| {
        // Directories always first
        match (a.is_directory, b.is_directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => match sort_by {
                SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortField::Size => a.size.cmp(&b.size),
                SortField::Modified => a.modified.cmp(&b.modified),
                SortField::Type => get_extension(&a.name).cmp(&get_extension(&b.name)),
            }
        }
    });
}
```

#### Memory-Mapped Large File Reading

```rust
use memmap2::Mmap;

async fn read_large_file_mmap(path: &Path) -> Result<Vec<u8>, io::Error> {
    let file = std::fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(mmap.to_vec())
}
```

#### Async File I/O with Tokio

```rust
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

// Use buffered I/O for better performance
async fn read_file_buffered(path: &Path) -> Result<String, io::Error> {
    let file = fs::File::open(path).await?;
    let mut reader = BufReader::new(file);
    let mut content = String::new();
    reader.read_to_string(&mut content).await?;
    Ok(content)
}
```

### 8.2 Mobile Client Optimizations

#### FlashList Configuration

```typescript
import { FlashList } from '@shopify/flash-list';

<FlashList
  data={entries}
  renderItem={renderFileItem}
  estimatedItemSize={56}  // Average row height
  drawDistance={250}      // Render items 250px offscreen
  keyExtractor={(item) => item.path}
  getItemType={(item) => item.isDirectory ? 'directory' : 'file'}
  overrideItemLayout={(layout, item) => {
    layout.size = item.isDirectory ? 56 : 48;
  }}
/>
```

#### Image Caching

```typescript
import FastImage from '@d11/react-native-fast-image';

// Preload thumbnails
FastImage.preload(
  entries
    .filter(e => e.mimeType?.startsWith('image/'))
    .slice(0, 20)
    .map(e => ({ uri: `mobilecli://thumbnail/${e.path}` }))
);
```

#### Debounced Search

```typescript
const debouncedSearch = useMemo(
  () => debounce((query: string) => {
    search(query);
  }, 300),
  [search]
);
```

### 8.3 Network Optimization

#### Message Batching

```typescript
// Batch multiple requests
const batchRequests = async (requests: FileSystemRequest[]) => {
  return Promise.all(requests.map(sendFileSystemRequest));
};

// Prefetch adjacent directories
const prefetchSiblings = async (currentPath: string) => {
  const parentPath = getParentPath(currentPath);
  const siblings = await listDirectory(parentPath);

  // Prefetch first 3 sibling directories
  const siblingDirs = siblings
    .filter(e => e.isDirectory && e.path !== currentPath)
    .slice(0, 3);

  await batchRequests(
    siblingDirs.map(d => ({ type: 'list_directory', path: d.path }))
  );
};
```

---

## 9. Cross-Platform Considerations

### 9.1 Path Handling

```rust
// cli/src/filesystem/platform.rs

use std::path::{Path, PathBuf, MAIN_SEPARATOR};

/// Normalize path separators for cross-platform compatibility
pub fn normalize_path(path: &str) -> PathBuf {
    // Convert Windows backslashes to forward slashes for consistency
    let normalized = path.replace('\\', "/");
    PathBuf::from(normalized)
}

/// Get path separator for current platform
pub fn path_separator() -> char {
    MAIN_SEPARATOR
}

/// Check if path is absolute (handles both Unix and Windows)
pub fn is_absolute(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute() ||
    // Windows: C:\ or \\server
    (cfg!(windows) && (
        path.to_string_lossy().chars().nth(1) == Some(':') ||
        path.to_string_lossy().starts_with("\\\\")
    ))
}

/// Get home directory cross-platform
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}
```

### 9.2 Hidden Files

```rust
// Different hidden file conventions per OS

#[cfg(unix)]
pub fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn is_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;

    // Check Windows hidden attribute
    if let Ok(metadata) = path.metadata() {
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        return metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
    }

    // Also check dot-prefix for compatibility
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}
```

### 9.3 Permissions

```rust
#[cfg(unix)]
pub fn format_permissions(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode();
    let user = format_rwx((mode >> 6) & 0o7);
    let group = format_rwx((mode >> 3) & 0o7);
    let other = format_rwx(mode & 0o7);

    format!("{}{}{}", user, group, other)
}

#[cfg(windows)]
pub fn format_permissions(metadata: &std::fs::Metadata) -> String {
    let readonly = metadata.permissions().readonly();
    if readonly { "r--" } else { "rw-" }.to_string()
}

fn format_rwx(bits: u32) -> String {
    format!(
        "{}{}{}",
        if bits & 4 != 0 { "r" } else { "-" },
        if bits & 2 != 0 { "w" } else { "-" },
        if bits & 1 != 0 { "x" } else { "-" },
    )
}
```

### 9.4 Line Endings

```rust
/// Normalize line endings for cross-platform text files
pub fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace("\r", "\n")
}

/// Convert to platform-specific line endings
pub fn to_platform_line_endings(content: &str) -> String {
    #[cfg(windows)]
    {
        content.replace("\n", "\r\n")
    }
    #[cfg(not(windows))]
    {
        content.to_string()
    }
}
```

---

## 10. Edge Cases & Error Handling

### 10.1 Race Conditions

```rust
// Handle file deleted during read
async fn read_file_safe(path: &Path) -> Result<FileContent, FileSystemError> {
    match fs::read(path).await {
        Ok(content) => Ok(process_content(content)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(FileSystemError::NotFound {
                path: path.display().to_string(),
            })
        }
        Err(e) => Err(FileSystemError::IoError {
            message: e.to_string(),
        }),
    }
}

// Handle directory modified during listing
async fn list_directory_safe(path: &Path) -> Result<Vec<FileEntry>, FileSystemError> {
    let mut entries = Vec::new();
    let mut read_dir = fs::read_dir(path).await?;

    while let Some(entry_result) = read_dir.next_entry().await.transpose() {
        match entry_result {
            Ok(entry) => {
                // Entry might be deleted between readdir and stat
                match entry.metadata().await {
                    Ok(metadata) => {
                        entries.push(build_file_entry(&entry.path(), &metadata));
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        // File was deleted, skip it
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to stat {}: {}", entry.path().display(), e);
                        continue;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read directory entry: {}", e);
                continue;
            }
        }
    }

    Ok(entries)
}
```

### 10.2 File Encoding Issues

```rust
/// Detect and handle file encoding
pub fn read_file_with_encoding(buffer: &[u8]) -> Result<(String, &'static str), FileSystemError> {
    // Try UTF-8 first
    if let Ok(content) = std::str::from_utf8(buffer) {
        return Ok((content.to_string(), "utf-8"));
    }

    // Try UTF-8 with BOM
    if buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(content) = std::str::from_utf8(&buffer[3..]) {
            return Ok((content.to_string(), "utf-8-bom"));
        }
    }

    // Try UTF-16 LE
    if buffer.starts_with(&[0xFF, 0xFE]) {
        let utf16: Vec<u16> = buffer[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Ok(content) = String::from_utf16(&utf16) {
            return Ok((content, "utf-16-le"));
        }
    }

    // Try UTF-16 BE
    if buffer.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = buffer[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        if let Ok(content) = String::from_utf16(&utf16) {
            return Ok((content, "utf-16-be"));
        }
    }

    // Fall back to lossy UTF-8
    Ok((String::from_utf8_lossy(buffer).to_string(), "utf-8-lossy"))
}
```

### 10.3 Special Filenames

```rust
/// Handle special characters in filenames
pub fn sanitize_filename(name: &str) -> String {
    // Remove/replace characters that cause issues
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '\0'..='\x1f' => '_',  // Control characters
            _ => c,
        })
        .collect();

    // Handle reserved names on Windows
    let reserved = ["CON", "PRN", "AUX", "NUL",
                   "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
                   "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];

    let upper = sanitized.to_uppercase();
    if reserved.iter().any(|r| upper == *r || upper.starts_with(&format!("{}.", r))) {
        format!("_{}", sanitized)
    } else {
        sanitized
    }
}
```

### 10.4 Error Messages

```typescript
// mobile/utils/errorMessages.ts

export function getErrorMessage(error: FileSystemError): string {
  switch (error.type) {
    case 'not_found':
      return `File not found: ${getFilename(error.path)}`;

    case 'permission_denied':
      return error.reason || `Access denied: ${getFilename(error.path)}`;

    case 'path_traversal':
      return 'Invalid path: Access outside allowed directories is not permitted';

    case 'not_a_directory':
      return `${getFilename(error.path)} is not a folder`;

    case 'not_a_file':
      return `${getFilename(error.path)} is not a file`;

    case 'already_exists':
      return `${getFilename(error.path)} already exists`;

    case 'not_empty':
      return `Cannot delete ${getFilename(error.path)}: folder is not empty`;

    case 'file_too_large':
      return `File is too large (${formatSize(error.size)}). Maximum size is ${formatSize(error.max_size)}`;

    case 'io_error':
      return error.message || 'An unexpected error occurred';

    case 'invalid_encoding':
      return `Cannot read ${getFilename(error.path)}: invalid text encoding`;

    case 'operation_cancelled':
      return 'Operation was cancelled';

    case 'rate_limited':
      return `Too many requests. Please wait ${Math.ceil(error.retry_after_ms / 1000)} seconds`;

    default:
      return 'An unexpected error occurred';
  }
}

function getFilename(path: string): string {
  return path.split('/').pop() || path;
}

function formatSize(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB'];
  let size = bytes;
  let unitIndex = 0;

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }

  return `${size.toFixed(1)} ${units[unitIndex]}`;
}
```

---

## 11. UX/UI Design Guidelines

### 11.1 Navigation Patterns

```
┌────────────────────────────────────┐
│ ← │ 🏠 > Documents > Projects      │  ← Breadcrumb (tap to navigate)
├────────────────────────────────────┤
│ [🔍 Search...]            [⋮ Menu] │  ← Search + options
├────────────────────────────────────┤
│                                    │
│ 📁 src                     →       │  ← Folders first
│ 📁 tests                   →       │
│ ────────────────────────────────── │  ← Visual separator
│ 📄 main.rs            2.4 KB       │  ← Files with size
│ 📄 Cargo.toml         1.1 KB       │
│ 📄 README.md          3.2 KB       │
│                                    │
├────────────────────────────────────┤
│ [Sessions] [📁 Files] [Settings]   │  ← Bottom tabs
└────────────────────────────────────┘
```

### 11.2 Gestures

| Gesture | Action |
|---------|--------|
| Tap | Open file/folder |
| Long press | Enter selection mode, select item |
| Swipe left | Quick delete (with confirmation) |
| Swipe right | Quick info sheet |
| Pull down | Refresh directory |
| Pinch (images) | Zoom in/out |
| Double tap (images) | Toggle zoom |

### 11.3 Haptic Feedback

```typescript
import * as Haptics from 'expo-haptics';

// Selection feedback
const onSelect = () => {
  Haptics.selectionAsync();
};

// Success feedback (file saved, deleted, etc.)
const onSuccess = () => {
  Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
};

// Error feedback
const onError = () => {
  Haptics.notificationAsync(Haptics.NotificationFeedbackType.Error);
};

// Warning feedback (confirm delete)
const onWarning = () => {
  Haptics.notificationAsync(Haptics.NotificationFeedbackType.Warning);
};
```

### 11.4 Loading States

```typescript
// Skeleton loading for directory listing
function FileListSkeleton() {
  return (
    <View style={styles.skeleton}>
      {Array.from({ length: 10 }).map((_, i) => (
        <View key={i} style={styles.skeletonRow}>
          <View style={styles.skeletonIcon} />
          <View style={styles.skeletonText}>
            <View style={[styles.skeletonLine, { width: `${60 + Math.random() * 30}%` }]} />
            <View style={[styles.skeletonLine, { width: '40%', height: 10 }]} />
          </View>
        </View>
      ))}
    </View>
  );
}
```

### 11.5 Dark Mode

All icons and UI elements must work in both light and dark modes:

```typescript
const FILE_ICONS_LIGHT = {
  folder: '#FFB300',        // Amber
  file: '#757575',          // Gray
  image: '#4CAF50',         // Green
  video: '#F44336',         // Red
  audio: '#9C27B0',         // Purple
  document: '#2196F3',      // Blue
  code: '#FF5722',          // Deep Orange
  archive: '#795548',       // Brown
};

const FILE_ICONS_DARK = {
  folder: '#FFD54F',        // Lighter amber
  file: '#BDBDBD',          // Lighter gray
  image: '#81C784',         // Lighter green
  video: '#E57373',         // Lighter red
  audio: '#BA68C8',         // Lighter purple
  document: '#64B5F6',      // Lighter blue
  code: '#FF8A65',          // Lighter deep orange
  archive: '#A1887F',       // Lighter brown
};
```

---

## 12. Accessibility

### 12.1 Screen Reader Support

```typescript
<TouchableOpacity
  accessibilityRole="button"
  accessibilityLabel={`${entry.name}, ${entry.isDirectory ? 'folder' : 'file'}, ${
    entry.isDirectory ? '' : formatSize(entry.size)
  }`}
  accessibilityHint={entry.isDirectory ? 'Double tap to open folder' : 'Double tap to open file'}
  accessibilityState={{
    selected: isSelected,
  }}
>
  {/* ... */}
</TouchableOpacity>
```

### 12.2 Touch Target Sizes

```typescript
// Minimum 44x44 points (iOS) / 48x48 dp (Android)
const styles = StyleSheet.create({
  fileRow: {
    minHeight: 56,
    paddingVertical: 8,
    paddingHorizontal: 16,
  },
  iconButton: {
    width: 44,
    height: 44,
    justifyContent: 'center',
    alignItems: 'center',
  },
});
```

### 12.3 Focus Management

```typescript
// Announce directory changes
import { AccessibilityInfo } from 'react-native';

useEffect(() => {
  if (entries.length > 0) {
    AccessibilityInfo.announceForAccessibility(
      `${currentPath.split('/').pop()} folder, ${entries.length} items`
    );
  }
}, [currentPath, entries.length]);
```

---

## 13. Testing Strategy

### 13.1 Unit Tests (Rust)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_path_validation_blocks_traversal() {
        let config = FileSystemConfig::default();
        let validator = PathValidator::new(config);

        assert!(validator.validate("/../etc/passwd").is_err());
        assert!(validator.validate("/home/user/../../../etc/passwd").is_err());
    }

    #[test]
    fn test_path_validation_allows_valid_paths() {
        let temp = TempDir::new().unwrap();
        let config = FileSystemConfig {
            allowed_roots: vec![temp.path().to_path_buf()],
            ..Default::default()
        };
        let validator = PathValidator::new(config);

        std::fs::write(temp.path().join("test.txt"), "content").unwrap();

        assert!(validator.validate(
            &temp.path().join("test.txt").to_string_lossy()
        ).is_ok());
    }

    #[tokio::test]
    async fn test_list_directory_sorts_correctly() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("dir_a")).unwrap();
        std::fs::create_dir(temp.path().join("dir_b")).unwrap();
        std::fs::write(temp.path().join("file_a.txt"), "a").unwrap();
        std::fs::write(temp.path().join("file_b.txt"), "b").unwrap();

        let ops = FileOperations::new(/* ... */);
        let result = ops.list_directory(
            &temp.path().to_string_lossy(),
            false,
            Some(SortField::Name),
            Some(SortOrder::Asc),
        ).await.unwrap();

        // Directories should come first
        assert!(result.entries[0].is_directory);
        assert!(result.entries[1].is_directory);
        assert!(!result.entries[2].is_directory);
    }
}
```

### 13.2 Integration Tests (Mobile)

```typescript
// __tests__/fileSystem.test.ts
import { renderHook, act } from '@testing-library/react-hooks';
import { useFileSystemStore } from '../stores/fileSystemStore';

describe('FileSystemStore', () => {
  it('should navigate to directory and update entries', async () => {
    const { result } = renderHook(() => useFileSystemStore());

    await act(async () => {
      await result.current.navigate('/home/user/Documents');
    });

    expect(result.current.currentPath).toBe('/home/user/Documents');
    expect(result.current.entries.length).toBeGreaterThan(0);
  });

  it('should maintain navigation history', async () => {
    const { result } = renderHook(() => useFileSystemStore());

    await act(async () => {
      await result.current.navigate('/home/user');
      await result.current.navigate('/home/user/Documents');
      await result.current.navigate('/home/user/Documents/Projects');
    });

    expect(result.current.history).toHaveLength(3);

    act(() => {
      result.current.goBack();
    });

    expect(result.current.currentPath).toBe('/home/user/Documents');
  });
});
```

### 13.3 E2E Tests

```typescript
// e2e/fileBrowser.test.ts
describe('File Browser', () => {
  beforeAll(async () => {
    await device.launchApp();
  });

  it('should display files tab', async () => {
    await element(by.text('Files')).tap();
    await expect(element(by.id('file-browser'))).toBeVisible();
  });

  it('should navigate into folder on tap', async () => {
    await element(by.text('Documents')).tap();
    await expect(element(by.id('breadcrumb'))).toHaveText('Documents');
  });

  it('should open file viewer for text file', async () => {
    await element(by.text('README.md')).tap();
    await expect(element(by.id('file-viewer'))).toBeVisible();
  });
});
```

---

## 14. Implementation Phases

### Phase 1: Foundation (Week 1)
- [ ] Protocol messages in `protocol.rs`
- [ ] Security module with path validation
- [ ] Basic file operations (list, read)
- [ ] Mobile store setup
- [ ] Files tab with directory listing
- [ ] Basic navigation (tap folders)

### Phase 2: Core Features (Week 2)
- [ ] Write operations (create, delete, rename)
- [ ] Breadcrumb navigation
- [ ] Text file viewer
- [ ] Code editor (Monaco WebView)
- [ ] Pull-to-refresh
- [ ] Loading states

### Phase 3: Enhanced Viewing (Week 3)
- [ ] Markdown viewer with preview
- [ ] Image viewer with zoom
- [ ] PDF viewer
- [ ] Hex viewer for binary files
- [ ] MIME type detection

### Phase 4: Advanced Features (Week 4)
- [ ] File search
- [ ] File watching (real-time updates)
- [ ] Clipboard (copy/cut/paste)
- [ ] Multi-select mode
- [ ] Folder picker for session spawn

### Phase 5: Polish (Week 5)
- [ ] Offline caching
- [ ] Performance optimization
- [ ] Accessibility audit
- [ ] Error handling polish
- [ ] Dark mode refinement
- [ ] Haptic feedback

---

## 15. Appendices

### A. Rust Crate Versions

```toml
[dependencies]
walkdir = "2.5"
ignore = "0.4"
notify = "6.1"
notify-debouncer-mini = "0.4"
path_jail = "0.3"
infer = "0.15"
memmap2 = "0.9"
dashmap = "5.5"
rayon = "1.10"
base64 = "0.22"
md5 = "0.7"
glob-match = "0.2"
```

### B. npm Package Versions

```json
{
  "@shopify/flash-list": "^2.2.0",
  "@d11/react-native-fast-image": "^8.13.0",
  "react-native-webview": "^13.12.0",
  "react-native-pdf": "^6.7.0",
  "react-native-markdown-display": "^7.0.0",
  "expo-haptics": "~14.0.0",
  "zustand": "^4.5.0"
}
```

### C. File Type Icons

| Category | Extensions | Icon | Color |
|----------|------------|------|-------|
| Folder | (directory) | `folder` | Amber |
| Code | rs, py, js, ts, go, java, c, cpp, rb | `file-code` | Deep Orange |
| Markup | html, xml, svg | `file-code` | Orange |
| Style | css, scss, less | `file-code` | Pink |
| Data | json, yaml, toml, csv | `file-text` | Cyan |
| Document | md, txt, pdf, doc | `file-text` | Blue |
| Image | png, jpg, gif, svg, webp | `file-image` | Green |
| Video | mp4, mov, avi, mkv | `file-video` | Red |
| Audio | mp3, wav, flac, ogg | `file-music` | Purple |
| Archive | zip, tar, gz, 7z, rar | `file-archive` | Brown |
| Binary | exe, dll, so, dylib | `file` | Gray |

### D. Keyboard Shortcuts (Code Editor)

| Shortcut | Action |
|----------|--------|
| Cmd/Ctrl + S | Save file |
| Cmd/Ctrl + Z | Undo |
| Cmd/Ctrl + Shift + Z | Redo |
| Cmd/Ctrl + F | Find |
| Cmd/Ctrl + G | Go to line |
| Cmd/Ctrl + / | Toggle comment |

---

## 16. System-Wide Optimizations

This section provides a deep dive into optimization strategies across the entire MobileCLI stack, from Rust daemon to mobile client.

### 16.1 Rust Compiler Optimizations

#### Release Profile Configuration

```toml
# Cargo.toml - Optimal release settings

[profile.release]
# Link-Time Optimization - enables cross-crate inlining
lto = "fat"                    # "fat" = full LTO, best optimization
                               # "thin" = faster compile, good optimization

# Single codegen unit for maximum optimization
codegen-units = 1              # Default is 16, reducing enables more inlining

# Disable incremental compilation for release
incremental = false            # Ensures consistent optimization

# Optimization level
opt-level = 3                  # Maximum optimization (default for release)

# Strip symbols for smaller binary
strip = "symbols"              # Reduces binary size by ~30-50%

# Panic handling
panic = "abort"                # Smaller binary, no unwinding overhead

[profile.release.package."*"]
# Optimize dependencies aggressively
opt-level = 3
```

#### Target-Specific Compilation

```bash
# Build with CPU-native optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release

# For distribution, target baseline + specific features
RUSTFLAGS="-C target-cpu=x86-64-v3" cargo build --release  # AVX2 baseline

# Enable specific SIMD features
RUSTFLAGS="-C target-feature=+avx2,+fma" cargo build --release

# Profile-Guided Optimization (PGO)
# Step 1: Build instrumented binary
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release

# Step 2: Run with representative workload
./target/release/mobilecli  # Exercise typical operations

# Step 3: Merge profile data
llvm-profdata merge -o /tmp/pgo-data/merged.profdata /tmp/pgo-data

# Step 4: Build optimized binary
RUSTFLAGS="-Cprofile-use=/tmp/pgo-data/merged.profdata" cargo build --release
```

#### Memory Allocator Selection

```toml
# Cargo.toml - Use jemalloc for better performance

[target.'cfg(not(target_env = "msvc"))'.dependencies]
jemallocator = "0.5"

[target.'cfg(target_os = "linux")'.dependencies]
tikv-jemallocator = "0.5"      # Better maintained fork
```

```rust
// main.rs - Configure global allocator

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// For Windows, consider mimalloc
#[cfg(target_env = "msvc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

### 16.2 Rust Data Structure Optimizations

#### Arena Allocation for Request Processing

```rust
use bumpalo::Bump;

/// Process file operations with arena allocation
/// Avoids per-object heap allocations for short-lived data
pub async fn process_request_batch(requests: Vec<FileSystemRequest>) -> Vec<FileSystemResponse> {
    // Create arena for this batch - all allocations freed at once
    let arena = Bump::new();

    let mut responses = Vec::with_capacity(requests.len());

    for request in requests {
        // Allocate intermediate data in arena
        let path_str = arena.alloc_str(&request.path());
        let validated = validate_path_in_arena(&arena, path_str);

        let response = match process_single_request(&arena, request, validated).await {
            Ok(r) => r,
            Err(e) => FileSystemResponse::OperationError {
                operation: request.operation_name().to_string(),
                path: request.path().to_string(),
                error: e,
            },
        };
        responses.push(response);
    }

    responses
    // Arena automatically dropped here, freeing all allocations at once
}
```

#### SmallVec for Short Collections

```rust
use smallvec::SmallVec;

/// FileEntry with inline storage for common cases
pub struct OptimizedFileEntry {
    /// Name stored inline for names up to 64 bytes (covers 99% of filenames)
    pub name: SmallVec<[u8; 64]>,

    /// Path stored inline for paths up to 256 bytes
    pub path: SmallVec<[u8; 256]>,

    /// Extension stored inline (8 bytes covers all common extensions)
    pub extension: SmallVec<[u8; 8]>,

    pub is_directory: bool,
    pub size: u64,
    pub modified: u64,
}

/// Directory listing with inline storage for typical directories
pub struct OptimizedDirectoryListing {
    /// Most directories have < 100 files, store inline
    pub entries: SmallVec<[OptimizedFileEntry; 100]>,
    pub path: SmallVec<[u8; 256]>,
}
```

#### Compact String Representations

```rust
use compact_str::CompactString;

/// Use CompactString for strings up to 24 bytes (stored inline)
pub struct CompactFileEntry {
    /// Most filenames are < 24 chars
    pub name: CompactString,

    /// MIME types are typically short
    pub mime_type: Option<CompactString>,

    /// File extensions are very short
    pub extension: Option<CompactString>,

    pub size: u64,
    pub modified: u64,
    pub is_directory: bool,
}
```

#### Cache-Friendly Data Layout

```rust
/// Structure of Arrays (SoA) for better cache locality in bulk operations
pub struct DirectoryListingSoA {
    // Hot data - accessed together during sorting/filtering
    pub names: Vec<CompactString>,
    pub is_directory: Vec<bool>,
    pub sizes: Vec<u64>,
    pub modified: Vec<u64>,

    // Cold data - accessed only when displaying details
    pub paths: Vec<String>,
    pub mime_types: Vec<Option<String>>,
    pub permissions: Vec<Option<String>>,
}

impl DirectoryListingSoA {
    /// Sort by name with cache-friendly access pattern
    pub fn sort_by_name(&mut self) {
        // Create index array
        let mut indices: Vec<usize> = (0..self.names.len()).collect();

        // Sort indices by name (only touches names array)
        indices.sort_unstable_by(|&a, &b| {
            self.is_directory[b].cmp(&self.is_directory[a])
                .then_with(|| self.names[a].cmp(&self.names[b]))
        });

        // Reorder all arrays by sorted indices
        self.reorder_by_indices(&indices);
    }
}
```

### 16.3 Async I/O Optimizations

#### Tokio Runtime Configuration

```rust
use tokio::runtime::Builder;

fn create_optimized_runtime() -> tokio::runtime::Runtime {
    Builder::new_multi_thread()
        // Use all available cores
        .worker_threads(num_cpus::get())

        // Larger stack for file operations
        .thread_stack_size(4 * 1024 * 1024)  // 4MB stack

        // Enable I/O driver
        .enable_io()

        // Enable time driver for timeouts
        .enable_time()

        // Thread naming for debugging
        .thread_name("mobilecli-worker")

        // Callback when threads start
        .on_thread_start(|| {
            // Pin to CPU core for cache locality (optional)
            // core_affinity::set_for_current(core_id);
        })

        .build()
        .expect("Failed to create Tokio runtime")
}
```

#### Buffered I/O Patterns

```rust
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

const BUFFER_SIZE: usize = 64 * 1024;  // 64KB buffer

/// Optimized file reading with adaptive buffering
pub async fn read_file_optimized(path: &Path) -> io::Result<Vec<u8>> {
    let file = tokio::fs::File::open(path).await?;
    let metadata = file.metadata().await?;
    let size = metadata.len() as usize;

    // Pre-allocate exact size to avoid reallocations
    let mut content = Vec::with_capacity(size);

    // Use buffered reader for efficient syscalls
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
    reader.read_to_end(&mut content).await?;

    Ok(content)
}

/// Optimized file writing with buffering
pub async fn write_file_optimized(path: &Path, content: &[u8]) -> io::Result<()> {
    let file = tokio::fs::File::create(path).await?;
    let mut writer = BufWriter::with_capacity(BUFFER_SIZE, file);

    writer.write_all(content).await?;
    writer.flush().await?;

    Ok(())
}
```

#### Memory-Mapped I/O for Large Files

```rust
use memmap2::{Mmap, MmapOptions};
use std::fs::File;

/// Read large files with memory mapping (zero-copy)
pub fn read_large_file_mmap(path: &Path) -> io::Result<Mmap> {
    let file = File::open(path)?;

    // Safety: File must not be modified while mapped
    unsafe { MmapOptions::new().map(&file) }
}

/// Chunked reading from memory-mapped file
pub struct MmapChunkReader {
    mmap: Mmap,
    position: usize,
    chunk_size: usize,
}

impl MmapChunkReader {
    pub fn new(path: &Path, chunk_size: usize) -> io::Result<Self> {
        let mmap = read_large_file_mmap(path)?;
        Ok(Self { mmap, position: 0, chunk_size })
    }

    pub fn next_chunk(&mut self) -> Option<&[u8]> {
        if self.position >= self.mmap.len() {
            return None;
        }

        let end = (self.position + self.chunk_size).min(self.mmap.len());
        let chunk = &self.mmap[self.position..end];
        self.position = end;

        Some(chunk)
    }
}
```

### 16.4 JSON/Serialization Optimizations

#### SIMD-Accelerated JSON Parsing

```toml
# Cargo.toml
[dependencies]
simd-json = "0.14"
```

```rust
use simd_json::prelude::*;

/// Parse JSON with SIMD acceleration (2-3x faster than serde_json)
pub fn parse_message_simd(data: &mut [u8]) -> Result<ClientMessage, simd_json::Error> {
    simd_json::from_slice(data)
}

/// Serialize with simd-json
pub fn serialize_response_simd(response: &ServerMessage) -> Result<Vec<u8>, simd_json::Error> {
    simd_json::to_vec(response)
}
```

#### Pre-allocated Serialization Buffers

```rust
use std::io::Write;

/// Reusable serialization buffer to avoid allocations
pub struct SerializationBuffer {
    buffer: Vec<u8>,
}

impl SerializationBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64 * 1024),  // 64KB initial capacity
        }
    }

    pub fn serialize<T: serde::Serialize>(&mut self, value: &T) -> Result<&[u8], serde_json::Error> {
        self.buffer.clear();
        serde_json::to_writer(&mut self.buffer, value)?;
        Ok(&self.buffer)
    }
}

// Thread-local buffers for zero-contention serialization
thread_local! {
    static SERIALIZE_BUFFER: std::cell::RefCell<SerializationBuffer> =
        std::cell::RefCell::new(SerializationBuffer::new());
}

pub fn serialize_fast<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    SERIALIZE_BUFFER.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.serialize(value).map(|s| s.to_vec())
    })
}
```

#### Binary Protocol Option (MessagePack)

```toml
# Cargo.toml
[dependencies]
rmp-serde = "1.1"
```

```rust
use rmp_serde::{Deserializer, Serializer};

/// MessagePack serialization - 20-50% smaller than JSON
pub fn serialize_msgpack<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut buf = Vec::with_capacity(1024);
    value.serialize(&mut Serializer::new(&mut buf))?;
    Ok(buf)
}

pub fn deserialize_msgpack<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    rmp_serde::from_slice(data)
}
```

### 16.5 Compression Optimizations

#### LZ4 for Real-Time Transfer

```toml
# Cargo.toml
[dependencies]
lz4_flex = "0.11"
```

```rust
use lz4_flex::{compress_prepend_size, decompress_size_prepended};

/// LZ4 compression - optimized for speed, ~400MB/s compression
pub fn compress_lz4(data: &[u8]) -> Vec<u8> {
    compress_prepend_size(data)
}

pub fn decompress_lz4(compressed: &[u8]) -> Result<Vec<u8>, lz4_flex::block::DecompressError> {
    decompress_size_prepended(compressed)
}

/// Compress file content for transfer if beneficial
pub fn maybe_compress(data: &[u8], mime_type: &str) -> (Vec<u8>, bool) {
    // Don't compress already-compressed formats
    if is_compressed_format(mime_type) || data.len() < 1024 {
        return (data.to_vec(), false);
    }

    let compressed = compress_lz4(data);

    // Only use compression if it actually helps
    if compressed.len() < data.len() * 9 / 10 {  // At least 10% reduction
        (compressed, true)
    } else {
        (data.to_vec(), false)
    }
}

fn is_compressed_format(mime: &str) -> bool {
    matches!(mime,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" |
        "video/mp4" | "video/webm" |
        "audio/mp3" | "audio/aac" | "audio/ogg" |
        "application/zip" | "application/gzip" | "application/x-7z-compressed"
    )
}
```

#### Zstd for Better Compression Ratios

```toml
# Cargo.toml
[dependencies]
zstd = "0.13"
```

```rust
use zstd::stream::{encode_all, decode_all};

/// Zstd compression - better ratios for storage/bandwidth-constrained scenarios
pub fn compress_zstd(data: &[u8], level: i32) -> io::Result<Vec<u8>> {
    encode_all(data, level)  // Level 1-3 for speed, 10+ for size
}

pub fn decompress_zstd(compressed: &[u8]) -> io::Result<Vec<u8>> {
    decode_all(compressed)
}

/// Adaptive compression based on content type
pub fn adaptive_compress(data: &[u8], prefer_speed: bool) -> (Vec<u8>, CompressionType) {
    if data.len() < 1024 {
        return (data.to_vec(), CompressionType::None);
    }

    if prefer_speed {
        (compress_lz4(data), CompressionType::Lz4)
    } else {
        match compress_zstd(data, 3) {
            Ok(compressed) => (compressed, CompressionType::Zstd),
            Err(_) => (data.to_vec(), CompressionType::None),
        }
    }
}
```

### 16.6 Directory Traversal Optimizations

#### Parallel Directory Walking

```rust
use rayon::prelude::*;
use ignore::WalkBuilder;

/// High-performance parallel directory listing
pub fn list_directory_parallel(path: &Path, max_entries: usize) -> Vec<FileEntry> {
    let entries: Vec<_> = WalkBuilder::new(path)
        .max_depth(Some(1))
        .hidden(false)
        .git_ignore(false)
        .build()
        .par_bridge()  // Convert to parallel iterator
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.path() == path {
                return None;  // Skip root directory itself
            }
            build_file_entry(entry.path()).ok()
        })
        .take_any(max_entries)  // Parallel take
        .collect();

    entries
}

/// Parallel file search with early termination
pub fn search_files_parallel(
    root: &Path,
    pattern: &str,
    max_results: usize,
) -> Vec<FileEntry> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let found_count = AtomicUsize::new(0);
    let pattern = glob::Pattern::new(pattern).unwrap();

    WalkBuilder::new(root)
        .git_ignore(true)
        .build_parallel()
        .run(|| {
            let pattern = pattern.clone();
            let found_count = &found_count;

            Box::new(move |entry| {
                // Check if we've found enough
                if found_count.load(Ordering::Relaxed) >= max_results {
                    return ignore::WalkState::Quit;
                }

                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                let name = entry.file_name().to_string_lossy();
                if pattern.matches(&name) {
                    found_count.fetch_add(1, Ordering::Relaxed);
                }

                ignore::WalkState::Continue
            })
        });

    // ... collect results
    vec![]
}
```

#### Batch Metadata Fetching

```rust
#[cfg(target_os = "macos")]
use std::os::macos::fs::MetadataExt;

/// macOS: Use getattrlistbulk for 1600x fewer syscalls on large directories
#[cfg(target_os = "macos")]
pub fn list_directory_bulk_macos(path: &Path) -> io::Result<Vec<FileEntry>> {
    // getattrlistbulk can fetch metadata for entire directories in one syscall
    // This is ~4x faster on NVMe, ~50x faster on network storage

    // Note: Requires direct FFI call to getattrlistbulk()
    // For implementation, see: https://developer.apple.com/documentation/kernel/getattrlistbulk

    unimplemented!("Requires FFI implementation")
}

/// Linux: Use getdents64 for efficient directory reading
#[cfg(target_os = "linux")]
pub fn list_directory_efficient_linux(path: &Path) -> io::Result<Vec<FileEntry>> {
    // Standard readdir is already using getdents64 under the hood
    // But we can batch stat() calls using io_uring for better performance

    std::fs::read_dir(path)?
        .par_bridge()  // Parallel stat() calls
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let metadata = e.metadata().ok()?;
            Some(build_file_entry_from_metadata(e.path(), metadata))
        })
        .collect()
}
```

### 16.7 WebSocket Optimizations

#### Message Batching

```rust
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

/// Batch outgoing messages for efficiency
pub struct MessageBatcher {
    tx: mpsc::Sender<ServerMessage>,
    batch_size: usize,
    batch_timeout: Duration,
}

impl MessageBatcher {
    pub async fn run(
        mut rx: mpsc::Receiver<ServerMessage>,
        ws_sender: impl WebSocketSender,
        batch_size: usize,
        batch_timeout: Duration,
    ) {
        let mut batch = Vec::with_capacity(batch_size);
        let mut flush_interval = interval(batch_timeout);

        loop {
            tokio::select! {
                Some(msg) = rx.recv() => {
                    batch.push(msg);

                    if batch.len() >= batch_size {
                        Self::flush_batch(&ws_sender, &mut batch).await;
                    }
                }

                _ = flush_interval.tick() => {
                    if !batch.is_empty() {
                        Self::flush_batch(&ws_sender, &mut batch).await;
                    }
                }
            }
        }
    }

    async fn flush_batch(ws: &impl WebSocketSender, batch: &mut Vec<ServerMessage>) {
        if batch.len() == 1 {
            // Single message, send directly
            ws.send(&batch[0]).await;
        } else {
            // Multiple messages, send as batch
            ws.send_batch(batch).await;
        }
        batch.clear();
    }
}
```

#### Binary Frame Support

```rust
use tokio_tungstenite::tungstenite::Message;

/// Efficient binary message handling
pub fn create_binary_message(response: &ServerMessage) -> Result<Message, rmp_serde::encode::Error> {
    let data = serialize_msgpack(response)?;
    Ok(Message::Binary(data))
}

/// Parse incoming message (text or binary)
pub fn parse_message(msg: Message) -> Result<ClientMessage, ParseError> {
    match msg {
        Message::Text(text) => {
            serde_json::from_str(&text).map_err(ParseError::Json)
        }
        Message::Binary(data) => {
            rmp_serde::from_slice(&data).map_err(ParseError::MsgPack)
        }
        _ => Err(ParseError::UnsupportedMessageType),
    }
}
```

#### Connection Keep-Alive

```rust
/// Optimized WebSocket connection handling
pub struct OptimizedConnection {
    /// Ping interval to keep connection alive
    ping_interval: Duration,

    /// Read timeout
    read_timeout: Duration,

    /// Enable TCP_NODELAY for lower latency
    tcp_nodelay: bool,

    /// Enable TCP keepalive
    tcp_keepalive: Option<Duration>,
}

impl Default for OptimizedConnection {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(30),
            read_timeout: Duration::from_secs(60),
            tcp_nodelay: true,  // Disable Nagle's algorithm for lower latency
            tcp_keepalive: Some(Duration::from_secs(60)),
        }
    }
}
```

### 16.8 Mobile Client Optimizations

#### FlashList Configuration

```typescript
import { FlashList, ListRenderItem } from '@shopify/flash-list';

// Optimal FlashList configuration for file browser
const FileList: React.FC<{ entries: FileEntry[] }> = ({ entries }) => {
  const renderItem: ListRenderItem<FileEntry> = useCallback(({ item }) => (
    <FileListItem entry={item} />
  ), []);

  return (
    <FlashList
      data={entries}
      renderItem={renderItem}

      // Essential performance settings
      estimatedItemSize={56}        // Average row height
      estimatedListSize={{
        width: Dimensions.get('window').width,
        height: Dimensions.get('window').height - 150,
      }}

      // Offscreen rendering
      drawDistance={300}            // Render 300px offscreen

      // Key extraction
      keyExtractor={useCallback((item: FileEntry) => item.path, [])}

      // Item type for recycling
      getItemType={useCallback((item: FileEntry) =>
        item.isDirectory ? 'directory' : 'file', []
      )}

      // Avoid re-renders
      extraData={undefined}

      // Native driver animations
      overScrollMode="never"
    />
  );
};
```

#### Memoization Patterns

```typescript
import { memo, useMemo, useCallback } from 'react';

// Memoize file list items to prevent unnecessary re-renders
const FileListItem = memo<{ entry: FileEntry; onPress: (path: string) => void }>(
  ({ entry, onPress }) => {
    const handlePress = useCallback(() => {
      onPress(entry.path);
    }, [entry.path, onPress]);

    const formattedSize = useMemo(() =>
      entry.isDirectory ? '' : formatSize(entry.size),
      [entry.isDirectory, entry.size]
    );

    const formattedDate = useMemo(() =>
      formatRelativeDate(entry.modified),
      [entry.modified]
    );

    return (
      <TouchableOpacity onPress={handlePress} style={styles.item}>
        <FileIcon mimeType={entry.mimeType} isDirectory={entry.isDirectory} />
        <View style={styles.info}>
          <Text style={styles.name} numberOfLines={1}>{entry.name}</Text>
          <Text style={styles.meta}>{formattedSize} · {formattedDate}</Text>
        </View>
      </TouchableOpacity>
    );
  },
  // Custom comparison for memo
  (prev, next) =>
    prev.entry.path === next.entry.path &&
    prev.entry.modified === next.entry.modified
);
```

#### Image Optimization

```typescript
import FastImage from '@d11/react-native-fast-image';

// Aggressive image caching for thumbnails
const FileThumbnail: React.FC<{ path: string; mimeType: string }> = ({ path, mimeType }) => {
  if (!mimeType?.startsWith('image/')) {
    return <FileTypeIcon mimeType={mimeType} />;
  }

  return (
    <FastImage
      source={{
        uri: `mobilecli://thumbnail/${encodeURIComponent(path)}`,
        priority: FastImage.priority.normal,
        cache: FastImage.cacheControl.immutable,  // Cache aggressively
      }}
      resizeMode={FastImage.resizeMode.cover}
      style={styles.thumbnail}
    />
  );
};

// Preload visible thumbnails
const preloadThumbnails = (entries: FileEntry[]) => {
  const imageEntries = entries
    .filter(e => e.mimeType?.startsWith('image/'))
    .slice(0, 20);  // First 20 images

  FastImage.preload(
    imageEntries.map(e => ({
      uri: `mobilecli://thumbnail/${encodeURIComponent(e.path)}`,
    }))
  );
};
```

#### Bundle Optimization

```javascript
// metro.config.js - Optimize bundle size

const { getDefaultConfig } = require('expo/metro-config');

const config = getDefaultConfig(__dirname);

config.transformer = {
  ...config.transformer,
  minifierConfig: {
    // Terser options for smaller bundle
    compress: {
      drop_console: true,  // Remove console.log in production
      drop_debugger: true,
      pure_funcs: ['console.log', 'console.info'],
    },
    mangle: true,
  },
};

// Tree-shaking for unused exports
config.resolver.resolveRequest = (context, moduleName, platform) => {
  // Custom resolution for dead code elimination
  return context.resolveRequest(context, moduleName, platform);
};

module.exports = config;
```

### 16.9 Battery & Resource Efficiency

#### Adaptive Refresh Rates

```typescript
import { AppState } from 'react-native';

// Reduce activity when app is backgrounded or battery is low
const useAdaptivePolling = (normalInterval: number) => {
  const [interval, setInterval] = useState(normalInterval);

  useEffect(() => {
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'background') {
        setInterval(normalInterval * 10);  // 10x slower in background
      } else if (state === 'active') {
        setInterval(normalInterval);
      }
    });

    return () => subscription.remove();
  }, [normalInterval]);

  return interval;
};
```

#### Network Efficiency

```typescript
// Batch WebSocket messages to reduce radio wake-ups
class MessageQueue {
  private queue: ClientMessage[] = [];
  private flushTimeout: NodeJS.Timeout | null = null;
  private readonly batchDelay = 50;  // 50ms batching window

  enqueue(message: ClientMessage) {
    this.queue.push(message);

    if (!this.flushTimeout) {
      this.flushTimeout = setTimeout(() => this.flush(), this.batchDelay);
    }
  }

  private flush() {
    if (this.queue.length === 0) return;

    if (this.queue.length === 1) {
      this.send(this.queue[0]);
    } else {
      this.sendBatch(this.queue);
    }

    this.queue = [];
    this.flushTimeout = null;
  }
}
```

#### Memory Pressure Handling

```typescript
import { DeviceEventEmitter, Platform } from 'react-native';

// React to memory pressure warnings
const useMemoryWarning = (onLowMemory: () => void) => {
  useEffect(() => {
    if (Platform.OS === 'ios') {
      const subscription = DeviceEventEmitter.addListener(
        'memoryWarning',
        onLowMemory
      );
      return () => subscription.remove();
    }

    // Android: Use MemoryInfo API
    return () => {};
  }, [onLowMemory]);
};

// Clear caches when memory is low
const handleLowMemory = () => {
  // Clear file cache
  useFileSystemStore.getState().clearCache();

  // Clear image cache
  FastImage.clearMemoryCache();

  // Force garbage collection hint
  global.gc?.();
};
```

### 16.10 Caching Strategies

#### Multi-Level Cache Architecture

```typescript
// L1: In-memory LRU cache (fast, limited size)
// L2: AsyncStorage/SQLite (persistent, larger)
// L3: Server (always authoritative)

class MultiLevelCache {
  private l1: LRUCache<string, CachedFile>;
  private l2: AsyncStorage;

  constructor() {
    this.l1 = new LRUCache({
      max: 100,                    // 100 files
      maxSize: 50 * 1024 * 1024,   // 50MB total
      sizeCalculation: (file) => file.content.length,
      ttl: 5 * 60 * 1000,          // 5 minute TTL
    });
  }

  async get(path: string): Promise<CachedFile | null> {
    // L1: Memory
    const l1Hit = this.l1.get(path);
    if (l1Hit && !this.isStale(l1Hit)) {
      return l1Hit;
    }

    // L2: Persistent storage
    const l2Hit = await this.l2.getItem(`file:${path}`);
    if (l2Hit) {
      const file = JSON.parse(l2Hit);
      if (!this.isStale(file)) {
        // Promote to L1
        this.l1.set(path, file);
        return file;
      }
    }

    // L3: Server (handled by caller)
    return null;
  }

  async set(path: string, file: CachedFile): Promise<void> {
    // Write to L1
    this.l1.set(path, file);

    // Write to L2 (async, don't block)
    this.l2.setItem(`file:${path}`, JSON.stringify(file)).catch(() => {});
  }

  private isStale(file: CachedFile): boolean {
    return Date.now() - file.cachedAt > 5 * 60 * 1000;  // 5 minutes
  }
}
```

#### Cache Invalidation

```typescript
// Subscribe to file change events for cache invalidation
const useCacheInvalidation = () => {
  const { invalidateCache } = useFileSystemStore();

  useEffect(() => {
    const handleFileChange = (change: FileChange) => {
      if (change.change_type === 'modified' || change.change_type === 'deleted') {
        invalidateCache(change.path);
      }
    };

    // Subscribe to server events
    wsClient.on('file_changed', handleFileChange);

    return () => wsClient.off('file_changed', handleFileChange);
  }, [invalidateCache]);
};
```

### 16.11 Benchmarking & Profiling

#### Rust Benchmarks

```rust
// benches/file_operations.rs
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_list_directory(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_directory");

    for size in [100, 1000, 10000].iter() {
        let temp_dir = create_temp_dir_with_files(*size);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &temp_dir,
            |b, dir| {
                b.iter(|| list_directory(dir.path()))
            },
        );
    }

    group.finish();
}

fn bench_json_serialization(c: &mut Criterion) {
    let entries: Vec<FileEntry> = (0..1000)
        .map(|i| create_sample_entry(i))
        .collect();

    let mut group = c.benchmark_group("serialization");

    group.bench_function("serde_json", |b| {
        b.iter(|| serde_json::to_vec(&entries))
    });

    group.bench_function("simd_json", |b| {
        b.iter(|| simd_json::to_vec(&entries))
    });

    group.bench_function("msgpack", |b| {
        b.iter(|| rmp_serde::to_vec(&entries))
    });

    group.finish();
}

criterion_group!(benches, bench_list_directory, bench_json_serialization);
criterion_main!(benches);
```

#### Mobile Performance Monitoring

```typescript
import { PerformanceObserver } from 'react-native-performance';

// Monitor render performance
const useRenderPerformance = (componentName: string) => {
  useEffect(() => {
    const startTime = performance.now();

    return () => {
      const duration = performance.now() - startTime;
      if (duration > 16.67) {  // Longer than one frame (60fps)
        console.warn(`${componentName} render took ${duration.toFixed(2)}ms`);
      }
    };
  });
};

// Track WebSocket latency
const trackLatency = () => {
  const start = Date.now();

  wsClient.send({ type: 'ping' });

  return new Promise<number>((resolve) => {
    const handler = (msg: ServerMessage) => {
      if (msg.type === 'pong') {
        wsClient.off('message', handler);
        resolve(Date.now() - start);
      }
    };
    wsClient.on('message', handler);
  });
};
```

---

## 17. Performance Targets & SLAs

### 17.1 Latency Targets

| Operation | Target | Maximum |
|-----------|--------|---------|
| Directory listing (≤100 files) | < 50ms | 100ms |
| Directory listing (≤1000 files) | < 100ms | 200ms |
| Directory listing (≤10000 files) | < 500ms | 1000ms |
| File read (≤1MB) | < 50ms | 100ms |
| File read (≤10MB) | < 200ms | 500ms |
| File write (≤1MB) | < 100ms | 200ms |
| Search (≤10000 files) | < 1000ms | 2000ms |
| WebSocket round-trip | < 20ms | 50ms |

### 17.2 Memory Targets

| Component | Target | Maximum |
|-----------|--------|---------|
| Daemon idle | < 10MB | 20MB |
| Daemon per connection | < 5MB | 10MB |
| Mobile app idle | < 50MB | 100MB |
| Mobile app browsing | < 100MB | 200MB |
| File cache (mobile) | 50MB | 100MB |

### 17.3 Battery Impact

| Scenario | Target |
|----------|--------|
| Idle connection (background) | < 1% per hour |
| Active browsing | < 5% per hour |
| File watching | < 2% per hour |

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0.0 | 2026-01-29 | MobileCLI Team | Initial specification |
| 1.1.0 | 2026-01-29 | MobileCLI Team | Added comprehensive optimization section |

---

*This document is the authoritative source for the MobileCLI File Browser & Editor feature. All implementation should follow these specifications unless explicitly approved changes are documented.*
