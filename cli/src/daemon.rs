//! Background daemon for MobileCLI
//!
//! Single WebSocket server that all terminal sessions stream to.
//! Mobile connects once and sees all active sessions.

use crate::detection::{
    detect_wait_event, strip_ansi_and_normalize, ApprovalModel, CliTracker, CliType, WaitType,
};
use crate::platform;
use crate::protocol::{ClientMessage, ServerMessage, SessionListItem};
use crate::session::{self, SessionInfo};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};

/// Shared HTTP client for push notifications (lazy initialized with timeout)
fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Default WebSocket port
pub const DEFAULT_PORT: u16 = 9847;

/// PID file path (cross-platform)
fn pid_file() -> PathBuf {
    platform::config_dir().join("daemon.pid")
}

/// Port file path (cross-platform)
fn port_file() -> PathBuf {
    platform::config_dir().join("daemon.port")
}

/// Get the running daemon's port (reads from port file)
pub fn get_port() -> Option<u16> {
    std::fs::read_to_string(port_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Check if daemon is running
pub fn is_running() -> bool {
    let pid_path = pid_file();
    if !pid_path.exists() {
        return false;
    }

    if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            return is_process_alive(pid);
        }
    }
    false
}

/// Check if a process is alive (cross-platform via platform module)
fn is_process_alive(pid: u32) -> bool {
    platform::is_process_alive(pid)
}

/// Get daemon PID
pub fn get_pid() -> Option<u32> {
    std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Waiting state for a session
#[derive(Debug, Clone)]
pub struct WaitingState {
    pub wait_type: WaitType, // normalized waiting type
    pub prompt_content: String,
    pub timestamp: chrono::DateTime<Utc>,
    pub approval_model: ApprovalModel,
    pub prompt_hash: u64,
}

/// Push notification token
#[derive(Debug, Clone)]
pub struct PushToken {
    pub token: String,
    pub token_type: String, // "expo" | "apns" | "fcm"
    pub platform: String,   // "ios" | "android"
}

/// Default scrollback buffer size (64KB)
const DEFAULT_SCROLLBACK_MAX_BYTES: usize = 64 * 1024;

/// Active PTY session
pub struct PtySession {
    pub session_id: String,
    pub name: String,
    pub command: String,
    pub project_path: String,
    pub started_at: chrono::DateTime<Utc>,
    pub input_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub resize_tx: mpsc::UnboundedSender<(u16, u16)>,
    pub waiting_state: Option<WaitingState>,
    pub cli_tracker: CliTracker,
    pub last_wait_hash: Option<u64>,
    /// Scrollback buffer for session history (for linked terminals)
    /// Uses VecDeque for efficient front truncation when buffer is full
    pub scrollback: VecDeque<u8>,
    /// Maximum scrollback buffer size
    pub scrollback_max_bytes: usize,
}

/// Daemon shared state
pub struct DaemonState {
    pub sessions: HashMap<String, PtySession>,
    pub mobile_clients: HashMap<SocketAddr, mpsc::UnboundedSender<Message>>,
    pub pty_broadcast: broadcast::Sender<(String, Vec<u8>)>,
    pub port: u16, // The actual port the daemon is running on
    pub push_tokens: Vec<PushToken>,
    pub mobile_views: HashMap<SocketAddr, std::collections::HashSet<String>>,
    pub session_view_counts: HashMap<String, usize>,
    /// Device UUID (for multi-device support)
    pub device_id: Option<String>,
    /// Device name (hostname)
    pub device_name: Option<String>,
}

impl DaemonState {
    pub fn new(port: u16) -> Self {
        let (pty_broadcast, _) = broadcast::channel(256);

        // Load device info from config
        let (device_id, device_name) = crate::setup::load_config()
            .map(|c| (Some(c.device_id), Some(c.device_name)))
            .unwrap_or((None, None));

        Self {
            sessions: HashMap::new(),
            mobile_clients: HashMap::new(),
            pty_broadcast,
            port,
            push_tokens: Vec::new(),
            mobile_views: HashMap::new(),
            session_view_counts: HashMap::new(),
            device_id,
            device_name,
        }
    }
}

pub type SharedState = Arc<RwLock<DaemonState>>;

/// Start the daemon (blocking - run in background)
pub async fn run(port: u16) -> std::io::Result<()> {
    // Write PID file
    let pid_path = pid_file();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, std::process::id().to_string())?;

    // Write port file so wrapper/QR can find the actual port
    let port_path = port_file();
    std::fs::write(&port_path, port.to_string())?;

    let state: SharedState = Arc::new(RwLock::new(DaemonState::new(port)));

    // Start WebSocket server on all interfaces (0.0.0.0)
    // This is intentional - mobile clients need network access to connect.
    // Security model: Access is controlled at the network level via:
    // - Local network: Only devices on same WiFi can connect
    // - Tailscale: Only authenticated Tailscale network members can connect
    // Users explicitly choose their connection mode in setup wizard.
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Daemon WebSocket server on port {}", port);

    // Run the main loop with platform-specific signal handling
    #[cfg(unix)]
    run_server_loop_unix(listener, state).await;

    #[cfg(not(unix))]
    run_server_loop_ctrlc_only(listener, state).await;

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(&port_path);
    Ok(())
}

/// Server loop with Unix signal handling (SIGTERM + Ctrl+C)
#[cfg(unix)]
async fn run_server_loop_unix(listener: TcpListener, state: SharedState) {
    use tokio::signal::unix::{signal, SignalKind};

    // Try to set up SIGTERM handler, fall back to Ctrl+C only if it fails
    let sigterm_result = signal(SignalKind::terminate());
    if sigterm_result.is_err() {
        tracing::warn!(
            "Failed to set up SIGTERM handler: {:?}. Only Ctrl+C will work for shutdown.",
            sigterm_result.err()
        );
        // Fall back to generic loop with just Ctrl+C
        run_server_loop_ctrlc_only(listener, state).await;
        return;
    }
    let mut sigterm = sigterm_result.unwrap();

    loop {
        tokio::select! {
            result = listener.accept() => {
                if let Ok((stream, addr)) = result {
                    let state = state.clone();
                    tokio::spawn(handle_connection(stream, addr, state));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Daemon shutting down (Ctrl+C)");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("Daemon shutting down (SIGTERM)");
                break;
            }
        }
    }
}

/// Server loop with Ctrl+C only (fallback or non-Unix)
async fn run_server_loop_ctrlc_only(listener: TcpListener, state: SharedState) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                if let Ok((stream, addr)) = result {
                    let state = state.clone();
                    tokio::spawn(handle_connection(stream, addr, state));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Daemon shutting down (Ctrl+C)");
                break;
            }
        }
    }
}

/// Handle WebSocket connection (could be mobile client or PTY session)
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws = accept_async(stream).await?;
    let (tx, mut rx) = ws.split();

    // Wait for first message to determine client type
    let first_msg = rx.next().await;

    match first_msg {
        Some(Ok(Message::Text(text))) => {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                if msg.get("type").and_then(|v| v.as_str()) == Some("register_pty") {
                    // This is a PTY session registering
                    return handle_pty_session(msg, tx, rx, addr, state).await;
                }
            }
            // Assume it's a mobile client
            handle_mobile_client(Some(text), tx, rx, addr, state).await
        }
        _ => Ok(()),
    }
}

/// Handle mobile client connection
async fn handle_mobile_client(
    first_msg: Option<String>,
    mut tx: futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
    mut rx: futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<TcpStream>>,
    addr: SocketAddr,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!("Mobile client connected: {}", addr);

    let (client_tx, mut client_rx) = mpsc::unbounded_channel::<Message>();

    // Register client and get broadcast receiver
    let mut pty_rx = {
        let mut st = state.write().await;
        st.mobile_clients.insert(addr, client_tx);
        st.pty_broadcast.subscribe()
    };

    // Send welcome with device info
    let (device_id, device_name) = {
        let st = state.read().await;
        (st.device_id.clone(), st.device_name.clone())
    };
    let welcome = ServerMessage::Welcome {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        authenticated: true,
        device_id,
        device_name,
    };
    tx.send(Message::Text(serde_json::to_string(&welcome)?))
        .await?;

    // Send sessions list
    send_sessions_list(&state, &mut tx).await?;

    // Send current waiting states for all sessions (for late-joining clients)
    send_waiting_states(&state, &mut tx).await?;

    // Process first message if it was a client message
    if let Some(text) = first_msg {
        if let Ok(msg) = serde_json::from_str::<ClientMessage>(&text) {
            process_client_msg(msg, &state, &mut tx, addr).await?;
        }
    }

    loop {
        tokio::select! {
            // PTY output
            result = pty_rx.recv() => {
                match result {
                    Ok((session_id, data)) => {
                        let msg = ServerMessage::PtyBytes {
                            session_id,
                            data: BASE64.encode(&data),
                        };
                        if tx.send(Message::Text(serde_json::to_string(&msg)?)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }

            // Queued messages
            Some(msg) = client_rx.recv() => {
                if tx.send(msg).await.is_err() {
                    break;
                }
            }

            // Client messages
            result = rx.next() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<ClientMessage>(&text) {
                            process_client_msg(msg, &state, &mut tx, addr).await?;
                        }
                    }
                    Some(Ok(Message::Ping(d))) => { let _ = tx.send(Message::Pong(d)).await; }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Unregister
    cleanup_mobile_views(&state, addr).await;
    state.write().await.mobile_clients.remove(&addr);
    tracing::info!("Mobile client disconnected: {}", addr);
    Ok(())
}

/// Handle PTY session registration
async fn handle_pty_session(
    reg_msg: serde_json::Value,
    mut tx: futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
    mut rx: futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<TcpStream>>,
    _addr: SocketAddr,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut exit_code: i32 = 0;
    let session_id = reg_msg["session_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("Missing or empty session_id in registration")?
        .to_string();
    let name = reg_msg["name"].as_str().unwrap_or("Terminal").to_string();
    let command = reg_msg["command"].as_str().unwrap_or("shell").to_string();
    let project_path = reg_msg["project_path"].as_str().unwrap_or("").to_string();

    tracing::info!("PTY session registered: {} ({})", name, session_id);

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<(u16, u16)>();

    // Register session
    let pty_broadcast = {
        let mut cli_tracker = CliTracker::new();
        cli_tracker.update_from_command(&command);

        let mut st = state.write().await;
        st.sessions.insert(
            session_id.clone(),
            PtySession {
                session_id: session_id.clone(),
                name: name.clone(),
                command,
                project_path,
                started_at: Utc::now(),
                input_tx,
                resize_tx,
                waiting_state: None,
                cli_tracker,
                last_wait_hash: None,
                scrollback: VecDeque::new(),
                scrollback_max_bytes: DEFAULT_SCROLLBACK_MAX_BYTES,
            },
        );
        st.pty_broadcast.clone()
    };

    // Notify mobile clients and persist to file
    broadcast_sessions_update(&state).await;
    persist_sessions_to_file(&state).await;

    // Send ACK
    tx.send(Message::Text(r#"{"type":"registered"}"#.to_string()))
        .await?;

    // Buffer for detecting waiting state patterns (ANSI-stripped, normalized)
    let mut output_buffer = String::new();
    const BUFFER_MAX_CHARS: usize = 4000; // Keep last N chars for pattern matching

    loop {
        tokio::select! {
            // PTY output from terminal wrapper
            result = rx.next() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                            if msg["type"].as_str() == Some("pty_output") {
                                if let Some(data) = msg["data"].as_str() {
                                    if let Ok(bytes) = BASE64.decode(data) {
                                        let _ = pty_broadcast.send((session_id.clone(), bytes.clone()));

                                        // Accumulate scrollback for session history (linked terminals)
                                        // Uses VecDeque for efficient front truncation
                                        {
                                            let mut st = state.write().await;
                                            if let Some(session) = st.sessions.get_mut(&session_id) {
                                                session.scrollback.extend(bytes.iter().copied());
                                                // Truncate from front if over limit (VecDeque is O(1) per pop)
                                                while session.scrollback.len() > session.scrollback_max_bytes {
                                                    session.scrollback.pop_front();
                                                }
                                            }
                                        }

                                        let text = String::from_utf8_lossy(&bytes);
                                        let normalized_chunk = strip_ansi_and_normalize(&text);

                                        if !normalized_chunk.is_empty() {
                                            output_buffer.push_str(&normalized_chunk);
                                            truncate_to_max_chars(&mut output_buffer, BUFFER_MAX_CHARS);

                                            // Update CLI tracker based on output
                                            let cli_type = {
                                                let mut st = state.write().await;
                                                if let Some(session) = st.sessions.get_mut(&session_id) {
                                                    session.cli_tracker.update_from_output(&normalized_chunk);
                                                    session.cli_tracker.current()
                                                } else {
                                                    CliType::Terminal
                                                }
                                            };

                                            // Check for waiting state patterns
                                            tracing::debug!("Checking for wait event, cli_type: {:?}, buffer_len: {}", cli_type, output_buffer.len());
                                            if let Some(wait_event) = detect_wait_event(&output_buffer, cli_type) {
                                                tracing::info!("Detected wait event: {:?} for session {}", wait_event.wait_type, session_id);
                                                let should_notify = {
                                                    let mut st = state.write().await;
                                                    if let Some(session) = st.sessions.get_mut(&session_id) {
                                                        let is_new = session.waiting_state.as_ref().map(|w| {
                                                            w.prompt_hash != wait_event.prompt_hash || w.wait_type != wait_event.wait_type
                                                        }).unwrap_or(true);
                                                        if is_new {
                                                            session.waiting_state = Some(WaitingState {
                                                                wait_type: wait_event.wait_type,
                                                                prompt_content: wait_event.prompt.clone(),
                                                                timestamp: Utc::now(),
                                                                approval_model: wait_event.approval_model,
                                                                prompt_hash: wait_event.prompt_hash,
                                                            });
                                                            session.last_wait_hash = Some(wait_event.prompt_hash);
                                                        }
                                                        is_new
                                                    } else {
                                                        false
                                                    }
                                                };

                                                if should_notify {
                                                    // Broadcast to mobile clients
                                                    broadcast_waiting_for_input(&state, &session_id).await;

                                                    // Send push notifications (async to avoid blocking PTY)
                                                    let tokens = {
                                                        let st = state.read().await;
                                                        st.push_tokens.clone()
                                                    };
                                                    let session_id_clone = session_id.clone();
                                                    let name_clone = name.clone();
                                                    tokio::spawn(async move {
                                                        let (title, body) = build_notification_text(cli_type, &name_clone, &wait_event);
                                                        send_push_notifications(&tokens, &title, &body, &session_id_clone).await;
                                                    });
                                                }
                                            } else {
                                                // If previously waiting, clear on meaningful output that is not a waiting prompt
                                                let should_clear = {
                                                    let mut st = state.write().await;
                                                    if let Some(session) = st.sessions.get_mut(&session_id) {
                                                        if session.waiting_state.is_some() && normalized_chunk.trim().chars().count() >= 10 {
                                                            session.waiting_state = None;
                                                            session.last_wait_hash = None;
                                                            true
                                                        } else {
                                                            false
                                                        }
                                                    } else {
                                                        false
                                                    }
                                                };

                                                if should_clear {
                                                    broadcast_waiting_cleared(&state, &session_id).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if msg["type"].as_str() == Some("session_ended") {
                                exit_code = msg["exit_code"].as_i64().unwrap_or(0) as i32;
                                tracing::info!("PTY session {} ended (exit_code={})", session_id, exit_code);
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let _ = pty_broadcast.send((session_id.clone(), data));
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }

            // Input from mobile to send to PTY
            Some(input) = input_rx.recv() => {
                let msg = serde_json::json!({
                    "type": "input",
                    "data": BASE64.encode(&input),
                });
                if tx.send(Message::Text(msg.to_string())).await.is_err() {
                    break;
                }

                // Clear waiting state when user sends input
                {
                    let mut st = state.write().await;
                    if let Some(session) = st.sessions.get_mut(&session_id) {
                        if session.waiting_state.is_some() {
                            session.waiting_state = None;
                            session.last_wait_hash = None;
                            drop(st);
                            broadcast_waiting_cleared(&state, &session_id).await;
                        }
                    }
                }

                // Clear output buffer on input
                output_buffer.clear();
            }

            // Resize from mobile
            Some((cols, rows)) = resize_rx.recv() => {
                let msg = serde_json::json!({
                    "type": "resize",
                    "cols": cols,
                    "rows": rows,
                });
                if tx.send(Message::Text(msg.to_string())).await.is_err() {
                    break;
                }
            }
        }
    }

    // Unregister session
    {
        let mut st = state.write().await;
        st.sessions.remove(&session_id);

        // Notify about session end
        let msg = ServerMessage::SessionEnded {
            session_id: session_id.clone(),
            exit_code,
        };
        let msg_str = serde_json::to_string(&msg)?;
        for client in st.mobile_clients.values() {
            let _ = client.send(Message::Text(msg_str.clone()));
        }
    }

    // Broadcast updated sessions list to all clients
    broadcast_sessions_update(&state).await;

    // Update persisted sessions
    persist_sessions_to_file(&state).await;

    tracing::info!("PTY session ended: {}", session_id);
    Ok(())
}

/// Validate command name - only allow known safe CLI commands
fn is_allowed_command(command: &str) -> bool {
    const ALLOWED_COMMANDS: &[&str] = &[
        "claude", "codex", "gemini", "opencode", "bash", "zsh", "sh", "fish", "nu", "pwsh",
        "python", "python3", "node", "ruby",
    ];
    // Get base command name (handle paths like /usr/bin/bash)
    let base = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command);
    ALLOWED_COMMANDS.contains(&base)
}

/// Validate that a string is safe for shell interpolation
/// Rejects newlines, null bytes, and other problematic characters
fn is_shell_safe(s: &str) -> bool {
    !s.contains('\n')
        && !s.contains('\r')
        && !s.contains('\0')
        && !s.contains('`')
        && !s.contains("$(")
}

/// Spawn a new session from mobile request
async fn spawn_session_from_mobile(
    command: &str,
    args: &[String],
    name: Option<&str>,
    working_dir: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Security: Validate command is in allowlist
    if !is_allowed_command(command) {
        return Err(format!("Command '{}' is not in the allowed list", command).into());
    }

    // Security: Validate all inputs are shell-safe (no injection)
    if !is_shell_safe(command) {
        return Err("Command contains unsafe characters".into());
    }
    for arg in args {
        if !is_shell_safe(arg) {
            return Err("Argument contains unsafe characters".into());
        }
    }
    if let Some(n) = name {
        if !is_shell_safe(n) {
            return Err("Name contains unsafe characters".into());
        }
    }
    if let Some(dir) = working_dir {
        if !is_shell_safe(dir) {
            return Err("Working directory contains unsafe characters".into());
        }
        // Security: Validate working directory exists and is a directory
        let path = std::path::Path::new(dir);
        if !path.is_absolute() {
            return Err("Working directory must be an absolute path".into());
        }
        if !path.is_dir() {
            return Err("Working directory does not exist or is not a directory".into());
        }
    }

    // Detect terminal emulator
    let terminal = detect_terminal_emulator()?;

    // Build the command to run inside the terminal
    let session_name = name.unwrap_or(command);
    let mut wrap_cmd = format!(
        "mobilecli wrap --name '{}'",
        session_name.replace("'", "'\\''")
    );

    if let Some(dir) = working_dir {
        wrap_cmd.push_str(&format!(" --dir '{}'", dir.replace("'", "'\\''")));
    }

    wrap_cmd.push(' ');
    wrap_cmd.push_str(command);

    for arg in args {
        wrap_cmd.push(' ');
        wrap_cmd.push_str(&format!("'{}'", arg.replace("'", "'\\''")));
    }

    tracing::info!("Spawning session: {} via {}", wrap_cmd, terminal.name);

    // Build terminal command based on detected emulator
    let mut cmd = std::process::Command::new(&terminal.binary);

    match terminal.name.as_str() {
        "kitty" => {
            cmd.args(["--", "sh", "-c", &wrap_cmd]);
        }
        "alacritty" => {
            cmd.args(["-e", "sh", "-c", &wrap_cmd]);
        }
        "gnome-terminal" | "tilix" => {
            cmd.args(["--", "sh", "-c", &wrap_cmd]);
        }
        "konsole" => {
            cmd.args(["-e", "sh", "-c", &wrap_cmd]);
        }
        "xterm" | "urxvt" => {
            cmd.args(["-e", "sh", "-c", &wrap_cmd]);
        }
        "wezterm" => {
            cmd.args(["start", "--", "sh", "-c", &wrap_cmd]);
        }
        "iterm" => {
            // macOS iTerm2 - use osascript to open a new window
            let script = format!(
                r#"tell application "iTerm"
                    create window with default profile
                    tell current session of current window
                        write text "{}"
                    end tell
                end tell"#,
                wrap_cmd.replace("\"", "\\\"")
            );
            cmd = std::process::Command::new("osascript");
            cmd.args(["-e", &script]);
        }
        "terminal" => {
            // macOS Terminal.app
            let script = format!(
                r#"tell application "Terminal"
                    do script "{}"
                    activate
                end tell"#,
                wrap_cmd.replace("\"", "\\\"")
            );
            cmd = std::process::Command::new("osascript");
            cmd.args(["-e", &script]);
        }
        _ => {
            // Generic fallback: try -e flag
            cmd.args(["-e", "sh", "-c", &wrap_cmd]);
        }
    }

    // Set working directory if specified
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    // Spawn detached
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // Create new session to detach from parent
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}

/// Terminal emulator detection result
struct TerminalInfo {
    name: String,
    binary: String,
}

/// Detect available terminal emulator
fn detect_terminal_emulator() -> Result<TerminalInfo, Box<dyn std::error::Error + Send + Sync>> {
    // Check for common terminal emulators in order of preference
    let terminals = [
        ("kitty", "kitty"),
        ("alacritty", "alacritty"),
        ("wezterm", "wezterm"),
        ("gnome-terminal", "gnome-terminal"),
        ("konsole", "konsole"),
        ("tilix", "tilix"),
        ("xterm", "xterm"),
        ("urxvt", "urxvt"),
    ];

    // Check TERM_PROGRAM environment variable first (macOS)
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        match term_program.to_lowercase().as_str() {
            "iterm.app" => {
                return Ok(TerminalInfo {
                    name: "iterm".to_string(),
                    binary: "osascript".to_string(),
                });
            }
            "apple_terminal" => {
                return Ok(TerminalInfo {
                    name: "terminal".to_string(),
                    binary: "osascript".to_string(),
                });
            }
            _ => {}
        }
    }

    // Check which terminals are available
    for (name, binary) in &terminals {
        if which::which(binary).is_ok() {
            return Ok(TerminalInfo {
                name: name.to_string(),
                binary: binary.to_string(),
            });
        }
    }

    // macOS fallback to Terminal.app
    #[cfg(target_os = "macos")]
    {
        return Ok(TerminalInfo {
            name: "terminal".to_string(),
            binary: "osascript".to_string(),
        });
    }

    #[cfg(not(target_os = "macos"))]
    Err("No supported terminal emulator found".into())
}

/// Process a message from mobile client
async fn process_client_msg(
    msg: ClientMessage,
    state: &SharedState,
    tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match msg {
        ClientMessage::Hello { client_version, .. } => {
            // Already sent Welcome on connect, but log the client version
            tracing::debug!("Client hello, version: {}", client_version);
        }
        ClientMessage::Subscribe { session_id } => {
            tracing::debug!("Client subscribed to session: {}", session_id);
            let mut st = state.write().await;
            let entry = st.mobile_views.entry(addr).or_default();
            if entry.insert(session_id.clone()) {
                let count = st
                    .session_view_counts
                    .entry(session_id.clone())
                    .or_insert(0);
                *count += 1;
            }
        }
        ClientMessage::Unsubscribe { session_id } => {
            tracing::debug!("Client unsubscribed from session: {}", session_id);
            let mut st = state.write().await;
            if let Some(entry) = st.mobile_views.get_mut(&addr) {
                if entry.remove(&session_id) {
                    if let Some(count) = st.session_view_counts.get_mut(&session_id) {
                        if *count > 0 {
                            *count -= 1;
                        }
                        if *count == 0 {
                            st.session_view_counts.remove(&session_id);
                            drop(st);
                            restore_pty_size(state, &session_id).await;
                            return Ok(());
                        }
                    }
                }
            }
        }
        ClientMessage::SendInput {
            session_id, text, ..
        } => {
            let st = state.read().await;
            if let Some(session) = st.sessions.get(&session_id) {
                let _ = session.input_tx.send(text.into_bytes());
            }
        }
        ClientMessage::PtyResize {
            session_id,
            cols,
            rows,
        } => {
            let st = state.read().await;
            let is_restore = cols == 0 && rows == 0;
            let has_viewers = st
                .session_view_counts
                .get(&session_id)
                .copied()
                .unwrap_or(0)
                > 0;
            if !is_restore && !has_viewers {
                tracing::debug!(
                    "Ignoring PTY resize for {} (no active mobile viewers)",
                    session_id
                );
                return Ok(());
            }
            if let Some(session) = st.sessions.get(&session_id) {
                let _ = session.resize_tx.send((cols, rows));
            }
        }
        ClientMessage::Ping => {
            tx.send(Message::Text(serde_json::to_string(&ServerMessage::Pong)?))
                .await?;
        }
        ClientMessage::GetSessions => {
            send_sessions_list(state, tx).await?;
        }
        ClientMessage::RenameSession {
            session_id,
            new_name,
        } => {
            let renamed = {
                let mut st = state.write().await;
                if let Some(session) = st.sessions.get_mut(&session_id) {
                    session.name = new_name.clone();
                    true
                } else {
                    false
                }
            };

            if renamed {
                // Send confirmation
                let msg = ServerMessage::SessionRenamed {
                    session_id: session_id.clone(),
                    new_name: new_name.clone(),
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;

                // Broadcast updated sessions list to all clients
                broadcast_sessions_update(state).await;

                // Update persisted sessions file
                persist_sessions_to_file(state).await;

                tracing::info!("Session {} renamed to '{}'", session_id, new_name);
            } else {
                let msg = ServerMessage::Error {
                    code: "session_not_found".to_string(),
                    message: format!("Session {} not found", session_id),
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
            }
        }
        ClientMessage::RegisterPushToken {
            token,
            token_type,
            platform,
        } => {
            let mut st = state.write().await;
            // Remove existing token with same value to avoid duplicates
            st.push_tokens.retain(|t| t.token != token);
            st.push_tokens.push(PushToken {
                token: token.clone(),
                token_type: token_type.clone(),
                platform: platform.clone(),
            });
            tracing::info!("Registered push token ({}/{})", token_type, platform);
        }
        ClientMessage::ToolApproval {
            session_id,
            response,
        } => {
            let maybe_input = {
                let mut st = state.write().await;
                if let Some(session) = st.sessions.get_mut(&session_id) {
                    let model = session
                        .waiting_state
                        .as_ref()
                        .map(|w| w.approval_model)
                        .unwrap_or_else(|| session.cli_tracker.current().default_approval_model());
                    approval_input_for(model, response.as_str())
                } else {
                    None
                }
            };

            let mut cleared = false;
            if let Some(input) = maybe_input {
                let mut st = state.write().await;
                if let Some(session) = st.sessions.get_mut(&session_id) {
                    let _ = session.input_tx.send(input.as_bytes().to_vec());
                    session.waiting_state = None;
                    session.last_wait_hash = None;
                    cleared = true;
                }
            } else {
                tracing::warn!(
                    "Tool approval ignored (no applicable approval model) for session {}",
                    session_id
                );
            }

            if cleared {
                broadcast_waiting_cleared(state, &session_id).await;
            }
        }
        ClientMessage::GetSessionHistory {
            session_id,
            max_bytes,
        } => {
            let (data, total_bytes) = {
                let st = state.read().await;
                if let Some(session) = st.sessions.get(&session_id) {
                    let max = max_bytes.unwrap_or(session.scrollback_max_bytes);
                    let total = session.scrollback.len();
                    let skip = total.saturating_sub(max);
                    // VecDeque doesn't support direct slicing, so collect the tail
                    let bytes: Vec<u8> = session.scrollback.iter().skip(skip).copied().collect();
                    (BASE64.encode(&bytes), total)
                } else {
                    (String::new(), 0)
                }
            };

            let msg = ServerMessage::SessionHistory {
                session_id,
                data,
                total_bytes,
            };
            tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
        }
        ClientMessage::SpawnSession {
            command,
            args,
            name,
            working_dir,
        } => {
            let result =
                spawn_session_from_mobile(&command, &args, name.as_deref(), working_dir.as_deref())
                    .await;
            let msg = match result {
                Ok(()) => ServerMessage::SpawnResult {
                    success: true,
                    session_id: None, // Session ID comes via session_info when PTY registers
                    error: None,
                },
                Err(e) => ServerMessage::SpawnResult {
                    success: false,
                    session_id: None,
                    error: Some(e.to_string()),
                },
            };
            tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
        }
    }
    Ok(())
}

/// Send sessions list to a client
async fn send_sessions_list(
    state: &SharedState,
    tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let st = state.read().await;
    let port = st.port;
    let items: Vec<SessionListItem> = st
        .sessions
        .values()
        .map(|s| SessionListItem {
            session_id: s.session_id.clone(),
            name: s.name.clone(),
            command: s.command.clone(),
            project_path: s.project_path.clone(),
            ws_port: port,
            started_at: s.started_at.to_rfc3339(),
            cli_type: s.cli_tracker.current().as_str().to_string(),
        })
        .collect();
    let msg = ServerMessage::Sessions { sessions: items };
    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
    Ok(())
}

/// Broadcast sessions update to all mobile clients
async fn broadcast_sessions_update(state: &SharedState) {
    let st = state.read().await;
    let port = st.port;
    let items: Vec<SessionListItem> = st
        .sessions
        .values()
        .map(|s| SessionListItem {
            session_id: s.session_id.clone(),
            name: s.name.clone(),
            command: s.command.clone(),
            project_path: s.project_path.clone(),
            ws_port: port,
            started_at: s.started_at.to_rfc3339(),
            cli_type: s.cli_tracker.current().as_str().to_string(),
        })
        .collect();
    let msg = ServerMessage::Sessions { sessions: items };
    if let Ok(msg_str) = serde_json::to_string(&msg) {
        for client in st.mobile_clients.values() {
            let _ = client.send(Message::Text(msg_str.clone()));
        }
    }
}

/// Persist daemon sessions to file for status command
async fn persist_sessions_to_file(state: &SharedState) {
    let st = state.read().await;
    let port = st.port;
    let sessions: Vec<SessionInfo> = st
        .sessions
        .values()
        .map(|s| SessionInfo {
            session_id: s.session_id.clone(),
            name: s.name.clone(),
            command: s.command.clone(),
            args: vec![],
            project_path: s.project_path.clone(),
            ws_port: port,
            pid: std::process::id(), // daemon PID since we manage all sessions
            started_at: s.started_at,
        })
        .collect();
    if let Err(e) = session::save_sessions(&sessions) {
        tracing::warn!("Failed to persist sessions: {}", e);
    }
}

/// Broadcast waiting_for_input to all mobile clients
async fn broadcast_waiting_for_input(state: &SharedState, session_id: &str) {
    let st = state.read().await;
    let session = match st.sessions.get(session_id) {
        Some(s) => s,
        None => return,
    };
    let waiting = match session.waiting_state.as_ref() {
        Some(w) => w,
        None => return,
    };

    let msg = ServerMessage::WaitingForInput {
        session_id: session_id.to_string(),
        timestamp: waiting.timestamp.to_rfc3339(),
        prompt_content: waiting.prompt_content.clone(),
        wait_type: waiting.wait_type.as_str().to_string(),
        cli_type: session.cli_tracker.current().as_str().to_string(),
    };
    if let Ok(msg_str) = serde_json::to_string(&msg) {
        for client in st.mobile_clients.values() {
            let _ = client.send(Message::Text(msg_str.clone()));
        }
    }
}

/// Broadcast waiting_cleared to all mobile clients
async fn broadcast_waiting_cleared(state: &SharedState, session_id: &str) {
    let st = state.read().await;
    let msg = ServerMessage::WaitingCleared {
        session_id: session_id.to_string(),
        timestamp: Utc::now().to_rfc3339(),
    };
    if let Ok(msg_str) = serde_json::to_string(&msg) {
        for client in st.mobile_clients.values() {
            let _ = client.send(Message::Text(msg_str.clone()));
        }
    }
}

/// Send current waiting states to a newly connected mobile client.
async fn send_waiting_states(
    state: &SharedState,
    tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let st = state.read().await;
    for session in st.sessions.values() {
        if let Some(waiting) = &session.waiting_state {
            let msg = ServerMessage::WaitingForInput {
                session_id: session.session_id.clone(),
                timestamp: waiting.timestamp.to_rfc3339(),
                prompt_content: waiting.prompt_content.clone(),
                wait_type: waiting.wait_type.as_str().to_string(),
                cli_type: session.cli_tracker.current().as_str().to_string(),
            };
            tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
        }
    }
    Ok(())
}

fn approval_input_for(model: ApprovalModel, response: &str) -> Option<&'static str> {
    match model {
        ApprovalModel::Numbered => match response {
            "yes" => Some("1\n"),
            "yes_always" => Some("2\n"),
            "no" => Some("3\n"),
            _ => None,
        },
        ApprovalModel::YesNo => match response {
            "yes" | "yes_always" => Some("y\n"),
            "no" => Some("n\n"),
            _ => None,
        },
        ApprovalModel::Arrow => match response {
            "yes" => Some("\r"),
            "yes_always" => Some("\x1b[C\r"),
            "no" => Some("\x1b[C\x1b[C\r"),
            _ => None,
        },
        ApprovalModel::None => None,
    }
}

fn truncate_to_max_chars(input: &mut String, max_chars: usize) {
    let len = input.chars().count();
    if len <= max_chars {
        return;
    }
    let trimmed: String = input.chars().skip(len - max_chars).collect();
    *input = trimmed;
}

fn build_notification_text(
    cli_type: CliType,
    session_name: &str,
    event: &crate::detection::WaitEvent,
) -> (String, String) {
    let cli_label = match cli_type {
        CliType::Claude => "Claude",
        CliType::Codex => "Codex",
        CliType::Gemini => "Gemini",
        CliType::OpenCode => "OpenCode",
        CliType::Terminal | CliType::Unknown => "CLI",
    };

    let title = match event.wait_type {
        WaitType::ToolApproval => "Tool Approval Needed",
        WaitType::PlanApproval => "Plan Approval Needed",
        WaitType::ClarifyingQuestion => "Question from CLI",
        WaitType::AwaitingResponse => "Awaiting Your Response",
    };

    let body = match event.wait_type {
        WaitType::ClarifyingQuestion => {
            let snippet = event.prompt.chars().take(100).collect::<String>();
            format!("{}: {}", cli_label, snippet)
        }
        WaitType::ToolApproval => format!("{} needs permission to proceed", cli_label),
        WaitType::PlanApproval => format!("{} has a plan ready for review", cli_label),
        WaitType::AwaitingResponse => format!("{} is waiting for input", cli_label),
    };

    let title_with_session = format!("{} · {}", session_name, title);
    (title_with_session, body)
}

async fn cleanup_mobile_views(state: &SharedState, addr: SocketAddr) {
    let sessions_to_restore = {
        let mut st = state.write().await;
        let sessions = match st.mobile_views.remove(&addr) {
            Some(s) => s,
            None => return,
        };
        let mut restore = Vec::new();
        for session_id in sessions {
            if let Some(count) = st.session_view_counts.get_mut(&session_id) {
                if *count > 0 {
                    *count -= 1;
                }
                if *count == 0 {
                    st.session_view_counts.remove(&session_id);
                    restore.push(session_id);
                }
            }
        }
        restore
    };

    for session_id in sessions_to_restore {
        restore_pty_size(state, &session_id).await;
    }
}

async fn restore_pty_size(state: &SharedState, session_id: &str) {
    let st = state.read().await;
    if let Some(session) = st.sessions.get(session_id) {
        let _ = session.resize_tx.send((0, 0));
    }
}

/// Send push notifications to all registered tokens
async fn send_push_notifications(tokens: &[PushToken], title: &str, body: &str, session_id: &str) {
    if tokens.is_empty() {
        return;
    }

    // Build Expo push messages
    let messages: Vec<serde_json::Value> = tokens
        .iter()
        .filter(|t| t.token_type == "expo")
        .map(|t| {
            serde_json::json!({
                "to": t.token,
                "title": title,
                "body": body,
                "data": {
                    "sessionId": session_id,
                    "session_id": session_id,
                    "type": "waiting_for_input"
                },
                "sound": "default",
                "priority": "high"
            })
        })
        .collect();

    if messages.is_empty() {
        return;
    }

    // Send to Expo Push API (using shared client with timeout)
    match http_client()
        .post("https://exp.host/--/api/v2/push/send")
        .header("Content-Type", "application/json")
        .json(&messages)
        .send()
        .await
    {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::warn!("Push notification failed: {}", resp.status());
            } else {
                tracing::debug!("Push notification sent to {} devices", messages.len());
            }
        }
        Err(e) => {
            tracing::warn!("Failed to send push notification: {}", e);
        }
    }
}
