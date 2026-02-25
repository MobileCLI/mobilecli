//! Background daemon for MobileCLI
//!
//! Single WebSocket server that all terminal sessions stream to.
//! Mobile connects once and sees all active sessions.

// CLI detection types removed - using pure terminal behavior
// use crate::detection::...;
use crate::filesystem::{config::FileSystemConfig, rate_limit::RateLimiter, FileSystemService};
use crate::terminal::contains_cursor_positioning;
use crate::platform;
use crate::protocol::{
    ChangeType, ClientMessage, FileEncoding, FileSystemError, PtyResizeReason, ServerMessage,
    SessionListItem,
};
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
use tokio_tungstenite::{
    accept_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};

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

    let pid = std::fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());

    if let Some(pid) = pid {
        if is_process_alive(pid) {
            return true;
        }
    }

    // Stale PID/port files cause the app to think the daemon is running when it isn't.
    let _ = std::fs::remove_file(pid_path);
    let _ = std::fs::remove_file(port_file());
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

/// Push notification token
#[derive(Debug, Clone)]
pub struct PushToken {
    pub token: String,
    pub token_type: String, // "expo" | "apns" | "fcm"
    #[allow(dead_code)]
    pub platform: String, // "ios" | "android"
}

/// Default scrollback buffer size (8MB).
///
/// Codex/OpenCode-style frame UIs emit high-volume ANSI redraw traffic. A
/// 64KB tail can roll over quickly and make reopened sessions appear to have
/// "lost" earlier chat. Retaining a larger buffer keeps substantially more
/// recoverable history while still bounding memory.
const DEFAULT_SCROLLBACK_MAX_BYTES: usize = 8 * 1024 * 1024;

/// For on-demand `GetSessionHistory` requests.
const HISTORY_SCROLLBACK_LINES: usize = 200_000;

const ALT_ENTER_SEQS: &[&[u8]] = &[b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
const ALT_LEAVE_SEQS: &[&[u8]] = &[b"\x1b[?1049l", b"\x1b[?1047l", b"\x1b[?47l"];
const ALT_TRACK_TAIL_BYTES: usize = 7;
const SNAPSHOT_CHUNK_BYTES: usize = 48 * 1024;
const CLIENT_CAP_ATTACH_V2: u32 = 1 << 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachProtocolMode {
    V1,
    V2,
}

impl AttachProtocolMode {
    fn from_env() -> Self {
        match std::env::var("MOBILECLI_ATTACH_PROTOCOL")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("v1") => Self::V1,
            Some("v2") => Self::V2,
            _ => Self::V2,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OverhaulFlags {
    pub attach_protocol: AttachProtocolMode,
    pub enable_local_echo: bool,
    pub resize_simplified: bool,
}

impl OverhaulFlags {
    pub fn from_env() -> Self {
        Self {
            attach_protocol: AttachProtocolMode::from_env(),
            enable_local_echo: env_flag("MOBILECLI_ENABLE_LOCAL_ECHO", false),
            resize_simplified: env_flag("MOBILECLI_RESIZE_SIMPLIFIED", true),
        }
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ResizeRequest {
    pub cols: u16,
    pub rows: u16,
    pub epoch: Option<u64>,
    pub reason: PtyResizeReason,
}

/// Active PTY session
pub struct PtySession {
    pub session_id: String,
    pub name: String,
    pub command: String,
    pub project_path: String,
    pub started_at: chrono::DateTime<Utc>,
    pub input_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub resize_tx: mpsc::UnboundedSender<ResizeRequest>,
    /// Scrollback buffer for session history (plain text only)
    /// Uses VecDeque for efficient front truncation when buffer is full
    pub scrollback: VecDeque<u8>,
    /// Maximum scrollback buffer size
    pub scrollback_max_bytes: usize,
    /// Whether the session is currently in alternate screen buffer mode
    pub in_alt_screen: bool,
    /// Tail bytes from prior chunk used to detect alt-screen escape sequences
    /// split across PTY read boundaries.
    pub alt_track_tail: Vec<u8>,
    /// Latest accepted resize epoch from mobile (for stale resize rejection).
    pub last_resize_epoch: u64,
    /// Last dimensions acknowledged by the PTY wrapper.
    pub last_applied_size: Option<(u16, u16)>,
    /// Last non-zero resize dimensions forwarded to the PTY wrapper.
    pub last_requested_size: Option<(u16, u16)>,
    /// Sequence number for live PTY chunks.
    pub live_seq: u64,
    /// Tail for raw mobile input filtering to handle escape sequences split
    /// across websocket messages.
    pub raw_input_tail: Vec<u8>,
}

/// Daemon shared state
pub struct DaemonState {
    pub sessions: HashMap<String, PtySession>,
    pub overhaul_flags: OverhaulFlags,
    pub next_attach_id: u64,
    pub mobile_clients: HashMap<SocketAddr, mpsc::Sender<Message>>,
    pub mobile_client_capabilities: HashMap<SocketAddr, u32>,
    pub mobile_attach_ids: HashMap<SocketAddr, HashMap<String, u64>>,
    /// Mapping from logical mobile sender ID to current socket address.
    /// Used to evict stale/replaced websocket addresses on reconnect.
    pub mobile_sender_addrs: HashMap<String, SocketAddr>,
    pub pty_broadcast: broadcast::Sender<(String, u64, Vec<u8>)>,
    pub port: u16, // The actual port the daemon is running on
    pub push_tokens: Vec<PushToken>,
    pub mobile_views: HashMap<SocketAddr, std::collections::HashSet<String>>,
    pub session_view_counts: HashMap<String, usize>,
    pub file_system: std::sync::Arc<FileSystemService>,
    pub file_watch_subscriptions: HashMap<SocketAddr, std::collections::HashSet<String>>,
    pub file_watch_counts: HashMap<String, usize>,
    pub file_rate_limiters: HashMap<SocketAddr, RateLimiter>,
    /// Device UUID (for multi-device support)
    pub device_id: Option<String>,
    /// Device name (hostname)
    pub device_name: Option<String>,
}

impl DaemonState {
    pub fn new(port: u16) -> Self {
        let (pty_broadcast, _) = broadcast::channel(256);
        let file_system = std::sync::Arc::new(FileSystemService::new(FileSystemConfig::default()));

        // Load device info from config. If missing, create a default config so we always
        // have a stable device_id for pairing.
        let cfg = crate::setup::load_config().unwrap_or_else(|| {
            let cfg = crate::setup::Config::default();
            let _ = crate::setup::save_config(&cfg);
            cfg
        });
        let (device_id, device_name) = (Some(cfg.device_id), Some(cfg.device_name));

        Self {
            sessions: HashMap::new(),
            overhaul_flags: OverhaulFlags::from_env(),
            next_attach_id: 1,
            mobile_clients: HashMap::new(),
            mobile_client_capabilities: HashMap::new(),
            mobile_attach_ids: HashMap::new(),
            mobile_sender_addrs: HashMap::new(),
            pty_broadcast,
            port,
            push_tokens: Vec::new(),
            mobile_views: HashMap::new(),
            session_view_counts: HashMap::new(),
            file_system,
            file_watch_subscriptions: HashMap::new(),
            file_watch_counts: HashMap::new(),
            file_rate_limiters: HashMap::new(),
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
    {
        let st = state.read().await;
        tracing::info!(
            target: "overhaul.phase0",
            attach_protocol = st.overhaul_flags.attach_protocol.as_str(),
            enable_local_echo = st.overhaul_flags.enable_local_echo,
            resize_simplified = st.overhaul_flags.resize_simplified,
            "Loaded terminal overhaul Phase 0 flags"
        );
    }

    // Limit concurrent connections to prevent resource exhaustion
    let conn_limit = Arc::new(tokio::sync::Semaphore::new(64));

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
    run_server_loop_unix(listener, state, conn_limit).await;

    #[cfg(not(unix))]
    run_server_loop_ctrlc_only(listener, state, conn_limit).await;

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(&port_path);
    Ok(())
}

/// Server loop with Unix signal handling (SIGTERM + Ctrl+C)
#[cfg(unix)]
async fn run_server_loop_unix(
    listener: TcpListener,
    state: SharedState,
    conn_limit: Arc<tokio::sync::Semaphore>,
) {
    use tokio::signal::unix::{signal, SignalKind};

    // Try to set up SIGTERM handler, fall back to Ctrl+C only if it fails
    let sigterm_result = signal(SignalKind::terminate());
    if sigterm_result.is_err() {
        tracing::warn!(
            "Failed to set up SIGTERM handler: {:?}. Only Ctrl+C will work for shutdown.",
            sigterm_result.err()
        );
        // Fall back to generic loop with just Ctrl+C
        run_server_loop_ctrlc_only(listener, state, conn_limit).await;
        return;
    }
    let mut sigterm = sigterm_result.unwrap();

    loop {
        tokio::select! {
            result = listener.accept() => {
                if let Ok((stream, addr)) = result {
                    let state = state.clone();
                    let permit = conn_limit.clone().try_acquire_owned();
                    match permit {
                        Ok(permit) => {
                            tokio::spawn(async move {
                                let _permit = permit; // held until connection ends
                                let _ = handle_connection(stream, addr, state).await;
                            });
                        }
                        Err(_) => {
                            tracing::warn!("Connection limit reached, rejecting {}", addr);
                        }
                    }
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
async fn run_server_loop_ctrlc_only(
    listener: TcpListener,
    state: SharedState,
    conn_limit: Arc<tokio::sync::Semaphore>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                if let Ok((stream, addr)) = result {
                    let state = state.clone();
                    let permit = conn_limit.clone().try_acquire_owned();
                    match permit {
                        Ok(permit) => {
                            tokio::spawn(async move {
                                let _permit = permit;
                                let _ = handle_connection(stream, addr, state).await;
                            });
                        }
                        Err(_) => {
                            tracing::warn!("Connection limit reached, rejecting {}", addr);
                        }
                    }
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
    // Uploads are sent base64-encoded over websocket. Keep headroom above the
    // 50MB file cap to avoid protocol-level disconnects.
    let ws_config = WebSocketConfig {
        max_message_size: Some(96 * 1024 * 1024),
        max_frame_size: Some(96 * 1024 * 1024),
        ..Default::default()
    };
    let ws = accept_async_with_config(stream, Some(ws_config)).await?;
    let (tx, mut rx) = ws.split();

    // Wait for first message to determine client type
    let first_msg = rx.next().await;

    match first_msg {
        Some(Ok(Message::Text(text))) => {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                if msg.get("type").and_then(|v| v.as_str()) == Some("register_pty") {
                    // This is a PTY session registering
                    if !addr.ip().is_loopback() {
                        tracing::warn!(
                            "Rejecting PTY registration from non-loopback address: {}",
                            addr
                        );
                        return Ok(());
                    }
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

    let (client_tx, mut client_rx) = mpsc::channel::<Message>(4096);
    let watch_tx = client_tx.clone();
    let (disconnect_tx, mut disconnect_rx) = tokio::sync::watch::channel(false);

    // Register client and get broadcast receiver
    let mut pty_rx = {
        let mut st = state.write().await;
        st.mobile_clients.insert(addr, client_tx);
        st.pty_broadcast.subscribe()
    };

    // Subscribe to file system change events for this client
    let mut file_watch_rx = {
        let st = state.read().await;
        st.file_system.watcher().subscribe()
    };
    let watch_state = state.clone();
    tokio::spawn(async move {
        'watch: loop {
            if *disconnect_rx.borrow() {
                break;
            }
            let change = tokio::select! {
                _ = disconnect_rx.changed() => {
                    if *disconnect_rx.borrow() {
                        break 'watch;
                    }
                    continue 'watch;
                }
                result = file_watch_rx.recv() => {
                    match result {
                        Ok(change) => change,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue 'watch,
                        Err(_) => break 'watch,
                    }
                }
            };

            let fs = {
                let st = watch_state.read().await;
                if !st.mobile_clients.contains_key(&addr) {
                    break 'watch;
                }
                let watched = match st.file_watch_subscriptions.get(&addr) {
                    Some(paths) => paths,
                    None => continue,
                };
                if !is_path_watched(&change.path, watched) {
                    continue;
                }
                st.file_system.clone()
            };

            let mut new_entry = None;
            if matches!(
                change.change_type,
                ChangeType::Created | ChangeType::Modified
            ) {
                let should_break = tokio::select! {
                    _ = disconnect_rx.changed() => *disconnect_rx.borrow(),
                    result = fs.ops().get_file_info(&change.path) => {
                        if let Ok(entry) = result {
                            new_entry = Some(entry);
                        }
                        false
                    }
                };
                if should_break {
                    break;
                }
            }
            if *disconnect_rx.borrow() {
                break;
            }

            let msg = ServerMessage::FileChanged {
                path: change.path.clone(),
                change_type: change.change_type.clone(),
                new_entry,
            };
            if let Ok(text) = serde_json::to_string(&msg) {
                if watch_tx.try_send(Message::Text(text)).is_err() {
                    break;
                }
            }
        }
    });

    // Send welcome with device info
    let (device_id, device_name) = {
        let st = state.read().await;
        (st.device_id.clone(), st.device_name.clone())
    };
    let welcome = ServerMessage::Welcome {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
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
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(msg) => process_client_msg(msg, &state, &mut tx, addr).await?,
            Err(e) => {
                tracing::debug!("Ignoring unparsable client message from {}: {}", addr, e);
            }
        }
    }

    loop {
        tokio::select! {
            // PTY output
            result = pty_rx.recv() => {
                match result {
                    Ok((session_id, seq, data)) => {
                        let (flags, capabilities, attach_id) = {
                            let st = state.read().await;
                            let caps = st.mobile_client_capabilities.get(&addr).copied().unwrap_or(0);
                            let attach_id = st
                                .mobile_attach_ids
                                .get(&addr)
                                .and_then(|sessions| sessions.get(&session_id).copied());
                            (st.overhaul_flags, caps, attach_id)
                        };

                        let msg = if should_use_attach_v2(flags, capabilities) {
                            let Some(attach_id) = attach_id else {
                                continue;
                            };
                            ServerMessage::PtyChunk {
                                session_id,
                                attach_id,
                                seq,
                                data: BASE64.encode(&data),
                                timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                            }
                        } else {
                            ServerMessage::PtyBytes {
                                session_id,
                                data: BASE64.encode(&data),
                            }
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
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(msg) => process_client_msg(msg, &state, &mut tx, addr).await?,
                            Err(e) => {
                                tracing::debug!("Ignoring unparsable client message from {}: {}", addr, e);
                            }
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
    let _ = disconnect_tx.send(true);
    {
        let mut st = state.write().await;
        st.mobile_clients.remove(&addr);
        st.file_rate_limiters.remove(&addr);
    }
    cleanup_client_state(&state, addr).await;
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
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<ResizeRequest>();

    // Register session
    let pty_broadcast = {
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
                scrollback: VecDeque::new(),
                scrollback_max_bytes: DEFAULT_SCROLLBACK_MAX_BYTES,
                in_alt_screen: false,
                alt_track_tail: Vec::new(),
                last_resize_epoch: 0,
                last_applied_size: None,
                last_requested_size: None,
                live_seq: 0,
                raw_input_tail: Vec::new(),
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
                                        // Accumulate scrollback and track render state.
                                        let mut live_seq: Option<u64> = None;
                                        {
                                            let mut st = state.write().await;
                                            if let Some(session) = st.sessions.get_mut(&session_id) {
                                                session.live_seq = session.live_seq.saturating_add(1);
                                                live_seq = Some(session.live_seq);
                                                session.scrollback.extend(bytes.iter().copied());
                                                // Truncate from front if over limit (VecDeque is O(1) per pop)
                                                while session.scrollback.len() > session.scrollback_max_bytes {
                                                    session.scrollback.pop_front();
                                                }
                                                // Track alt-screen transitions across chunk
                                                // boundaries so subscribe_ack reflects the
                                                // current rendering mode reliably.
                                                update_alt_screen_state(
                                                    &mut session.in_alt_screen,
                                                    &mut session.alt_track_tail,
                                                    &bytes,
                                                );
                                            }
                                        }
                                        if let Some(seq) = live_seq {
                                            tracing::trace!(
                                                target: "overhaul.sequence",
                                                session_id = %session_id,
                                                seq,
                                                chunk_bytes = bytes.len(),
                                                "Forwarding live PTY chunk"
                                            );
                                        }
                                        let seq = live_seq.unwrap_or(0);
                                        let _ =
                                            pty_broadcast.send((session_id.clone(), seq, bytes.clone()));

                                        // Note: Notification detection removed for simplicity.
                                        // Can be re-added later in a separate notification module.
                                    }
                                }
                            } else if msg["type"].as_str() == Some("pty_resized") {
                                let cols = msg["cols"].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
                                let rows = msg["rows"].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
                                let epoch = msg["epoch"].as_u64();
                                {
                                    let mut st = state.write().await;
                                    if let Some(session) = st.sessions.get_mut(&session_id) {
                                        session.last_applied_size = Some((cols, rows));
                                    }
                                }
                                broadcast_pty_resized(&state, &session_id, cols, rows, epoch).await;
                            } else if msg["type"].as_str() == Some("session_ended") {
                                exit_code = msg["exit_code"].as_i64().unwrap_or(0) as i32;
                                tracing::info!("PTY session {} ended (exit_code={})", session_id, exit_code);
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let seq = {
                            let mut st = state.write().await;
                            if let Some(session) = st.sessions.get_mut(&session_id) {
                                session.live_seq = session.live_seq.saturating_add(1);
                                session.live_seq
                            } else {
                                0
                            }
                        };
                        let _ = pty_broadcast.send((session_id.clone(), seq, data));
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }

            // Input from mobile to send to PTY
            result = input_rx.recv() => {
                match result {
                    Some(input) => {
                        let msg = serde_json::json!({
                            "type": "input",
                            "data": BASE64.encode(&input),
                        });
                        if tx.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }

                        // Clear output buffer on input
                        output_buffer.clear();
                    }
                    // Channel closed — session was removed (e.g. CloseSession)
                    None => break,
                }
            }

            // Resize from mobile
            result = resize_rx.recv() => {
                match result {
                    Some(mut req) => {
                        let mut coalesced = 0usize;
                        while let Ok(next) = resize_rx.try_recv() {
                            req = next;
                            coalesced += 1;
                        }
                        if coalesced > 0 {
                            tracing::debug!(
                                target: "overhaul.resize",
                                session_id = %session_id,
                                coalesced,
                                cols = req.cols,
                                rows = req.rows,
                                epoch = ?req.epoch,
                                reason = req.reason.as_str(),
                                "Coalesced resize burst to latest request"
                            );
                        }
                        let mut msg = serde_json::json!({
                            "type": "resize",
                            "cols": req.cols,
                            "rows": req.rows,
                            "reason": req.reason.as_str(),
                        });
                        if let Some(epoch) = req.epoch {
                            msg["epoch"] = serde_json::json!(epoch);
                        }
                        if tx.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                    // Channel closed — session was removed (e.g. CloseSession)
                    None => break,
                }
            }
        }
    }

    // Unregister session (skip if already removed by CloseSession)
    let was_present = {
        let mut st = state.write().await;
        if st.sessions.remove(&session_id).is_some() {
            clear_mobile_attach_for_session(&mut st, &session_id);
            // Notify about session end
            let msg = ServerMessage::SessionEnded {
                session_id: session_id.clone(),
                exit_code,
            };
            let msg_str = serde_json::to_string(&msg)?;
            for client in st.mobile_clients.values() {
                let _ = client.try_send(Message::Text(msg_str.clone()));
            }
            true
        } else {
            false
        }
    };

    if was_present {
        // Broadcast updated sessions list to all clients
        broadcast_sessions_update(&state).await;

        // Update persisted sessions
        persist_sessions_to_file(&state).await;
    }

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

/// POSIX-safe single-quote wrapper for shell tokens.
fn shell_quote_posix(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn shell_base_name(shell: &str) -> String {
    std::path::Path::new(shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(shell)
        .to_lowercase()
}

fn shell_args_for_command(shell: &str, command: &str) -> Vec<String> {
    let base = shell_base_name(shell);
    let supports_login = matches!(
        base.as_str(),
        "bash" | "zsh" | "fish" | "ksh" | "tcsh" | "csh"
    );
    let mut args = Vec::with_capacity(4);
    if supports_login {
        // Interactive + login ensures user PATH customizations (e.g. nvm) are loaded.
        args.push("-l".to_string());
        args.push("-i".to_string());
    }
    args.push("-c".to_string());
    args.push(command.to_string());
    args
}

/// Escape a string for embedding inside AppleScript double-quoted literals.
/// Handles backslashes, double quotes, and strips newlines to prevent injection.
fn escape_for_applescript(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
}

fn shell_command_line(shell: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(shell_quote_posix(shell));
    for arg in args {
        parts.push(shell_quote_posix(arg));
    }
    parts.join(" ")
}

fn resolve_mobilecli_bin() -> String {
    // When the daemon survives a binary replacement, /proc/self/exe can include
    // a trailing " (deleted)" suffix. Strip it and verify the path still exists.
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy().to_string();
        if std::path::Path::new(&exe_str).exists() {
            return exe_str;
        }

        if let Some(stripped) = exe_str.strip_suffix(" (deleted)") {
            if std::path::Path::new(stripped).exists() {
                return stripped.to_string();
            }
        }
    }

    if let Ok(found) = which::which("mobilecli") {
        let found_str = found.to_string_lossy().to_string();
        if std::path::Path::new(&found_str).exists() {
            return found_str;
        }
    }

    if let Some(home) = platform::home_dir() {
        let local_bin = home.join(".local/bin/mobilecli");
        if local_bin.exists() {
            return local_bin.to_string_lossy().to_string();
        }
    }

    if cfg!(windows) {
        "mobilecli.exe".to_string()
    } else {
        "mobilecli".to_string()
    }
}

/// Build the shell command to run inside a terminal emulator.
fn build_wrap_shell_command(
    mobilecli_bin: &str,
    session_name: &str,
    command: &str,
    args: &[String],
    working_dir: Option<&str>,
) -> String {
    let mut tokens = Vec::new();
    tokens.push(mobilecli_bin.to_string());
    tokens.push("--name".to_string());
    tokens.push(session_name.to_string());
    if let Some(dir) = working_dir {
        tokens.push("--dir".to_string());
        tokens.push(dir.to_string());
    }
    tokens.push(command.to_string());
    tokens.extend(args.iter().cloned());
    tokens
        .into_iter()
        .map(|t| shell_quote_posix(&t))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn spawn_session_windows(
    command: &str,
    args: &[String],
    name: Option<&str>,
    working_dir: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::os::windows::process::CommandExt;

    let session_name = name.unwrap_or(command);
    let mobilecli_bin = resolve_mobilecli_bin();

    let mut cmd = std::process::Command::new(&mobilecli_bin);
    cmd.arg("--name").arg(session_name);
    if let Some(dir) = working_dir {
        cmd.arg("--dir").arg(dir);
        cmd.current_dir(dir);
    }
    cmd.arg(command).args(args);

    // Create a new console window so the session is visible and interactive.
    const CREATE_NEW_CONSOLE: u32 = 0x00000010;
    cmd.creation_flags(CREATE_NEW_CONSOLE);

    cmd.spawn()?;
    Ok(())
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

    #[cfg(windows)]
    {
        return spawn_session_windows(command, args, name, working_dir);
    }

    // Try to detect terminal emulator (headless servers won't have one)
    let terminal = detect_terminal_emulator().ok();

    // Build the command to run inside the terminal
    let session_name = name.unwrap_or(command);
    let mobilecli_bin = resolve_mobilecli_bin();
    let shell_candidate = platform::default_shell();
    let shell = if std::path::Path::new(&shell_candidate).exists()
        || which::which(&shell_candidate).is_ok()
    {
        shell_candidate
    } else {
        "/bin/sh".to_string()
    };
    // Default to home directory when no working_dir is specified.
    // Without this, the spawned terminal inherits the daemon's CWD,
    // which is wherever the daemon was launched from (often the project dir).
    let home_dir_buf = dirs_next::home_dir();
    let effective_working_dir: Option<&str> =
        working_dir.or_else(|| home_dir_buf.as_ref().and_then(|p| p.to_str()));

    let wrap_cmd = build_wrap_shell_command(
        &mobilecli_bin,
        session_name,
        command,
        args,
        effective_working_dir,
    );
    let shell_args = shell_args_for_command(&shell, &wrap_cmd);

    let mut cmd = if let Some(ref terminal) = terminal {
        tracing::info!("Spawning session: {} via {}", wrap_cmd, terminal.name);

        // Build terminal command based on detected emulator
        let mut c = std::process::Command::new(&terminal.binary);

        match terminal.name.as_str() {
            "kitty" => {
                c.arg("--").arg(&shell).args(&shell_args);
            }
            "alacritty" => {
                c.arg("-e").arg(&shell).args(&shell_args);
            }
            "gnome-terminal" | "tilix" => {
                c.arg("--").arg(&shell).args(&shell_args);
            }
            "konsole" => {
                c.arg("-e").arg(&shell).args(&shell_args);
            }
            "xterm" | "urxvt" => {
                c.arg("-e").arg(&shell).args(&shell_args);
            }
            "wezterm" => {
                c.args(["start", "--"]).arg(&shell).args(&shell_args);
            }
            "iterm" => {
                // macOS iTerm2 - use osascript to open a new window
                let shell_cmd = escape_for_applescript(&shell_command_line(&shell, &shell_args));
                let script = format!(
                    r#"tell application "iTerm"
                        create window with default profile
                        tell current session of current window
                            write text "{shell_cmd}"
                        end tell
                    end tell"#,
                );
                c = std::process::Command::new("osascript");
                c.args(["-e", &script]);
            }
            "terminal" => {
                // macOS Terminal.app
                let shell_cmd = escape_for_applescript(&shell_command_line(&shell, &shell_args));
                let script = format!(
                    r#"tell application "Terminal"
                        do script "{shell_cmd}"
                        activate
                    end tell"#,
                );
                c = std::process::Command::new("osascript");
                c.args(["-e", &script]);
            }
            _ => {
                // Generic fallback: try -e flag
                c.arg("-e").arg(&shell).args(&shell_args);
            }
        }
        c
    } else {
        // Headless mode: no terminal emulator available.
        // Use tmux for session management if available, otherwise spawn directly.
        // The mobilecli pty-wrap command creates its own PTY, so a terminal
        // window is not required — the mobile app views output via WebSocket.
        if which::which("tmux").is_ok() {
            let tmux_name = format!(
                "mcli-{}",
                session_name.replace(|ch: char| !ch.is_alphanumeric() && ch != '-', "-")
            );
            let shell_cmd = shell_command_line(&shell, &shell_args);
            tracing::info!("Spawning session headless (tmux): {}", wrap_cmd);
            let mut c = std::process::Command::new("tmux");
            c.args(["new-session", "-d", "-s", &tmux_name, &shell_cmd]);
            c
        } else {
            // Direct spawn: mobilecli pty-wrap creates its own PTY
            tracing::info!("Spawning session headless (direct): {}", wrap_cmd);
            let mut c = std::process::Command::new(&shell);
            c.args(&shell_args);
            c
        }
    };

    // Set working directory (defaults to home dir when not specified by caller)
    if let Some(dir) = effective_working_dir {
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
        ClientMessage::Hello {
            client_version,
            sender_id,
            client_capabilities,
        } => {
            // Already sent Welcome on connect, but log the client version
            tracing::debug!("Client hello, version: {}", client_version);
            if let Some(sender_id) = sender_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
            {
                let replaced_addr = {
                    let mut st = state.write().await;
                    match st.mobile_sender_addrs.insert(sender_id.clone(), addr) {
                        Some(previous) if previous != addr => Some(previous),
                        _ => None,
                    }
                };
                if let Some(previous_addr) = replaced_addr {
                    tracing::info!(
                        sender_id = %sender_id,
                        previous_addr = %previous_addr,
                        new_addr = %addr,
                        "Replacing stale mobile socket for sender_id"
                    );
                    evict_mobile_addr(state, previous_addr).await;
                }
            }
            if let Some(capabilities) = client_capabilities {
                let mut st = state.write().await;
                st.mobile_client_capabilities.insert(addr, capabilities);
                tracing::debug!(
                    addr = %addr,
                    capabilities,
                    supports_attach_v2 = client_supports_attach_v2(capabilities),
                    "Registered mobile client capabilities"
                );
            }
        }
        ClientMessage::Subscribe {
            session_id,
            last_seen_seq,
            client_capabilities,
        } => {
            // Phase 3: Simplified unified subscribe flow (always v2 protocol)
            tracing::debug!("Client subscribed to session: {}", session_id);
            let attach_started = std::time::Instant::now();
            
            let mut st = state.write().await;
            if let Some(capabilities) = client_capabilities {
                st.mobile_client_capabilities.insert(addr, capabilities);
            }
            let attach_id = st.next_attach_id;
            st.next_attach_id = st.next_attach_id.saturating_add(1);
            
            // Collect session state under one lock
            let (
                scrollback_bytes,
                mobile_in_alt_screen,
                ready_cols,
                ready_rows,
            ) = if let Some(session) = st.sessions.get(&session_id) {
                let mobile_in_alt_screen = session.in_alt_screen;
                
                // Only replay scrollback if it's plain text (no cursor positioning)
                let sb = if !mobile_in_alt_screen && !session.scrollback.is_empty() {
                    let bytes: Vec<u8> = session.scrollback.iter().copied().collect();
                    if !contains_cursor_positioning(&bytes) {
                        Some(bytes)
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                let (ready_cols, ready_rows) = session.last_applied_size.unwrap_or((0, 0));
                (sb, mobile_in_alt_screen, ready_cols, ready_rows)
            } else {
                (None::<Vec<u8>>, false, 0, 0)
            };
            drop(st);

            tracing::info!(
                target: "overhaul.attach",
                session_id = %session_id,
                addr = %addr,
                attach_id,
                last_seen_seq = ?last_seen_seq,
                in_alt_screen = mobile_in_alt_screen,
                scrollback_bytes = scrollback_bytes.as_ref().map(|b| b.len()).unwrap_or(0),
                "Attach sequence started"
            );

            // 1. Send AttachBegin
            let begin = ServerMessage::AttachBegin {
                session_id: session_id.clone(),
                attach_id,
                mode: if last_seen_seq.is_some() {
                    "reconnect".to_string()
                } else {
                    "fresh".to_string()
                },
                in_alt_screen: mobile_in_alt_screen,
            };
            if let Ok(text) = serde_json::to_string(&begin) {
                let _ = tx.send(Message::Text(text)).await;
            }

            // 2. Send AttachClear (always clear terminal first)
            let clear = ServerMessage::AttachClear {
                session_id: session_id.clone(),
                attach_id,
            };
            if let Ok(text) = serde_json::to_string(&clear) {
                let _ = tx.send(Message::Text(text)).await;
            }

            // 3. Send scrollback in chunks (if safe to replay)
            let replay_bytes = scrollback_bytes.as_ref().map(|b| b.len()).unwrap_or(0);
            if let Some(bytes) = scrollback_bytes {
                let chunks = chunk_snapshot_payload(&bytes);
                let total_chunks = chunks.len().max(1) as u32;
                if chunks.is_empty() {
                    let msg = ServerMessage::AttachSnapshotChunk {
                        session_id: session_id.clone(),
                        attach_id,
                        chunk_seq: 1,
                        total_chunks,
                        is_last: true,
                        data: String::new(),
                    };
                    if let Ok(text) = serde_json::to_string(&msg) {
                        let _ = tx.send(Message::Text(text)).await;
                    }
                } else {
                    for (idx, chunk) in chunks.into_iter().enumerate() {
                        let msg = ServerMessage::AttachSnapshotChunk {
                            session_id: session_id.clone(),
                            attach_id,
                            chunk_seq: (idx + 1) as u32,
                            total_chunks,
                            is_last: idx + 1 == total_chunks as usize,
                            data: chunk,
                        };
                        if let Ok(text) = serde_json::to_string(&msg) {
                            let _ = tx.send(Message::Text(text)).await;
                        }
                    }
                }
                tracing::info!(
                    target: "overhaul.attach",
                    session_id = %session_id,
                    attach_id,
                    replay_bytes,
                    "Scrollback replay sent"
                );
            }

            // 4. Get current live sequence and send AttachReady
            let ready_live_seq = {
                let st = state.read().await;
                st.sessions
                    .get(&session_id)
                    .map(|s| s.live_seq)
                    .unwrap_or(0)
            };
            
            let ready = ServerMessage::AttachReady {
                session_id: session_id.clone(),
                attach_id,
                last_live_seq: ready_live_seq,
                cols: ready_cols,
                rows: ready_rows,
            };
            if let Ok(text) = serde_json::to_string(&ready) {
                let _ = tx.send(Message::Text(text)).await;
            }

            // 5. Register viewer for session view count (needed for priority switching)
            let mut st = state.write().await;
            let entry = st.mobile_views.entry(addr).or_default();
            if entry.insert(session_id.clone()) {
                let count = st
                    .session_view_counts
                    .entry(session_id.clone())
                    .or_insert(0);
                *count += 1;
            }
            st.mobile_attach_ids
                .entry(addr)
                .or_default()
                .insert(session_id.clone(), attach_id);

            tracing::info!(
                target: "overhaul.attach",
                session_id = %session_id,
                addr = %addr,
                attach_id,
                replay_bytes,
                duration_ms = attach_started.elapsed().as_millis() as u64,
                "Attach sequence completed"
            );
        }
        ClientMessage::Unsubscribe { session_id } => {
            tracing::debug!("Client unsubscribed from session: {}", session_id);
            let mut st = state.write().await;
            let mut remove_attach_addr = false;
            if let Some(attach_by_session) = st.mobile_attach_ids.get_mut(&addr) {
                attach_by_session.remove(&session_id);
                remove_attach_addr = attach_by_session.is_empty();
            }
            if remove_attach_addr {
                st.mobile_attach_ids.remove(&addr);
            }
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
            session_id,
            text,
            raw,
            ..
        } => {
            let mut st = state.write().await;
            if let Some(session) = st.sessions.get_mut(&session_id) {
                let mut payload = text.into_bytes();
                if raw {
                    let (filtered, dropped) = strip_terminal_report_sequences_stateful(
                        &mut session.raw_input_tail,
                        &payload,
                    );
                    if dropped > 0 {
                        tracing::debug!(
                            session_id = %session_id,
                            dropped_sequences = dropped,
                            original_len = payload.len(),
                            filtered_len = filtered.len(),
                            "Dropped terminal-report reply sequences from mobile raw input"
                        );
                    }
                    payload = filtered;
                }

                if !payload.is_empty() {
                    let _ = session.input_tx.send(payload);
                }
            }
        }
        ClientMessage::PtyResize {
            session_id,
            cols,
            rows,
            epoch,
            reason,
        } => {
            // Phase 4: Mobile/Desktop Priority Switching
            // - Mobile has priority: mobile resize always applies
            // - Desktop is passive: desktop resize only applies if no mobile viewers
            // - DetachRestore: restore desktop size when mobile disconnects
            
            let reason = reason.unwrap_or(PtyResizeReason::GeometryChange);
            let is_restore = cols == 0 && rows == 0;
            
            let mut st = state.write().await;
            let viewer_count = st
                .session_view_counts
                .get(&session_id)
                .copied()
                .unwrap_or(0);
            let sender_is_mobile = st
                .mobile_views
                .get(&addr)
                .map(|views| views.contains(&session_id))
                .unwrap_or(false);
            
            // Determine if we should apply this resize
            let should_apply = if is_restore {
                // Restore desktop size when mobile disconnects
                true
            } else if sender_is_mobile {
                // Mobile has priority - always apply mobile resize
                true
            } else {
                // Desktop resize - only apply if no mobile viewers
                viewer_count == 0
            };
            
            if !should_apply {
                tracing::debug!(
                    session_id = %session_id,
                    cols,
                    rows,
                    reason = reason.as_str(),
                    viewer_count,
                    sender_is_mobile,
                    decision = "ignored_desktop_while_mobile_active",
                    "Ignoring desktop resize while mobile viewers active"
                );
                return Ok(());
            }
            
            if let Some(session) = st.sessions.get_mut(&session_id) {
                // Check for stale epoch
                if let Some(epoch_val) = epoch {
                    if epoch_val <= session.last_resize_epoch {
                        tracing::debug!(
                            session_id = %session_id,
                            cols,
                            rows,
                            epoch = epoch_val,
                            last_epoch = session.last_resize_epoch,
                            decision = "ignored_stale_epoch",
                            "Ignoring stale PTY resize"
                        );
                        return Ok(());
                    }
                    session.last_resize_epoch = epoch_val;
                }
                
                // Skip if no-op
                if session.last_applied_size == Some((cols, rows)) {
                    tracing::debug!(
                        session_id = %session_id,
                        cols,
                        rows,
                        decision = "ignored_noop",
                        "Ignoring no-op PTY resize"
                    );
                    return Ok(());
                }
                
                tracing::debug!(
                    session_id = %session_id,
                    cols,
                    rows,
                    epoch = ?epoch,
                    reason = reason.as_str(),
                    viewer_count,
                    sender_is_mobile,
                    is_restore,
                    decision = "forwarded",
                    "Forwarding PTY resize to wrapper"
                );
                
                let _ = session.resize_tx.send(ResizeRequest {
                    cols,
                    rows,
                    epoch,
                    reason,
                });
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
        ClientMessage::CloseSession { session_id } => {
            // Drop the session's channels so the PTY task exits naturally,
            // then remove it from state and notify all clients.
            let closed = {
                let mut st = state.write().await;
                if let Some(session) = st.sessions.remove(&session_id) {
                    // Dropping input_tx/resize_tx causes the PTY read loop to break
                    drop(session);
                    // Clean up view counts for this session
                    st.session_view_counts.remove(&session_id);
                    clear_mobile_attach_for_session(&mut st, &session_id);
                    for views in st.mobile_views.values_mut() {
                        views.remove(&session_id);
                    }
                    true
                } else {
                    false
                }
            };

            if closed {
                // Confirm to requesting client
                let msg = ServerMessage::SessionClosed {
                    session_id: session_id.clone(),
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;

                // Broadcast session ended to all clients
                {
                    let st = state.read().await;
                    let end_msg = ServerMessage::SessionEnded {
                        session_id: session_id.clone(),
                        exit_code: -1,
                    };
                    let end_str = serde_json::to_string(&end_msg)?;
                    for client in st.mobile_clients.values() {
                        let _ = client.try_send(Message::Text(end_str.clone()));
                    }
                }

                // Broadcast updated sessions list and persist
                broadcast_sessions_update(state).await;
                persist_sessions_to_file(state).await;

                tracing::info!("Session {} closed by mobile client", session_id);
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
        ClientMessage::UnregisterPushToken { token } => {
            let mut st = state.write().await;
            let before = st.push_tokens.len();
            st.push_tokens.retain(|t| t.token != token);
            let after = st.push_tokens.len();
            tracing::info!(
                "Unregistered push token (removed {})",
                before.saturating_sub(after)
            );
        }
        ClientMessage::ToolApproval {
            session_id,
            response: _,
        } => {
            // DEPRECATED: CLI detection and tool approval removed
            tracing::warn!(
                "Tool approval ignored (feature removed) for session {}",
                session_id
            );
        }
        ClientMessage::GetSessionHistory {
            session_id,
            max_bytes,
        } => {
            let (data, total_bytes) = {
                let st = state.read().await;
                if let Some(session) = st.sessions.get(&session_id) {
                    let max = max_bytes.unwrap_or(session.scrollback_max_bytes);
                    let (bytes, total) = tail_scrollback_bytes(session, max);
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
        ClientMessage::ListDirectory {
            request_id,
            path,
            include_hidden,
            sort_by,
            sort_order,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "list_directory",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs
                .ops()
                .list_directory(&path, include_hidden, sort_by, sort_order)
                .await
            {
                Ok((path, entries, total_count, truncated)) => {
                    let msg = ServerMessage::DirectoryListing {
                        request_id,
                        path,
                        entries,
                        total_count,
                        truncated,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "list_directory", &path, e).await?;
                }
            }
        }
        ClientMessage::ReadFile {
            request_id,
            path,
            offset,
            length,
            encoding,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "read_file",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.ops().read_file(&path, offset, length, encoding).await {
                Ok(file) => {
                    let msg = ServerMessage::FileContent {
                        request_id,
                        path: file.path,
                        content: file.content,
                        encoding: file.encoding,
                        mime_type: file.mime_type,
                        size: file.size,
                        modified: file.modified,
                        truncated_at: file.truncated_at,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "read_file", &path, e).await?;
                }
            }
        }
        ClientMessage::ReadFileChunk {
            request_id,
            path,
            chunk_index,
            chunk_size,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "read_file_chunk",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            let size = chunk_size.unwrap_or(262_144);
            match fs.ops().read_file_chunk(&path, chunk_index, size).await {
                Ok((path, total_chunks, total_size, chunk_index, data, checksum, is_last)) => {
                    let msg = ServerMessage::FileChunk {
                        request_id,
                        path,
                        chunk_index,
                        total_chunks,
                        total_size,
                        data,
                        checksum,
                        is_last,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "read_file_chunk", &path, e).await?;
                }
            }
        }
        ClientMessage::WriteFile {
            request_id,
            path,
            content,
            encoding,
            create_parents,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "write_file",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs
                .ops()
                .write_file(&path, &content, encoding, create_parents)
                .await
            {
                Ok(()) => {
                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "write_file".to_string(),
                        path: path.clone(),
                        message: None,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "write_file", &path, e).await?;
                }
            }
        }
        ClientMessage::UploadFile {
            request_id,
            session_id,
            file_name,
            content_base64,
            mime_type,
        } => {
            tracing::debug!(
                "UploadFile request: session_id={}, file_name={}, bytes_base64={}",
                session_id,
                file_name,
                content_base64.len()
            );
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "upload_file",
                    &session_id,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }

            let (project_path, fs) = {
                let st = state.read().await;
                let Some(session) = st.sessions.get(&session_id) else {
                    send_fs_error(
                        tx,
                        request_id,
                        "upload_file",
                        &session_id,
                        FileSystemError::NotFound {
                            path: format!("session:{}", session_id),
                        },
                    )
                    .await?;
                    return Ok(());
                };
                (session.project_path.clone(), st.file_system.clone())
            };

            let destination =
                build_upload_destination_path(&project_path, sanitize_upload_file_name(&file_name));
            let destination_path = crate::filesystem::path_utils::to_protocol_path(&destination);

            match fs
                .ops()
                .write_file(
                    &destination_path,
                    &content_base64,
                    FileEncoding::Base64,
                    true,
                )
                .await
            {
                Ok(()) => {
                    tracing::debug!("UploadFile success: {}", destination_path);
                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "upload_file".to_string(),
                        path: destination_path,
                        message: mime_type,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    tracing::debug!("UploadFile error for {}: {:?}", project_path, e);
                    send_fs_error(tx, request_id, "upload_file", &project_path, e).await?;
                }
            }
        }
        ClientMessage::CreateDirectory {
            request_id,
            path,
            recursive,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "create_directory",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.ops().create_directory(&path, recursive).await {
                Ok(()) => {
                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "create_directory".to_string(),
                        path: path.clone(),
                        message: None,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "create_directory", &path, e).await?;
                }
            }
        }
        ClientMessage::DeletePath {
            request_id,
            path,
            recursive,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "delete_path",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.ops().delete_path(&path, recursive).await {
                Ok(()) => {
                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "delete_path".to_string(),
                        path: path.clone(),
                        message: None,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "delete_path", &path, e).await?;
                }
            }
        }
        ClientMessage::RenamePath {
            request_id,
            old_path,
            new_path,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "rename_path",
                    &old_path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.ops().rename_path(&old_path, &new_path).await {
                Ok(()) => {
                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "rename_path".to_string(),
                        path: old_path.clone(),
                        message: Some(new_path),
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "rename_path", &old_path, e).await?;
                }
            }
        }
        ClientMessage::CopyPath {
            request_id,
            source,
            destination,
            recursive,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "copy_path",
                    &source,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.ops().copy_path(&source, &destination, recursive).await {
                Ok(()) => {
                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "copy_path".to_string(),
                        path: source.clone(),
                        message: Some(destination),
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "copy_path", &source, e).await?;
                }
            }
        }
        ClientMessage::GetFileInfo { request_id, path } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "get_file_info",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.ops().get_file_info(&path).await {
                Ok(entry) => {
                    let msg = ServerMessage::FileInfo {
                        request_id,
                        path,
                        entry,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "get_file_info", &path, e).await?;
                }
            }
        }
        ClientMessage::SearchFiles {
            request_id,
            path,
            pattern,
            content_pattern,
            max_depth,
            max_results,
        } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "search_files",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            let max_results = max_results.unwrap_or(fs.config().max_search_results);
            match fs
                .search()
                .search_files(
                    &path,
                    &pattern,
                    content_pattern.as_deref(),
                    max_depth,
                    max_results,
                )
                .await
            {
                Ok((root, matches, truncated)) => {
                    let msg = ServerMessage::SearchResults {
                        request_id,
                        query: pattern,
                        path: root,
                        matches,
                        truncated,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "search_files", &path, e).await?;
                }
            }
        }
        ClientMessage::WatchDirectory { request_id, path } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "watch_directory",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            match fs.validator().validate_existing(&path) {
                Ok(canonical) => {
                    if !canonical.is_dir() {
                        send_fs_error(
                            tx,
                            request_id,
                            "watch_directory",
                            &path,
                            FileSystemError::NotADirectory {
                                path: crate::filesystem::path_utils::to_protocol_path(&canonical),
                            },
                        )
                        .await?;
                        return Ok(());
                    }

                    // Use protocol-normalized paths for watcher keys and subscriptions so the
                    // mobile app can match file_changed events reliably across OSes.
                    let watch_path = crate::filesystem::path_utils::to_protocol_path(&canonical);
                    let should_watch = {
                        let mut st = state.write().await;
                        let entry = st.file_watch_subscriptions.entry(addr).or_default();
                        if entry.insert(watch_path.clone()) {
                            let count = st.file_watch_counts.entry(watch_path.clone()).or_insert(0);
                            *count += 1;
                            *count == 1
                        } else {
                            false
                        }
                    };

                    if should_watch {
                        if let Err(e) = fs.watcher().watch(&watch_path) {
                            send_fs_error(tx, request_id, "watch_directory", &path, e).await?;
                            return Ok(());
                        }
                    }

                    let msg = ServerMessage::OperationSuccess {
                        request_id,
                        operation: "watch_directory".to_string(),
                        path: watch_path,
                        message: None,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                }
                Err(e) => {
                    send_fs_error(tx, request_id, "watch_directory", &path, e).await?;
                }
            }
        }
        ClientMessage::UnwatchDirectory { request_id, path } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "unwatch_directory",
                    &path,
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            let watch_path = match fs.validator().validate_existing(&path) {
                Ok(canonical) => crate::filesystem::path_utils::to_protocol_path(&canonical),
                Err(_) => path.clone(),
            };

            let should_unwatch = {
                let mut st = state.write().await;
                let entry = st.file_watch_subscriptions.entry(addr).or_default();
                let removed = entry.remove(&watch_path);
                if removed {
                    if let Some(count) = st.file_watch_counts.get_mut(&watch_path) {
                        if *count > 0 {
                            *count -= 1;
                        }
                        if *count == 0 {
                            st.file_watch_counts.remove(&watch_path);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if should_unwatch {
                let _ = fs.watcher().unwatch(&watch_path);
            }

            let msg = ServerMessage::OperationSuccess {
                request_id,
                operation: "unwatch_directory".to_string(),
                path: watch_path,
                message: None,
            };
            tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
        }
        ClientMessage::GetHomeDirectory { request_id } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "get_home_directory",
                    "",
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            let home = dirs_next::home_dir().or_else(|| fs.config().allowed_roots.first().cloned());
            if let Some(path) = home {
                let msg = ServerMessage::HomeDirectory {
                    request_id,
                    path: crate::filesystem::path_utils::to_protocol_path(&path),
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
            } else {
                send_fs_error(
                    tx,
                    request_id,
                    "get_home_directory",
                    "",
                    FileSystemError::NotFound {
                        path: "home".to_string(),
                    },
                )
                .await?;
            }
        }
        ClientMessage::GetAllowedRoots { request_id } => {
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                send_fs_error(
                    tx,
                    request_id,
                    "get_allowed_roots",
                    "",
                    FileSystemError::RateLimited { retry_after_ms },
                )
                .await?;
                return Ok(());
            }
            let fs = { state.read().await.file_system.clone() };
            let roots = fs
                .config()
                .allowed_roots
                .iter()
                .map(|p| crate::filesystem::path_utils::to_protocol_path(p))
                .collect();
            let msg = ServerMessage::AllowedRoots { request_id, roots };
            tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
        }
    }
    Ok(())
}

async fn send_fs_error(
    tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
    request_id: String,
    operation: &str,
    path: &str,
    error: FileSystemError,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let msg = ServerMessage::OperationError {
        request_id,
        operation: operation.to_string(),
        path: path.to_string(),
        error,
    };
    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
    Ok(())
}

// Uploads are stored as:
//   <project>/.mobilecli/uploads/<timestamp>-<short_uuid>-<sanitized_name>
// and then written atomically via FileOperations::write_file, which appends
// ".tmp-<uuid>" as a sibling path. Keep the sanitized name short enough that
// the temp-file component stays under conservative filesystem limits.
const UPLOAD_COMPONENT_BUDGET_BYTES: usize = 90;
const UPLOAD_DEST_PREFIX_BYTES: usize = 25; // YYYYMMDD-HHMMSS-XXXXXXXX-
const UPLOAD_TEMP_SUFFIX_BYTES: usize = 41; // .tmp-<uuid-v4>
const MAX_UPLOAD_FILE_NAME_BYTES: usize =
    UPLOAD_COMPONENT_BUDGET_BYTES - UPLOAD_DEST_PREFIX_BYTES - UPLOAD_TEMP_SUFFIX_BYTES;

fn sanitize_upload_file_name(file_name: &str) -> String {
    let trimmed = file_name.trim();
    let candidate = if trimmed.is_empty() {
        "attachment.bin"
    } else {
        trimmed
    };

    let mut sanitized: String = candidate
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            ch if ch.is_control() => '_',
            _ => ch,
        })
        .collect();

    sanitized = sanitized
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");

    sanitized = sanitized.trim_matches('.').trim().to_string();
    if sanitized.is_empty() {
        return "attachment.bin".to_string();
    }

    let stem = sanitized
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.']);
    if is_windows_reserved_device_name(stem) {
        sanitized.push_str("_file");
    }

    if sanitized.len() > MAX_UPLOAD_FILE_NAME_BYTES {
        sanitized = truncate_file_name_preserving_extension(&sanitized, MAX_UPLOAD_FILE_NAME_BYTES);
        sanitized = sanitized.trim_matches('.').trim().to_string();
        if sanitized.is_empty() {
            return "attachment.bin".to_string();
        }
    }
    sanitized
}

fn truncate_file_name_preserving_extension(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }

    let Some((stem, ext)) = input.rsplit_once('.') else {
        return truncate_utf8_to_max_bytes(input, max_bytes);
    };
    if stem.is_empty() || ext.is_empty() {
        return truncate_utf8_to_max_bytes(input, max_bytes);
    }

    let ext_with_dot = format!(".{}", ext);
    if ext_with_dot.len() >= max_bytes {
        return truncate_utf8_to_max_bytes(input, max_bytes);
    }

    let stem_budget = max_bytes - ext_with_dot.len();
    let mut stem_truncated = truncate_utf8_to_max_bytes(stem, stem_budget);
    stem_truncated = stem_truncated
        .trim_matches('.')
        .trim_end_matches(' ')
        .to_string();
    if stem_truncated.is_empty() {
        return truncate_utf8_to_max_bytes(input, max_bytes);
    }

    format!("{}{}", stem_truncated, ext_with_dot)
}

fn truncate_utf8_to_max_bytes(input: &str, max_bytes: usize) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if out.len() + ch.len_utf8() > max_bytes {
            break;
        }
        out.push(ch);
    }
    out
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    let upper = name.trim().to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        return true;
    }
    false
}

fn build_upload_destination_path(project_path: &str, file_name: String) -> PathBuf {
    let mut path = PathBuf::from(project_path);
    path.push(".mobilecli");
    path.push("uploads");
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let suffix = uuid::Uuid::new_v4().to_string();
    let short_suffix = &suffix[..8];
    path.push(format!("{}-{}-{}", stamp, short_suffix, file_name));
    path
}

async fn check_fs_rate_limit(state: &SharedState, addr: SocketAddr) -> Result<(), u64> {
    const REQUESTS_PER_SECOND: u32 = 100;
    const BURST_SIZE: u32 = 50;
    let mut st = state.write().await;
    let limiter = st
        .file_rate_limiters
        .entry(addr)
        .or_insert_with(|| RateLimiter::new(REQUESTS_PER_SECOND, BURST_SIZE));
    limiter.allow()
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
            cli_type: "terminal".to_string(),
            runtime: None,
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
            cli_type: "terminal".to_string(),
            runtime: None,
        })
        .collect();
    let msg = ServerMessage::Sessions { sessions: items };
    if let Ok(msg_str) = serde_json::to_string(&msg) {
        for client in st.mobile_clients.values() {
            let _ = client.try_send(Message::Text(msg_str.clone()));
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

/// Broadcast pty_resized to clients actively viewing this session.
async fn broadcast_pty_resized(
    state: &SharedState,
    session_id: &str,
    cols: u16,
    rows: u16,
    epoch: Option<u64>,
) {
    let msg = ServerMessage::PtyResized {
        session_id: session_id.to_string(),
        cols,
        rows,
        epoch,
    };
    let Ok(msg_str) = serde_json::to_string(&msg) else {
        return;
    };

    let ack_clients: Vec<mpsc::Sender<Message>> = {
        let st = state.read().await;
        st.mobile_views
            .iter()
            .filter_map(|(addr, views)| {
                if !views.contains(session_id) {
                    return None;
                }
                st.mobile_clients.get(addr).cloned()
            })
            .collect()
    };

    tracing::debug!(
        target: "overhaul.resize",
        session_id = %session_id,
        cols,
        rows,
        epoch = ?epoch,
        ack_clients = ack_clients.len(),
        "Broadcasted pty_resized"
    );

    for client in ack_clients {
        let _ = client.try_send(Message::Text(msg_str.clone()));
    }
}

/// Send current waiting states to a newly connected mobile client.
/// DEPRECATED: CLI detection removed, this function is now a no-op.
async fn send_waiting_states(
    _state: &SharedState,
    _tx: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // CLI detection removed - no waiting states to send
    Ok(())
}

fn truncate_to_max_chars(input: &mut String, max_chars: usize) {
    let len = input.chars().count();
    if len <= max_chars {
        return;
    }
    let trimmed: String = input.chars().skip(len - max_chars).collect();
    *input = trimmed;
}

fn tail_scrollback_bytes(session: &PtySession, max_bytes: usize) -> (Vec<u8>, usize) {
    let total = session.scrollback.len();
    let skip = total.saturating_sub(max_bytes);
    let bytes: Vec<u8> = session.scrollback.iter().skip(skip).copied().collect();
    (bytes, total)
}

fn client_supports_attach_v2(capabilities: u32) -> bool {
    capabilities & CLIENT_CAP_ATTACH_V2 != 0
}

fn should_use_attach_v2(flags: OverhaulFlags, capabilities: u32) -> bool {
    flags.attach_protocol == AttachProtocolMode::V2 && client_supports_attach_v2(capabilities)
}

fn chunk_snapshot_payload(data: &[u8]) -> Vec<String> {
    if data.is_empty() {
        return Vec::new();
    }
    data.chunks(SNAPSHOT_CHUNK_BYTES)
        .map(|chunk| BASE64.encode(chunk))
        .collect()
}

fn clear_mobile_attach_for_session(st: &mut DaemonState, session_id: &str) {
    let mut remove_addrs = Vec::new();
    for (addr, attach_by_session) in st.mobile_attach_ids.iter_mut() {
        attach_by_session.remove(session_id);
        if attach_by_session.is_empty() {
            remove_addrs.push(*addr);
        }
    }
    for addr in remove_addrs {
        st.mobile_attach_ids.remove(&addr);
    }
}

fn is_terminal_report_csi(body: &[u8], final_byte: u8) -> bool {
    match final_byte {
        // Device Attributes response (e.g. ESC[>0;276;0c from xterm.js)
        b'c' => body.starts_with(b">") || body.starts_with(b"?"),
        // Cursor position report (ESC[row;colR)
        b'R' => body.contains(&b';') && body.iter().all(|b| b.is_ascii_digit() || *b == b';'),
        // Status reports (ESC[0n / ESC[3n / ESC[?...)
        b'n' => body.starts_with(b"?") || body.iter().all(|b| b.is_ascii_digit() || *b == b';'),
        // Mode reports (ESC[?...$y)
        b'y' => body.starts_with(b"?") && body.contains(&b'$'),
        _ => false,
    }
}

#[cfg(test)]
fn strip_terminal_report_sequences(input: &[u8]) -> (Vec<u8>, usize) {
    let mut tail = Vec::new();
    let (mut out, dropped) = strip_terminal_report_sequences_stateful(&mut tail, input);
    if !tail.is_empty() {
        out.extend_from_slice(&tail);
    }
    (out, dropped)
}

fn strip_terminal_report_sequences_stateful(tail: &mut Vec<u8>, input: &[u8]) -> (Vec<u8>, usize) {
    let mut scan = Vec::with_capacity(tail.len() + input.len());
    scan.extend_from_slice(tail);
    scan.extend_from_slice(input);
    tail.clear();

    let mut out = Vec::with_capacity(scan.len());
    let mut i = 0usize;
    let mut dropped = 0usize;

    while i < scan.len() {
        if scan[i] == 0x1b {
            if i + 1 >= scan.len() {
                tail.extend_from_slice(&scan[i..]);
                break;
            }

            if scan[i + 1] == b'[' {
                let mut j = i + 2;
                let mut handled = false;
                while j < scan.len() {
                    let b = scan[j];
                    if b.is_ascii_alphabetic() || b == b'~' {
                        let body = &scan[i + 2..j];
                        if is_terminal_report_csi(body, b) {
                            dropped += 1;
                        } else {
                            out.extend_from_slice(&scan[i..=j]);
                        }
                        i = j + 1;
                        handled = true;
                        break;
                    }
                    if b == 0x1b || (j - i) > 64 {
                        // Malformed sequence. Keep byte-for-byte fallback behavior.
                        out.push(scan[i]);
                        i += 1;
                        handled = true;
                        break;
                    }
                    j += 1;
                }

                if !handled {
                    // Sequence is incomplete across websocket frames.
                    tail.extend_from_slice(&scan[i..]);
                    break;
                }
                continue;
            }
        }

        out.push(scan[i]);
        i += 1;
    }

    if tail.len() > 64 {
        let keep = tail.len() - 64;
        tail.drain(..keep);
    }

    (out, dropped)
}

fn is_noop_resize(last_applied: Option<(u16, u16)>, cols: u16, rows: u16) -> bool {
    last_applied == Some((cols, rows))
}

/// Update alternate-screen state from a PTY chunk, including sequences split
/// across chunk boundaries.
fn update_alt_screen_state(in_alt_screen: &mut bool, tail: &mut Vec<u8>, chunk: &[u8]) {
    if chunk.is_empty() {
        return;
    }

    let mut scan = Vec::with_capacity(tail.len() + chunk.len());
    scan.extend_from_slice(tail);
    scan.extend_from_slice(chunk);

    for i in 0..scan.len() {
        let rem = &scan[i..];
        if ALT_ENTER_SEQS.iter().any(|seq| rem.starts_with(seq)) {
            *in_alt_screen = true;
            continue;
        }
        if ALT_LEAVE_SEQS.iter().any(|seq| rem.starts_with(seq)) {
            *in_alt_screen = false;
        }
    }

    let keep = ALT_TRACK_TAIL_BYTES.min(scan.len());
    tail.clear();
    tail.extend_from_slice(&scan[scan.len() - keep..]);
}

async fn cleanup_client_state(state: &SharedState, addr: SocketAddr) {
    let (sessions_to_restore, to_unwatch) = {
        let mut st = state.write().await;
        let stale_sender_ids: Vec<String> = st
            .mobile_sender_addrs
            .iter()
            .filter_map(|(sender_id, mapped_addr)| {
                (*mapped_addr == addr).then_some(sender_id.clone())
            })
            .collect();
        for sender_id in stale_sender_ids {
            st.mobile_sender_addrs.remove(&sender_id);
        }
        st.mobile_client_capabilities.remove(&addr);
        st.mobile_attach_ids.remove(&addr);

        let sessions_to_restore = match st.mobile_views.remove(&addr) {
            Some(sessions) => {
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
            }
            None => Vec::new(),
        };

        let to_unwatch = match st.file_watch_subscriptions.remove(&addr) {
            Some(paths) => {
                let mut to_unwatch = Vec::new();
                for path in paths {
                    if let Some(count) = st.file_watch_counts.get_mut(&path) {
                        if *count > 0 {
                            *count -= 1;
                        }
                        if *count == 0 {
                            st.file_watch_counts.remove(&path);
                            to_unwatch.push(path);
                        }
                    }
                }
                to_unwatch
            }
            None => Vec::new(),
        };

        (sessions_to_restore, to_unwatch)
    };

    let fs = { state.read().await.file_system.clone() };
    for path in to_unwatch {
        let _ = fs.watcher().unwatch(&path);
    }

    for session_id in sessions_to_restore {
        restore_pty_size(state, &session_id).await;
    }
}

/// Evict a stale mobile websocket address and clean all associated view/watch state.
async fn evict_mobile_addr(state: &SharedState, addr: SocketAddr) {
    let stale_tx = {
        let mut st = state.write().await;
        st.file_rate_limiters.remove(&addr);
        st.mobile_clients.remove(&addr)
    };
    if let Some(tx) = stale_tx {
        let _ = tx.try_send(Message::Close(None));
    }
    cleanup_client_state(state, addr).await;
}

fn is_path_watched(changed_path: &str, watched: &std::collections::HashSet<String>) -> bool {
    if watched.contains(changed_path) {
        return true;
    }
    let parent = std::path::Path::new(changed_path)
        .parent()
        .map(crate::filesystem::path_utils::to_protocol_path);
    if let Some(parent) = parent {
        return watched.contains(&parent);
    }
    false
}

async fn restore_pty_size(state: &SharedState, session_id: &str) {
    let st = state.read().await;
    if let Some(session) = st.sessions.get(session_id) {
        let _ = session.resize_tx.send(ResizeRequest {
            cols: 0,
            rows: 0,
            epoch: None,
            reason: PtyResizeReason::DetachRestore,
        });
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

#[cfg(test)]
mod tests {
    use super::{
        broadcast_pty_resized, build_upload_destination_path,
        clear_mobile_attach_for_session, is_noop_resize,
        is_windows_reserved_device_name, sanitize_upload_file_name,
        should_use_attach_v2,
        strip_terminal_report_sequences, strip_terminal_report_sequences_stateful,
        update_alt_screen_state,
        AttachProtocolMode, DaemonState, OverhaulFlags, PtyResizeReason,
        CLIENT_CAP_ATTACH_V2, DEFAULT_SCROLLBACK_MAX_BYTES, MAX_UPLOAD_FILE_NAME_BYTES,
    };
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::{mpsc, RwLock};
    use tokio::time::{timeout, Duration};
    use tokio_tungstenite::tungstenite::Message;

    #[test]
    fn default_scrollback_is_large_enough_for_frame_clis() {
        assert_eq!(DEFAULT_SCROLLBACK_MAX_BYTES, 8 * 1024 * 1024);
        assert!(DEFAULT_SCROLLBACK_MAX_BYTES > 64 * 1024);
    }

    #[test]
    fn strip_terminal_reports_drops_secondary_da_reply() {
        let input = b"\x1b[>0;276;0c";
        let (out, dropped) = strip_terminal_report_sequences(input);
        assert_eq!(dropped, 1);
        assert!(out.is_empty());
    }

    #[test]
    fn strip_terminal_reports_preserves_user_escape_keys() {
        let input = b"abc\x1b[A\r";
        let (out, dropped) = strip_terminal_report_sequences(input);
        assert_eq!(dropped, 0);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_terminal_reports_removes_embedded_report_sequences() {
        let input = b"hello\x1b[>0;276;0cworld";
        let (out, dropped) = strip_terminal_report_sequences(input);
        assert_eq!(dropped, 1);
        assert_eq!(out, b"helloworld");
    }

    #[test]
    fn strip_terminal_reports_stateful_drops_split_da_reply() {
        let mut tail = Vec::new();
        let (out1, dropped1) = strip_terminal_report_sequences_stateful(&mut tail, b"\x1b[>");
        assert_eq!(dropped1, 0);
        assert!(out1.is_empty());
        assert!(!tail.is_empty());

        let (out2, dropped2) = strip_terminal_report_sequences_stateful(&mut tail, b"0;276;0c");
        assert_eq!(dropped2, 1);
        assert!(out2.is_empty());
        assert!(tail.is_empty());
    }

    #[test]
    fn strip_terminal_reports_stateful_preserves_split_arrow_key() {
        let mut tail = Vec::new();
        let (out1, dropped1) = strip_terminal_report_sequences_stateful(&mut tail, b"\x1b[");
        assert_eq!(dropped1, 0);
        assert!(out1.is_empty());
        assert!(!tail.is_empty());

        let (out2, dropped2) = strip_terminal_report_sequences_stateful(&mut tail, b"A");
        assert_eq!(dropped2, 0);
        assert_eq!(out2, b"\x1b[A");
        assert!(tail.is_empty());
    }

    #[test]
    fn sanitize_upload_file_name_replaces_invalid_chars() {
        let out = sanitize_upload_file_name(r#" folder/..\bad:name?.txt "#);
        assert_eq!(out, "folder_.._bad_name_.txt");
    }

    #[test]
    fn sanitize_upload_file_name_handles_windows_reserved_names() {
        assert!(is_windows_reserved_device_name("con"));
        assert!(is_windows_reserved_device_name("LPT1"));
        assert!(!is_windows_reserved_device_name("config"));

        let out = sanitize_upload_file_name("con.txt");
        assert_eq!(out, "con.txt_file");
    }

    #[test]
    fn sanitize_upload_file_name_truncates_without_unicode_panic() {
        let input = "你".repeat(200);
        let out = sanitize_upload_file_name(&input);
        assert!(!out.is_empty());
        assert!(out.len() <= MAX_UPLOAD_FILE_NAME_BYTES);
    }

    #[test]
    fn sanitize_upload_file_name_preserves_extension_on_truncate() {
        let input = format!("{}{}", "a".repeat(200), ".png");
        let out = sanitize_upload_file_name(&input);
        assert!(out.ends_with(".png"));
        assert!(out.len() <= MAX_UPLOAD_FILE_NAME_BYTES);
    }

    #[test]
    fn build_upload_destination_path_uses_expected_folder_structure() {
        let out = build_upload_destination_path("/tmp/project", "image.png".to_string());
        let components: Vec<String> = out
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        assert!(components.contains(&".mobilecli".to_string()));
        assert!(components.contains(&"uploads".to_string()));
        let last = components.last().map(String::as_str).unwrap_or_default();
        assert!(last.ends_with("-image.png"));
    }

    #[tokio::test]
    async fn unicode_upload_name_stays_within_filesystem_limits() {
        let temp = TempDir::new().expect("tempdir");
        let project = temp.path().join("project");
        tokio::fs::create_dir_all(&project)
            .await
            .expect("create project");

        let file_name = sanitize_upload_file_name(&(String::from("你").repeat(160) + ".txt"));
        assert!(file_name.len() <= MAX_UPLOAD_FILE_NAME_BYTES);

        let path = build_upload_destination_path(&project.to_string_lossy(), file_name);
        let parent = path.parent().expect("parent").to_path_buf();
        tokio::fs::create_dir_all(parent)
            .await
            .expect("create uploads dir");

        tokio::fs::write(&path, b"x")
            .await
            .expect("unicode path should be writable");
    }

    #[test]
    fn update_alt_screen_state_detects_split_enter_and_leave_sequences() {
        let mut in_alt = false;
        let mut tail = Vec::new();

        update_alt_screen_state(&mut in_alt, &mut tail, b"\x1b[?10");
        assert!(!in_alt);

        update_alt_screen_state(&mut in_alt, &mut tail, b"49h");
        assert!(in_alt);

        update_alt_screen_state(&mut in_alt, &mut tail, b"\x1b[?1049l");
        assert!(!in_alt);
    }

    #[test]
    fn noop_resize_detection_matches_last_applied_dimensions() {
        assert!(is_noop_resize(Some((80, 24)), 80, 24));
        assert!(!is_noop_resize(Some((80, 24)), 81, 24));
        assert!(!is_noop_resize(None, 80, 24));
    }

    #[test]
    fn attach_v2_gating_requires_flag_and_capability() {
        let v1_flags = OverhaulFlags {
            attach_protocol: AttachProtocolMode::V1,
            enable_local_echo: false,
            resize_simplified: false,
        };
        let v2_flags = OverhaulFlags {
            attach_protocol: AttachProtocolMode::V2,
            enable_local_echo: false,
            resize_simplified: false,
        };

        assert!(!should_use_attach_v2(v1_flags, CLIENT_CAP_ATTACH_V2));
        assert!(!should_use_attach_v2(v2_flags, 0));
        assert!(should_use_attach_v2(v2_flags, CLIENT_CAP_ATTACH_V2));
    }

    #[test]
    fn clear_mobile_attach_for_session_prunes_empty_maps() {
        let mut state = DaemonState::new(9847);
        let addr_a: std::net::SocketAddr = "127.0.0.1:50001".parse().expect("socket addr a");
        let addr_b: std::net::SocketAddr = "127.0.0.1:50002".parse().expect("socket addr b");

        state.mobile_attach_ids.insert(
            addr_a,
            std::collections::HashMap::from([
                ("target".to_string(), 1u64),
                ("keep".to_string(), 2u64),
            ]),
        );
        state.mobile_attach_ids.insert(
            addr_b,
            std::collections::HashMap::from([("target".to_string(), 9u64)]),
        );

        clear_mobile_attach_for_session(&mut state, "target");

        let a_map = state
            .mobile_attach_ids
            .get(&addr_a)
            .expect("addr_a survives");
        assert!(!a_map.contains_key("target"));
        assert_eq!(a_map.get("keep"), Some(&2u64));
        assert!(
            !state.mobile_attach_ids.contains_key(&addr_b),
            "addr_b should be pruned when its map becomes empty"
        );
    }

    #[tokio::test]
    async fn pty_resized_broadcasts_ack_only() {
        let state = Arc::new(RwLock::new(DaemonState::new(9847)));
        let addr: std::net::SocketAddr = "127.0.0.1:40001".parse().expect("socket addr");
        let session_id = "test-session".to_string();

        let (client_tx, mut client_rx) = mpsc::channel::<Message>(32);

        {
            let mut st = state.write().await;
            st.mobile_clients.insert(addr, client_tx);
            st.mobile_views
                .entry(addr)
                .or_default()
                .insert(session_id.clone());
            st.session_view_counts.insert(session_id.clone(), 1);
        }

        broadcast_pty_resized(&state, &session_id, 95, 27, Some(1)).await;

        let mut first_ack = 0usize;
        let mut first_replay = 0usize;
        loop {
            match timeout(Duration::from_millis(50), client_rx.recv()).await {
                Ok(Some(Message::Text(text))) => {
                    let msg: serde_json::Value =
                        serde_json::from_str(text.as_ref()).expect("valid json");
                    match msg.get("type").and_then(|t| t.as_str()) {
                        Some("pty_resized") => first_ack += 1,
                        Some("pty_bytes") => first_replay += 1,
                        _ => {}
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }

        assert_eq!(first_ack, 1);
        assert_eq!(first_replay, 0);

        broadcast_pty_resized(&state, &session_id, 96, 27, Some(2)).await;

        let mut second_ack = 0usize;
        let mut second_replay = 0usize;
        loop {
            match timeout(Duration::from_millis(50), client_rx.recv()).await {
                Ok(Some(Message::Text(text))) => {
                    let msg: serde_json::Value =
                        serde_json::from_str(text.as_ref()).expect("valid json");
                    match msg.get("type").and_then(|t| t.as_str()) {
                        Some("pty_resized") => second_ack += 1,
                        Some("pty_bytes") => second_replay += 1,
                        _ => {}
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }

        assert_eq!(second_ack, 1);
        assert_eq!(second_replay, 0);
    }
}
