//! WebSocket protocol messages
//!
//! Compatible with the MobileCLI mobile app protocol.

use serde::{Deserialize, Serialize};

/// Messages sent from mobile client to server
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        auth_token: Option<String>,
        client_version: String,
    },
    Subscribe {
        session_id: String,
    },
    Unsubscribe {
        session_id: String,
    },
    SendInput {
        session_id: String,
        text: String,
        #[serde(default)]
        raw: bool,
        #[serde(default)]
        client_msg_id: Option<String>,
    },
    /// Resize PTY - mobile sends terminal dimensions
    PtyResize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    /// Heartbeat ping
    Ping,
    /// Request list of available sessions
    GetSessions,
    /// Rename a session
    RenameSession {
        session_id: String,
        new_name: String,
    },
    /// Register push notification token
    RegisterPushToken {
        token: String,
        token_type: String, // "expo" | "apns" | "fcm"
        platform: String,   // "ios" | "android"
    },
    /// Tool approval response from mobile
    ToolApproval {
        session_id: String,
        response: String, // "yes" | "yes_always" | "no"
    },
    /// Request session history (scrollback buffer)
    GetSessionHistory {
        session_id: String,
        #[serde(default)]
        max_bytes: Option<usize>,
    },
    /// Spawn a new session from mobile
    SpawnSession {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        working_dir: Option<String>,
    },
    // ---- File system requests ----
    ListDirectory {
        request_id: String,
        path: String,
        #[serde(default)]
        include_hidden: bool,
        #[serde(default)]
        sort_by: Option<SortField>,
        #[serde(default)]
        sort_order: Option<SortOrder>,
    },
    ReadFile {
        request_id: String,
        path: String,
        #[serde(default)]
        offset: Option<u64>,
        #[serde(default)]
        length: Option<u64>,
        #[serde(default)]
        encoding: FileEncoding,
    },
    WriteFile {
        request_id: String,
        path: String,
        content: String,
        #[serde(default)]
        encoding: FileEncoding,
        #[serde(default)]
        create_parents: bool,
    },
    CreateDirectory {
        request_id: String,
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    DeletePath {
        request_id: String,
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    RenamePath {
        request_id: String,
        old_path: String,
        new_path: String,
    },
    CopyPath {
        request_id: String,
        source: String,
        destination: String,
        #[serde(default)]
        recursive: bool,
    },
    GetFileInfo {
        request_id: String,
        path: String,
    },
    SearchFiles {
        request_id: String,
        path: String,
        pattern: String,
        #[serde(default)]
        content_pattern: Option<String>,
        #[serde(default)]
        max_depth: Option<u32>,
        #[serde(default)]
        max_results: Option<u32>,
    },
    WatchDirectory {
        request_id: String,
        path: String,
    },
    UnwatchDirectory {
        request_id: String,
        path: String,
    },
    GetHomeDirectory {
        request_id: String,
    },
    GetAllowedRoots {
        request_id: String,
    },
    ReadFileChunk {
        request_id: String,
        path: String,
        chunk_index: u64,
        #[serde(default)]
        chunk_size: Option<u64>,
    },
}

/// Messages sent from server to mobile client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        server_version: String,
        authenticated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        device_name: Option<String>,
    },
    Error {
        code: String,
        message: String,
    },
    /// Raw PTY bytes (base64 encoded) - preserves all ANSI codes and formatting
    PtyBytes {
        session_id: String,
        data: String, // base64 encoded
    },
    /// Session info
    SessionInfo {
        session_id: String,
        name: String,
        command: String,
        project_path: String,
        started_at: String,
    },
    /// List of available sessions
    Sessions {
        sessions: Vec<SessionListItem>,
    },
    /// Session ended
    SessionEnded {
        session_id: String,
        exit_code: i32,
    },
    /// Session renamed
    SessionRenamed {
        session_id: String,
        new_name: String,
    },
    /// PTY resized confirmation
    PtyResized {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    /// Heartbeat pong
    Pong,
    /// Session is waiting for user input (tool approval, question, etc.)
    WaitingForInput {
        session_id: String,
        timestamp: String,
        prompt_content: String,
        wait_type: String, // "tool_approval" | "plan_approval" | "clarifying_question" | "awaiting_response"
        cli_type: String,  // "claude" | "codex" | "gemini" | "opencode" | "terminal"
    },
    /// Waiting state cleared (user responded)
    WaitingCleared {
        session_id: String,
        timestamp: String,
    },
    /// Session history (scrollback buffer) for linked terminals
    SessionHistory {
        session_id: String,
        data: String, // base64 encoded
        total_bytes: usize,
    },
    /// Result of spawning a new session
    SpawnResult {
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    // ---- File system responses ----
    DirectoryListing {
        request_id: String,
        path: String,
        entries: Vec<FileEntry>,
        total_count: usize,
        truncated: bool,
    },
    FileContent {
        request_id: String,
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
        request_id: String,
        path: String,
        entry: FileEntry,
    },
    OperationSuccess {
        request_id: String,
        operation: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    OperationError {
        request_id: String,
        operation: String,
        path: String,
        error: FileSystemError,
    },
    SearchResults {
        request_id: String,
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
        request_id: String,
        path: String,
    },
    AllowedRoots {
        request_id: String,
        roots: Vec<String>,
    },
    FileChunk {
        request_id: String,
        path: String,
        chunk_index: u64,
        total_chunks: u64,
        total_size: u64,
        data: String,
        checksum: String,
        is_last: bool,
    },
}

/// Session list item for GetSessions response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListItem {
    pub session_id: String,
    pub name: String,
    pub command: String,
    pub project_path: String,
    pub ws_port: u16,
    pub started_at: String,
    /// Explicit CLI type identifier for mobile app disambiguation
    pub cli_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Name,
    Size,
    Modified,
    Type,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileEncoding {
    Utf8,
    Base64,
}

impl Default for FileEncoding {
    fn default() -> Self {
        FileEncoding::Utf8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: u64,
    pub modified: u64, // Unix timestamp ms
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub encoding: FileEncoding,
    pub mime_type: String,
    pub size: u64,
    pub modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Ignored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub entry: FileEntry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_matches: Option<Vec<ContentMatch>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMatch {
    pub line_number: u32,
    pub line_content: String,
    pub match_start: u32,
    pub match_end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChanged {
    pub path: String,
    pub change_type: ChangeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_entry: Option<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
    Renamed { from: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Connection info for QR code / pairing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// WebSocket URL (e.g., ws://192.168.1.100:9847)
    pub ws_url: String,
    /// Session ID
    pub session_id: String,
    /// Session name (optional)
    pub session_name: Option<String>,
    /// Optional encryption key (base64)
    pub encryption_key: Option<String>,
    /// Server version
    pub version: String,
    /// Device UUID (for multi-device support)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Device name/hostname (for display)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

impl ConnectionInfo {
    /// Encode as JSON for QR code (full format)
    pub fn to_qr_data(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!("Failed to serialize ConnectionInfo: {}", e);
                String::new()
            }
        }
    }

    /// Encode as compact string for QR code (smaller QR)
    /// Format: mobilecli://host:port?device_id=UUID&device_name=HOSTNAME
    ///
    /// Note: This format is for device-level pairing, not session-specific connections.
    /// The mobile app connects to the device and then fetches the session list via
    /// GetSessions. This enables multi-device support where one mobile app can link
    /// to multiple computers. Session-specific QR codes are no longer used as sessions
    /// are transient and device pairing is persistent.
    pub fn to_compact_qr(&self) -> String {
        // Extract host:port from ws_url
        let host_port = self
            .ws_url
            .strip_prefix("ws://")
            .or_else(|| self.ws_url.strip_prefix("wss://"))
            .unwrap_or(&self.ws_url);

        // Build URL with query parameters for device info
        let mut url = format!("mobilecli://{}", host_port);

        // Add query parameters for device info
        let mut params = Vec::new();
        if let Some(id) = &self.device_id {
            params.push(format!("device_id={}", urlencoding::encode(id)));
        }
        if let Some(name) = &self.device_name {
            params.push(format!("device_name={}", urlencoding::encode(name)));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        url
    }
}
