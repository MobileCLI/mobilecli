//! Session management
//!
//! Tracks active streaming sessions and persists session info.

use crate::platform;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Session info stored in the sessions file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub project_path: String,
    pub ws_port: u16,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

/// Get the sessions file path (cross-platform)
fn sessions_file() -> PathBuf {
    platform::config_dir().join("sessions.json")
}

/// Ensure the config directory exists
fn ensure_config_dir() -> std::io::Result<()> {
    let sessions_path = sessions_file();
    if let Some(parent) = sessions_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Load all sessions from file
pub fn load_sessions() -> Vec<SessionInfo> {
    let path = sessions_file();
    if !path.exists() {
        return Vec::new();
    }

    fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Save sessions to file
pub fn save_sessions(sessions: &[SessionInfo]) -> std::io::Result<()> {
    ensure_config_dir()?;
    let path = sessions_file();
    let data = serde_json::to_string_pretty(sessions)?;
    fs::write(path, data)
}

/// Check if a process is still alive (cross-platform via platform module)
///
/// Uses kill(pid, 0) signal test on Unix, Windows API on Windows.
fn is_process_alive(pid: u32) -> bool {
    platform::is_process_alive(pid)
}

/// Get list of active sessions for API response
pub fn list_active_sessions() -> Vec<SessionInfo> {
    load_sessions()
        .into_iter()
        .filter(|s| is_process_alive(s.pid))
        .collect()
}
