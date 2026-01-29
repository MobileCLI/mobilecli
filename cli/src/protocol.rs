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
