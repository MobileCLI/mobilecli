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

/// Register a new session
pub fn register_session(session: SessionInfo) -> std::io::Result<()> {
    let mut sessions = load_sessions();

    // Remove any dead sessions (process no longer exists)
    sessions.retain(|s| is_process_alive(s.pid));

    // Add the new session
    sessions.push(session);
    save_sessions(&sessions)
}

/// Unregister a session
pub fn unregister_session(session_id: &str) -> std::io::Result<()> {
    let mut sessions = load_sessions();
    sessions.retain(|s| s.session_id != session_id);
    save_sessions(&sessions)
}

/// Rename a session
pub fn rename_session(session_id: &str, new_name: &str) -> std::io::Result<bool> {
    let mut sessions = load_sessions();
    let mut found = false;

    for session in &mut sessions {
        if session.session_id == session_id {
            session.name = new_name.to_string();
            found = true;
            break;
        }
    }

    if found {
        save_sessions(&sessions)?;
    }
    Ok(found)
}

/// Get a session by ID
pub fn get_session(session_id: &str) -> Option<SessionInfo> {
    load_sessions()
        .into_iter()
        .find(|s| s.session_id == session_id && is_process_alive(s.pid))
}

/// Check if a process is still alive (cross-platform via platform module)
///
/// Uses kill(pid, 0) signal test on Unix, Windows API on Windows.
fn is_process_alive(pid: u32) -> bool {
    platform::is_process_alive(pid)
}

/// Show status of active sessions
pub fn show_status() {
    use colored::Colorize;

    let sessions = load_sessions();

    // Filter to only alive sessions
    let alive_sessions: Vec<_> = sessions
        .iter()
        .filter(|s| is_process_alive(s.pid))
        .collect();

    if alive_sessions.is_empty() {
        println!("{}", "No active streaming sessions.".dimmed());
        println!("\n{}", "Start a terminal with mobile streaming:".dimmed());
        println!("  {} mobilecli", "$".green());
        println!("  {} mobilecli -n \"My Project\"", "$".green());
        println!("  {} mobilecli claude", "$".green());
        return;
    }

    println!(
        "{} {} active session(s):\n",
        "●".green(),
        alive_sessions.len()
    );

    for session in alive_sessions {
        let duration = Utc::now()
            .signed_duration_since(session.started_at)
            .num_minutes();

        println!(
            "  {} {} {}",
            "→".cyan(),
            session.name.bold(),
            format!("({}m)", duration).dimmed()
        );
        println!(
            "    {} ws://localhost:{}",
            "WebSocket:".dimmed(),
            session.ws_port
        );
        println!(
            "    {} {} (PID: {})",
            "Command:".dimmed(),
            session.command,
            session.pid
        );
        println!("    {} {}", "Directory:".dimmed(), session.project_path);
        println!();
    }
}

/// Get list of active sessions for API response
pub fn list_active_sessions() -> Vec<SessionInfo> {
    load_sessions()
        .into_iter()
        .filter(|s| is_process_alive(s.pid))
        .collect()
}
