//! Background daemon for MobileCLI
//!
//! Single WebSocket server that all terminal sessions stream to.
//! Mobile connects once and sees all active sessions.

use crate::detection::{
    detect_wait_event, strip_ansi_and_normalize, ApprovalModel, CliTracker, CliType, WaitType,
};
use crate::filesystem::{config::FileSystemConfig, rate_limit::RateLimiter, FileSystemService};
use crate::platform;
use crate::protocol::{
    ChangeType, ClientMessage, FileEncoding, FileSystemError, PtyResizeReason, ServerMessage,
    SessionListItem, TmuxViewportAction,
};
use crate::session::{self, SessionInfo};
use crate::tmux::sanitize_tmux_token;
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

        // On Windows, process detection can fail across sessions even if daemon is alive
        // Try connecting to the port as a fallback before declaring daemon dead
        #[cfg(windows)]
        {
            if let Some(port) = get_port() {
                if is_port_open(port) {
                    return true;
                }
            }
        }
    }

    // Stale PID/port files cause the app to think the daemon is running when it isn't.
    let _ = std::fs::remove_file(pid_path);
    let _ = std::fs::remove_file(port_file());
    false
}

/// Quick check if a TCP port is open (used as fallback on Windows)
#[cfg(windows)]
fn is_port_open(port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;

    // Try to connect to localhost:port with a short timeout
    // We use a non-blocking connect with timeout
    match TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(500),
    ) {
        Ok(_) => true,
        Err(_) => false,
    }
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

/// Maximum tmux scrollback lines fetched via capture-pane on the subscribe path.
///
/// Mobile xterm.js holds 20 000 lines. Desktop sessions are typically 160–240 cols
/// while mobile is 80–100 cols, giving a worst-case wrap ratio of ~3×. So 10 000
/// tmux lines → up to ~30 000 visual lines, which keeps the buffer well-fed without
/// sending megabytes of scrollback that will immediately be discarded by xterm.js.
///
/// For on-demand `GetSessionHistory` requests the full 200 000-line depth is used.
const SUBSCRIBE_SCROLLBACK_LINES: usize = 10_000;
const HISTORY_SCROLLBACK_LINES: usize = 200_000;

const ALT_ENTER_SEQS: &[&[u8]] = &[b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
const ALT_LEAVE_SEQS: &[&[u8]] = &[b"\x1b[?1049l", b"\x1b[?1047l", b"\x1b[?47l"];
const ALT_TRACK_TAIL_BYTES: usize = 7;
const SNAPSHOT_CHUNK_BYTES: usize = 48 * 1024;
const CLIENT_CAP_ATTACH_V2: u32 = 1 << 0;
const TMUX_VIEWPORT_MIN_MAJOR: u32 = 3;
const TMUX_VIEWPORT_DEFAULT_COUNT: u16 = 1;
const TMUX_VIEWPORT_MAX_COUNT: u16 = 20;

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
    pub tmux_shared_viewport: bool,
}

impl OverhaulFlags {
    pub fn from_env() -> Self {
        Self {
            attach_protocol: AttachProtocolMode::from_env(),
            enable_local_echo: env_flag("MOBILECLI_ENABLE_LOCAL_ECHO", false),
            resize_simplified: env_flag("MOBILECLI_RESIZE_SIMPLIFIED", true),
            tmux_shared_viewport: env_flag("MOBILECLI_TMUX_SHARED_VIEWPORT", true),
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
    /// Execution runtime used by the wrapper (`pty` or `tmux`).
    pub runtime: String,
    /// tmux socket label (when runtime=tmux)
    pub tmux_socket: Option<String>,
    /// tmux session name (when runtime=tmux)
    pub tmux_session: Option<String>,
    pub name: String,
    pub command: String,
    pub project_path: String,
    pub started_at: chrono::DateTime<Utc>,
    pub input_tx: mpsc::UnboundedSender<Vec<u8>>,
    pub resize_tx: mpsc::UnboundedSender<ResizeRequest>,
    pub waiting_state: Option<WaitingState>,
    pub cli_tracker: CliTracker,
    pub last_wait_hash: Option<u64>,
    /// Scrollback buffer for session history (for linked terminals)
    /// Uses VecDeque for efficient front truncation when buffer is full
    pub scrollback: VecDeque<u8>,
    /// Maximum scrollback buffer size
    pub scrollback_max_bytes: usize,
    /// Whether the CLI is currently in alternate screen buffer mode
    pub in_alt_screen: bool,
    /// Tail bytes from prior chunk used to detect alt-screen escape sequences
    /// split across PTY read boundaries.
    pub alt_track_tail: Vec<u8>,
    /// Latest accepted resize epoch from mobile (for stale resize rejection).
    pub last_resize_epoch: u64,
    /// Last dimensions acknowledged by the PTY wrapper.
    pub last_applied_size: Option<(u16, u16)>,
    /// Sequence number for live PTY chunks (Phase 0 observability scaffold).
    pub live_seq: u64,
    /// Tail for raw mobile input filtering to handle escape sequences split
    /// across websocket messages.
    pub raw_input_tail: Vec<u8>,
}

/// Daemon shared state
pub struct DaemonState {
    pub sessions: HashMap<String, PtySession>,
    pub overhaul_flags: OverhaulFlags,
    pub tmux_viewport_supported: bool,
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
    /// Current mobile controller socket for tmux shared viewport per session.
    pub tmux_viewport_controllers: HashMap<String, SocketAddr>,
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
        let overhaul_flags = OverhaulFlags::from_env();
        let tmux_viewport_supported =
            detect_tmux_viewport_support(overhaul_flags.tmux_shared_viewport);

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
            overhaul_flags,
            tmux_viewport_supported,
            next_attach_id: 1,
            mobile_clients: HashMap::new(),
            mobile_client_capabilities: HashMap::new(),
            mobile_attach_ids: HashMap::new(),
            mobile_sender_addrs: HashMap::new(),
            pty_broadcast,
            port,
            push_tokens: Vec::new(),
            mobile_views: HashMap::new(),
            tmux_viewport_controllers: HashMap::new(),
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
            tmux_shared_viewport = st.overhaul_flags.tmux_shared_viewport,
            tmux_viewport_supported = st.tmux_viewport_supported,
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
    let runtime = reg_msg["runtime"].as_str().unwrap_or("pty").to_lowercase();
    let (tmux_socket, tmux_session) = if runtime == "tmux" {
        let token = sanitize_tmux_token(&session_id);
        let name = format!("mcli-{}", token);
        (Some(name.clone()), Some(name))
    } else {
        (None, None)
    };

    tracing::info!(
        "PTY session registered: {} ({}) runtime={}",
        name,
        session_id,
        runtime
    );

    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<ResizeRequest>();

    // Register session
    let pty_broadcast = {
        let mut cli_tracker = CliTracker::new();
        cli_tracker.update_from_command(&command);

        let mut st = state.write().await;
        st.sessions.insert(
            session_id.clone(),
            PtySession {
                session_id: session_id.clone(),
                runtime: runtime.clone(),
                tmux_socket,
                tmux_session,
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
                in_alt_screen: false,
                alt_track_tail: Vec::new(),
                last_resize_epoch: 0,
                last_applied_size: None,
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
                                                // tmux runtime runs with alternate-screen disabled
                                                // during bootstrap, so raw 1049/1047 bytes from
                                                // frame CLIs are not a reliable signal for
                                                // mobile-side alt-screen policy.
                                                if session.runtime == "tmux" {
                                                    session.in_alt_screen = false;
                                                    session.alt_track_tail.clear();
                                                } else {
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
            st.tmux_viewport_controllers.remove(&session_id);
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
        "claude",
        "codex",
        "gemini",
        "opencode",
        "bash",
        "zsh",
        "sh",
        "fish",
        "nu",
        "pwsh",
        "powershell",
        "python",
        "python3",
        "node",
        "ruby",
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
    let mut tokens = vec![
        mobilecli_bin.to_string(),
        "--name".to_string(),
        session_name.to_string(),
    ];
    // Use quiet mode to prevent "Connected!" message from scrolling the terminal
    tokens.push("--quiet".to_string());
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

    // Map Unix shell commands to Windows equivalents
    let (effective_command, effective_args): (&str, Vec<String>) = match command {
        "bash" | "sh" | "zsh" => {
            // Use PowerShell as the Windows equivalent
            ("powershell", vec![])
        }
        _ => (command, args.to_vec()),
    };

    // Spawn mobilecli directly with CREATE_NEW_CONSOLE to create a visible terminal window.
    // Note: When the daemon runs as a service, windows may not be visible on the desktop
    // due to session isolation. This is a Windows security limitation.
    let mut cmd = std::process::Command::new(&mobilecli_bin);
    cmd.arg("--name").arg(session_name);
    // Use quiet mode to prevent "Connected!" message from scrolling the terminal
    cmd.arg("--quiet");
    if let Some(dir) = working_dir {
        cmd.arg("--dir").arg(dir);
        cmd.current_dir(dir);
    }
    cmd.arg(effective_command);
    cmd.args(effective_args);
    // Keep host desktop geometry stable for Windows demo sessions even if
    // the machine has a legacy mirror policy set in the environment.
    cmd.env("MOBILECLI_DESKTOP_RESIZE_POLICY", "preserve");

    // CREATE_NEW_CONSOLE (0x00000010) creates a new console window for the process.
    // This makes the terminal window visible when possible.
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
                // Konsole's -e consumes the rest of the command line
                // Pass all args as a single coherent command to avoid quote issues
                let full_cmd = format!("{} {}", shell, shell_args.join(" "));
                c.arg("-e").arg("/bin/sh").arg("-c").arg(&full_cmd);
            }
            "xterm" => {
                // In headless Xvfb environments, xterm needs explicit geometry
                // and font settings to render correctly without a real display.
                // -geometry: Set terminal size (160 columns x 50 rows is generous for modern apps)
                // -fa: Use a standard monospace font to avoid font rendering issues
                // -fg/-bg: Explicit foreground/background colors
                c.args([
                    "-geometry",
                    "160x50",
                    "-fa",
                    "Monospace",
                    "-fg",
                    "white",
                    "-bg",
                    "black",
                    "-e",
                    &shell,
                ])
                .args(&shell_args);
            }
            "urxvt" => {
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

    // On Linux/Unix, verify a display server is available before using GUI terminal emulators.
    // Without DISPLAY or WAYLAND_DISPLAY, GUI terminals silently fail (e.g., when running
    // as a systemd service), causing sessions to never appear on mobile.
    #[cfg(not(target_os = "windows"))]
    {
        let has_display = std::env::var("DISPLAY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
            || std::env::var("WAYLAND_DISPLAY")
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);

        if !has_display {
            tracing::info!(
                "No display server detected (DISPLAY/WAYLAND_DISPLAY not set), \
                falling back to headless mode"
            );
            return Err("No display server available".into());
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
            tracing::debug!("Client subscribed to session: {}", session_id);
            let attach_started = std::time::Instant::now();
            let mut st = state.write().await;
            if let Some(capabilities) = client_capabilities {
                st.mobile_client_capabilities.insert(addr, capabilities);
            }
            let attach_id = st.next_attach_id;
            st.next_attach_id = st.next_attach_id.saturating_add(1);
            let overhaul_flags = st.overhaul_flags;
            let capabilities = st
                .mobile_client_capabilities
                .get(&addr)
                .copied()
                .unwrap_or(0);
            let use_attach_v2 = should_use_attach_v2(overhaul_flags, capabilities);

            // Collect session state under one lock, then drop it before sending.
            let (
                scrollback_bytes,
                render_as_tui,
                mobile_in_alt_screen,
                scrollback_len,
                runtime,
                tmux_snapshot_req,
                initial_live_seq,
                ready_cols,
                ready_rows,
            ) = if let Some(session) = st.sessions.get(&session_id) {
                let render_as_tui =
                    should_treat_as_tui_for_mobile(&session.runtime, session.in_alt_screen);
                let mobile_in_alt_screen =
                    should_mobile_enter_alt_screen(&session.runtime, session.in_alt_screen);
                let has_scrollback = !session.scrollback.is_empty();
                let runtime = session.runtime.clone();
                // tmux sessions always replay from capture-pane. Alternate-screen
                // sessions replay only the visible pane; main-screen sessions may
                // include scrollback.
                let tmux_snapshot_req = if runtime == "tmux" {
                    match (&session.tmux_socket, &session.tmux_session) {
                        (Some(socket), Some(name)) => Some((
                            socket.clone(),
                            name.clone(),
                            session.scrollback_max_bytes,
                            !session.in_alt_screen,
                        )),
                        _ => None,
                    }
                } else {
                    None
                };
                let sb = if runtime != "tmux" && !render_as_tui && has_scrollback {
                    Some(session.scrollback.iter().copied().collect::<Vec<u8>>())
                } else {
                    None
                };
                let (ready_cols, ready_rows) = session.last_applied_size.unwrap_or((0, 0));
                (
                    sb,
                    render_as_tui,
                    mobile_in_alt_screen,
                    session.scrollback.len(),
                    runtime,
                    tmux_snapshot_req,
                    session.live_seq,
                    ready_cols,
                    ready_rows,
                )
            } else {
                (None, false, false, 0, "pty".to_string(), None, 0, 0, 0)
            };

            tracing::debug!(
                session_id = %session_id,
                addr = %addr,
                attach_id,
                runtime = %runtime,
                capabilities,
                use_attach_v2,
                last_seen_seq = ?last_seen_seq,
                render_as_tui,
                mobile_in_alt_screen,
                scrollback_len,
                text_replay_bytes = scrollback_bytes.as_ref().map(|b| b.len()).unwrap_or(0),
                "Subscribe replay decision"
            );
            drop(st);
            tracing::info!(
                target: "overhaul.attach",
                session_id = %session_id,
                addr = %addr,
                attach_id,
                runtime = %runtime,
                protocol = overhaul_flags.attach_protocol.as_str(),
                use_attach_v2,
                last_seen_seq = ?last_seen_seq,
                "Attach sequence started"
            );
            let clear_mode = if mobile_in_alt_screen {
                "alternate_screen"
            } else {
                "main_screen"
            };
            let mut replay_events: Vec<(&'static str, usize)> = Vec::new();
            if use_attach_v2 {
                let begin = ServerMessage::AttachBegin {
                    session_id: session_id.clone(),
                    attach_id,
                    runtime: runtime.clone(),
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
            }

            // ── Step 1: wipe mobile's xterm.js buffer ──────────────────
            //
            // On every (re-)subscribe we first clear the client terminal so
            // that replay data doesn't append to stale content from a prior
            // subscribe. Sessions in mobile alternate-screen use 1049h clear.
            // Text main-buffer sessions erase display+scrollback so the next
            // replay starts from a deterministic baseline.
            //
            {
                if use_attach_v2 {
                    let clear = ServerMessage::AttachClear {
                        session_id: session_id.clone(),
                        attach_id,
                    };
                    if let Ok(text) = serde_json::to_string(&clear) {
                        let _ = tx.send(Message::Text(text)).await;
                    }
                } else {
                    let clear: &[u8] = if mobile_in_alt_screen {
                        // \x1b[?1049h  enter alternate screen
                        // \x1b[2J      erase display
                        // \x1b[H       cursor home
                        b"\x1b[?1049h\x1b[2J\x1b[H"
                    } else {
                        // \x1b[2J      erase display
                        // \x1b[3J      erase scrollback (xterm extension, supported by xterm.js)
                        // \x1b[H       cursor home
                        b"\x1b[2J\x1b[3J\x1b[H"
                    };
                    let clear_msg = ServerMessage::PtyBytes {
                        session_id: session_id.clone(),
                        data: BASE64.encode(clear),
                    };
                    if let Ok(text) = serde_json::to_string(&clear_msg) {
                        let _ = tx.send(Message::Text(text)).await;
                    }
                }
                tracing::info!(
                    target: "overhaul.attach",
                    session_id = %session_id,
                    attach_id,
                    clear_mode,
                    clear_bytes = if use_attach_v2 { 0 } else if mobile_in_alt_screen { b"\x1b[?1049h\x1b[2J\x1b[H".len() } else { b"\x1b[2J\x1b[3J\x1b[H".len() },
                    "Attach clear frame sent"
                );
            }

            // ── Step 2: replay history ────────────────────────────────────
            //
            // PTY text sessions replay daemon scrollback.
            if !render_as_tui {
                if let Some(bytes) = scrollback_bytes {
                    replay_events.push(("daemon_scrollback", bytes.len()));
                    if use_attach_v2 {
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
                    } else {
                        let msg = ServerMessage::PtyBytes {
                            session_id: session_id.clone(),
                            data: BASE64.encode(&bytes),
                        };
                        if let Ok(text) = serde_json::to_string(&msg) {
                            let _ = tx.send(Message::Text(text)).await;
                        }
                    }
                    tracing::info!(
                        target: "overhaul.attach",
                        session_id = %session_id,
                        attach_id,
                        replay_source = "daemon_scrollback",
                        replay_bytes = bytes.len(),
                        "Attach replay sent"
                    );
                }
            }

            // tmux sessions always get an authoritative pane snapshot on
            // subscribe.
            if let Some((socket, name, max_bytes, include_scrollback)) = tmux_snapshot_req {
                let max_lines = if include_scrollback {
                    SUBSCRIBE_SCROLLBACK_LINES
                } else {
                    0
                };
                if let Some(snapshot) =
                    capture_tmux_history_with_retry(socket, name, include_scrollback, max_lines)
                        .await
                {
                    let total_bytes = snapshot.len();
                    let skip = total_bytes.saturating_sub(max_bytes);
                    let bytes = &snapshot[skip..];
                    replay_events.push(("tmux_capture_pane", bytes.len()));
                    if use_attach_v2 {
                        let chunks = chunk_snapshot_payload(bytes);
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
                    } else {
                        let msg = ServerMessage::SessionHistory {
                            session_id: session_id.clone(),
                            data: BASE64.encode(bytes),
                            total_bytes,
                        };
                        if let Ok(text) = serde_json::to_string(&msg) {
                            let _ = tx.send(Message::Text(text)).await;
                        }
                    }
                    tracing::info!(
                        target: "overhaul.attach",
                        session_id = %session_id,
                        attach_id,
                        replay_source = "tmux_capture_pane",
                        replay_bytes = bytes.len(),
                        total_snapshot_bytes = total_bytes,
                        "Attach replay sent"
                    );
                } else {
                    tracing::warn!(
                        target: "overhaul.attach",
                        session_id = %session_id,
                        attach_id,
                        include_scrollback,
                        "tmux capture-pane unavailable during attach replay"
                    );
                }
            }

            // Always send SubscribeAck so mobile knows whether to suppress
            // stale bytes and enter alt-screen mode before resizing (v1 path).
            let runtime_for_log = runtime.clone();
            if use_attach_v2 {
                let ready_live_seq = {
                    let st = state.read().await;
                    st.sessions
                        .get(&session_id)
                        .map(|session| session.live_seq)
                        .unwrap_or(initial_live_seq)
                };
                tracing::debug!(
                    target: "overhaul.sequence",
                    session_id = %session_id,
                    attach_id,
                    initial_live_seq,
                    ready_live_seq,
                    "Attach ready sequence barrier"
                );
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
            } else {
                let ack = ServerMessage::SubscribeAck {
                    session_id: session_id.clone(),
                    in_alt_screen: mobile_in_alt_screen,
                    runtime: Some(runtime),
                };
                if let Ok(text) = serde_json::to_string(&ack) {
                    let _ = tx.send(Message::Text(text)).await;
                }
            }

            // Register active view after initial clear/replay/ack sequence so
            // live PTY stream can't interleave with bootstrap replay bytes.
            let mut st = state.write().await;
            let entry = st.mobile_views.entry(addr).or_default();
            if entry.insert(session_id.clone()) {
                let count = st
                    .session_view_counts
                    .entry(session_id.clone())
                    .or_insert(0);
                *count += 1;
            }
            if st.tmux_viewport_supported && runtime_for_log == "tmux" {
                let active_controller = st.tmux_viewport_controllers.get(&session_id).copied();
                match active_controller {
                    Some(controller) if controller != addr => {
                        let controller_is_viewing = st
                            .mobile_views
                            .get(&controller)
                            .map(|views| views.contains(&session_id))
                            .unwrap_or(false);
                        if controller_is_viewing {
                            tracing::debug!(
                                session_id = %session_id,
                                active_controller = %controller,
                                requester = %addr,
                                "Keeping existing tmux viewport controller on subscribe"
                            );
                        } else {
                            st.tmux_viewport_controllers
                                .insert(session_id.clone(), addr);
                            tracing::debug!(
                                session_id = %session_id,
                                stale_controller = %controller,
                                new_controller = %addr,
                                "Reassigned stale tmux viewport controller on subscribe"
                            );
                        }
                    }
                    Some(_) => {}
                    None => {
                        st.tmux_viewport_controllers
                            .insert(session_id.clone(), addr);
                    }
                }
            }
            if use_attach_v2 {
                st.mobile_attach_ids
                    .entry(addr)
                    .or_default()
                    .insert(session_id.clone(), attach_id);
            } else {
                let mut remove_addr = false;
                if let Some(attach_by_session) = st.mobile_attach_ids.get_mut(&addr) {
                    attach_by_session.remove(&session_id);
                    remove_addr = attach_by_session.is_empty();
                }
                if remove_addr {
                    st.mobile_attach_ids.remove(&addr);
                }
            }
            let replay_sources = if replay_events.is_empty() {
                "none".to_string()
            } else {
                replay_events
                    .iter()
                    .map(|(source, _)| *source)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let replay_bytes_total: usize = replay_events.iter().map(|(_, bytes)| *bytes).sum();
            tracing::info!(
                target: "overhaul.attach",
                session_id = %session_id,
                addr = %addr,
                attach_id,
                runtime = %runtime_for_log,
                protocol = overhaul_flags.attach_protocol.as_str(),
                use_attach_v2,
                clear_mode,
                replay_sources = %replay_sources,
                replay_events = replay_events.len(),
                replay_bytes_total,
                duration_ms = attach_started.elapsed().as_millis() as u64,
                "Attach sequence completed"
            );
        }
        ClientMessage::Unsubscribe { session_id } => {
            tracing::debug!("Client unsubscribed from session: {}", session_id);
            let mut st = state.write().await;
            let mut remove_attach_addr = false;
            let mut should_restore = false;
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
                            should_restore = true;
                        }
                    }
                }
            }
            if st.tmux_viewport_controllers.get(&session_id).copied() == Some(addr) {
                st.tmux_viewport_controllers.remove(&session_id);
                if let Some(next_addr) = st.mobile_views.iter().find_map(|(candidate, views)| {
                    views.contains(&session_id).then_some(*candidate)
                }) {
                    st.tmux_viewport_controllers
                        .insert(session_id.clone(), next_addr);
                }
            }
            drop(st);
            if should_restore {
                restore_pty_size(state, &session_id).await;
                return Ok(());
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
                            runtime = %session.runtime,
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
            let reason = resolve_resize_reason(cols, rows, reason);
            let mut st = state.write().await;
            let resize_simplified = st.overhaul_flags.resize_simplified;
            let is_restore = reason == PtyResizeReason::DetachRestore;
            let viewer_count = st
                .session_view_counts
                .get(&session_id)
                .copied()
                .unwrap_or(0);
            let sender_is_viewing = st
                .mobile_views
                .get(&addr)
                .map(|views| views.contains(&session_id))
                .unwrap_or(false);
            let mut synthetic_ack: Option<(u16, u16, Option<u64>)> = None;

            let ignore_no_viewer_resize =
                should_ignore_resize_without_viewers(is_restore, viewer_count, reason);
            if ignore_no_viewer_resize {
                tracing::debug!(
                    target: "overhaul.resize",
                    session_id = %session_id,
                    cols,
                    rows,
                    epoch = ?epoch,
                    reason = reason.as_str(),
                    viewer_count,
                    sender_is_viewing,
                    resize_simplified,
                    bootstrap_reason = matches!(reason, PtyResizeReason::AttachInit | PtyResizeReason::ReconnectSync),
                    decision = "ignored_no_active_viewers",
                    "Ignoring PTY resize with no active mobile viewers"
                );
                return Ok(());
            }

            // A restore request (0x0) must not override active dimensions while
            // other viewers are still attached to this session.
            if should_ignore_restore_resize(is_restore, viewer_count, sender_is_viewing) {
                tracing::debug!(
                    target: "overhaul.resize",
                    session_id = %session_id,
                    cols,
                    rows,
                    epoch = ?epoch,
                    reason = reason.as_str(),
                    viewer_count,
                    sender_is_viewing,
                    resize_simplified,
                    decision = "ignored_restore_guard",
                    "Ignoring PTY restore while active viewers remain"
                );
                return Ok(());
            }

            if let Some(session) = st.sessions.get_mut(&session_id) {
                if is_stale_resize_epoch(session.last_resize_epoch, epoch) {
                    tracing::debug!(
                        target: "overhaul.resize",
                        session_id = %session_id,
                        cols,
                        rows,
                        epoch = ?epoch,
                        reason = reason.as_str(),
                        viewer_count,
                        sender_is_viewing,
                        alt_screen = session.in_alt_screen,
                        last_epoch = session.last_resize_epoch,
                        resize_simplified,
                        decision = "ignored_stale_epoch",
                        "Ignoring stale PTY resize"
                    );
                    return Ok(());
                }
                if let Some(epoch) = epoch {
                    session.last_resize_epoch = epoch;
                }

                let ack_dims = session.last_applied_size.unwrap_or((cols, rows));
                if reason == PtyResizeReason::KeyboardOverlay {
                    tracing::debug!(
                        target: "overhaul.resize",
                        session_id = %session_id,
                        cols,
                        rows,
                        epoch = ?epoch,
                        reason = reason.as_str(),
                        viewer_count,
                        sender_is_viewing,
                        alt_screen = session.in_alt_screen,
                        resize_simplified,
                        decision = "ignored_keyboard_overlay",
                        "Ignoring keyboard-overlay resize (local UX only)"
                    );
                    synthetic_ack = Some((ack_dims.0, ack_dims.1, epoch));
                } else if is_noop_resize(session.last_applied_size, cols, rows) {
                    tracing::debug!(
                        target: "overhaul.resize",
                        session_id = %session_id,
                        cols,
                        rows,
                        epoch = ?epoch,
                        reason = reason.as_str(),
                        viewer_count,
                        sender_is_viewing,
                        alt_screen = session.in_alt_screen,
                        resize_simplified,
                        decision = "ignored_noop",
                        "Ignoring no-op PTY resize"
                    );
                    synthetic_ack = Some((ack_dims.0, ack_dims.1, epoch));
                } else {
                    tracing::debug!(
                        target: "overhaul.resize",
                        session_id = %session_id,
                        cols,
                        rows,
                        epoch = ?epoch,
                        reason = reason.as_str(),
                        viewer_count,
                        sender_is_viewing,
                        alt_screen = session.in_alt_screen,
                        resize_simplified,
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
            drop(st);
            if let Some((ack_cols, ack_rows, ack_epoch)) = synthetic_ack {
                broadcast_pty_resized(state, &session_id, ack_cols, ack_rows, ack_epoch).await;
            }
        }
        ClientMessage::TmuxViewport {
            session_id,
            action,
            count,
        } => {
            let auth_result = {
                let mut st = state.write().await;
                if !st.tmux_viewport_supported {
                    Err((
                        "tmux_viewport_disabled",
                        "tmux shared viewport control is disabled".to_string(),
                    ))
                } else {
                    let sender_is_viewing = st
                        .mobile_views
                        .get(&addr)
                        .map(|views| views.contains(&session_id))
                        .unwrap_or(false);
                    match st.sessions.get(&session_id) {
                        None => {
                            let message = format!("Session {} not found", session_id);
                            Err(("session_not_found", message))
                        }
                        Some(session) => {
                            let runtime = session.runtime.clone();
                            let tmux_socket = session.tmux_socket.clone();
                            let tmux_session = session.tmux_session.clone();

                            if runtime != "tmux" {
                                Err((
                                    "unsupported_runtime",
                                    "tmux viewport controls require a tmux runtime session"
                                        .to_string(),
                                ))
                            } else if !sender_is_viewing {
                                Err((
                                    "unauthorized_viewport",
                                    "viewport control requires an active mobile viewer".to_string(),
                                ))
                            } else {
                                // Action wins controller ownership. This prevents stale or
                                // ghost sockets from locking viewport control after reconnects.
                                match st
                                    .tmux_viewport_controllers
                                    .insert(session_id.clone(), addr)
                                {
                                    Some(previous) if previous != addr => {
                                        tracing::debug!(
                                            session_id = %session_id,
                                            previous_controller = %previous,
                                            new_controller = %addr,
                                            "Reassigned tmux viewport controller on action"
                                        );
                                    }
                                    _ => {}
                                }
                                Ok((tmux_socket, tmux_session))
                            }
                        }
                    }
                }
            };
            let (tmux_socket, tmux_session) = match auth_result {
                Ok(values) => values,
                Err((code, message)) => {
                    let msg = ServerMessage::Error {
                        code: code.to_string(),
                        message,
                    };
                    tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                    return Ok(());
                }
            };

            let Some(socket) = tmux_socket else {
                let msg = ServerMessage::Error {
                    code: "tmux_missing_metadata".to_string(),
                    message: "tmux socket metadata missing for session".to_string(),
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                return Ok(());
            };
            let Some(name) = tmux_session else {
                let msg = ServerMessage::Error {
                    code: "tmux_missing_metadata".to_string(),
                    message: "tmux session metadata missing for session".to_string(),
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
                return Ok(());
            };

            let clamped_count = count
                .unwrap_or(TMUX_VIEWPORT_DEFAULT_COUNT)
                .clamp(1, TMUX_VIEWPORT_MAX_COUNT);
            let action_result = apply_tmux_viewport_action_with_retry(
                socket.clone(),
                name.clone(),
                action,
                clamped_count,
            )
            .await;
            let viewport_state = query_tmux_viewport_state(socket.clone(), name.clone())
                .await
                .unwrap_or_default();

            // After scroll action, capture the visible viewport and send as PtyBytes
            // so mobile terminal shows the scrolled content formatted for its dimensions.
            // Only capture if we're still not following live (user is scrolled up).
            if action_result.is_ok() && !viewport_state.following_live {
                // Small delay to let tmux settle into its new scroll position
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                // Re-check state after delay to avoid race conditions
                let viewport_state = query_tmux_viewport_state(socket.clone(), name.clone())
                    .await
                    .unwrap_or_default();

                if !viewport_state.following_live {
                    if let Some(frame_bytes) = capture_tmux_viewport_simple(socket, name).await {
                        // Prepend clear + home escape sequences so mobile redraws fresh
                        let mut payload = Vec::with_capacity(frame_bytes.len() + 16);
                        payload.extend_from_slice(b"\x1b[2J\x1b[H"); // clear + home
                        payload.extend_from_slice(&frame_bytes);

                        let pty_msg = ServerMessage::PtyBytes {
                            session_id: session_id.clone(),
                            data: BASE64.encode(&payload),
                        };
                        if let Ok(json) = serde_json::to_string(&pty_msg) {
                            let _ = tx.send(Message::Text(json)).await;
                        }
                    }
                }
            }

            let state_msg = ServerMessage::TmuxViewportState {
                session_id: session_id.clone(),
                in_copy_mode: viewport_state.in_copy_mode,
                scroll_position: viewport_state.scroll_position,
                history_size: viewport_state.history_size,
                following_live: viewport_state.following_live,
            };
            tx.send(Message::Text(serde_json::to_string(&state_msg)?))
                .await?;

            if let Err(error) = action_result {
                let msg = ServerMessage::Error {
                    code: "tmux_viewport_action_failed".to_string(),
                    message: error,
                };
                tx.send(Message::Text(serde_json::to_string(&msg)?)).await?;
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
                    st.tmux_viewport_controllers.remove(&session_id);
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
            let mut tmux_capture_req: Option<(String, String, usize, bool)> = None;
            let (fallback_bytes, fallback_total_bytes) = {
                let st = state.read().await;
                if let Some(session) = st.sessions.get(&session_id) {
                    let max = max_bytes.unwrap_or(session.scrollback_max_bytes);
                    let render_as_tui =
                        should_treat_as_tui_for_mobile(&session.runtime, session.in_alt_screen);
                    if session.runtime == "tmux" {
                        if let (Some(socket), Some(name)) =
                            (session.tmux_socket.clone(), session.tmux_session.clone())
                        {
                            tmux_capture_req = Some((socket, name, max, !render_as_tui));
                        }
                        (Vec::new(), 0)
                    } else if render_as_tui {
                        // PTY alternate-screen sessions are live-only.
                        (Vec::new(), 0)
                    } else {
                        tail_scrollback_bytes(session, max)
                    }
                } else {
                    (Vec::new(), 0)
                }
            };

            let (data, total_bytes) =
                if let Some((socket, name, max, include_scrollback)) = tmux_capture_req {
                    // On-demand history requests use the full depth so users can
                    // retrieve the maximum available scrollback from tmux.
                    let max_lines = if include_scrollback {
                        HISTORY_SCROLLBACK_LINES
                    } else {
                        0
                    };
                    if let Some(snapshot) =
                        capture_tmux_history_with_retry(socket, name, include_scrollback, max_lines)
                            .await
                    {
                        let total = snapshot.len();
                        let skip = total.saturating_sub(max);
                        (BASE64.encode(&snapshot[skip..]), total)
                    } else {
                        tracing::warn!(
                            session_id = %session_id,
                            "tmux capture-pane unavailable for session_history request"
                        );
                        (String::new(), 0)
                    }
                } else {
                    tracing::debug!(
                        session_id = %session_id,
                        fallback_total_bytes = fallback_total_bytes,
                        "Using daemon scrollback replay for session_history"
                    );
                    (BASE64.encode(&fallback_bytes), fallback_total_bytes)
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
            tracing::info!(request_id = %request_id, path = %path, "ListDirectory request");
            if let Err(retry_after_ms) = check_fs_rate_limit(state, addr).await {
                tracing::warn!(request_id = %request_id, "ListDirectory rate limited");
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
            tracing::info!(request_id = %request_id, "ListDirectory calling fs.ops()");
            let fs = { state.read().await.file_system.clone() };
            tracing::info!(request_id = %request_id, "ListDirectory got fs, calling list_directory");
            match fs
                .ops()
                .list_directory(&path, include_hidden, sort_by, sort_order)
                .await
            {
                Ok((path, entries, total_count, truncated)) => {
                    tracing::info!(request_id = %request_id, total_count = total_count, "ListDirectory success");
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
                    tracing::warn!(request_id = %request_id, error = ?e, "ListDirectory error");
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
            cli_type: s.cli_tracker.current().as_str().to_string(),
            runtime: Some(s.runtime.clone()),
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
            runtime: Some(s.runtime.clone()),
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
            let _ = client.try_send(Message::Text(msg_str.clone()));
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
            let _ = client.try_send(Message::Text(msg_str.clone()));
        }
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

#[derive(Debug, Clone, Copy)]
struct TmuxViewportStateSnapshot {
    in_copy_mode: bool,
    scroll_position: usize,
    history_size: usize,
    following_live: bool,
}

impl Default for TmuxViewportStateSnapshot {
    fn default() -> Self {
        Self {
            in_copy_mode: false,
            scroll_position: 0,
            history_size: 0,
            following_live: true,
        }
    }
}

fn parse_tmux_version_major(version: &str) -> Option<u32> {
    let token = version.split_whitespace().nth(1)?;
    let major = token.split('.').next()?;
    major.parse::<u32>().ok()
}

fn detect_tmux_viewport_support(flag_enabled: bool) -> bool {
    if !flag_enabled {
        return false;
    }
    if which::which("tmux").is_err() {
        return false;
    }
    let output = std::process::Command::new("tmux").arg("-V").output();
    let Ok(out) = output else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    parse_tmux_version_major(raw.trim())
        .map(|major| major >= TMUX_VIEWPORT_MIN_MAJOR)
        .unwrap_or(false)
}

fn tmux_targets(session: &str) -> [String; 3] {
    [
        format!("{}:0.0", session),
        format!("{}:0", session),
        session.to_string(),
    ]
}

fn run_tmux_viewport_cmd(socket: &str, target: &str, args: &[&str]) -> Result<(), String> {
    if args.is_empty() {
        return Err("missing tmux command".to_string());
    }
    let mut cmd = std::process::Command::new("tmux");
    cmd.arg("-L")
        .arg(socket)
        .arg("-f")
        .arg("/dev/null")
        .env_remove("TMUX")
        .arg(args[0]);
    if args[0] == "send-keys" {
        cmd.arg("-t").arg(target).args(&args[1..]);
    } else {
        cmd.args(&args[1..]).arg("-t").arg(target);
    }
    let output = cmd.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn run_tmux_viewport_action_once(
    socket: &str,
    session: &str,
    action: TmuxViewportAction,
    count: u16,
) -> Result<(), String> {
    let command = match action {
        TmuxViewportAction::ScrollUp => "scroll-up",
        TmuxViewportAction::ScrollDown => "scroll-down",
        TmuxViewportAction::PageUp => "page-up",
        TmuxViewportAction::PageDown => "page-down",
        TmuxViewportAction::Follow => "cancel",
    };

    let mut last_error: Option<String> = None;
    for target in tmux_targets(session) {
        match action {
            TmuxViewportAction::Follow => {
                match run_tmux_viewport_cmd(socket, &target, &["send-keys", "-X", command]) {
                    Ok(()) => return Ok(()),
                    Err(error) => {
                        // Already-following clients can report "not in a mode"; treat as success.
                        if error.contains("not in a mode") {
                            return Ok(());
                        }
                        last_error = Some(error);
                    }
                }
            }
            _ => {
                // Enter copy-mode first to make paging deterministic.
                let _ = run_tmux_viewport_cmd(socket, &target, &["copy-mode", "-e"]);
                let count_str = count.to_string();
                match run_tmux_viewport_cmd(
                    socket,
                    &target,
                    &["send-keys", "-X", "-N", &count_str, command],
                ) {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = Some(error),
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "tmux viewport action failed".to_string()))
}

async fn apply_tmux_viewport_action_with_retry(
    socket: String,
    session: String,
    action: TmuxViewportAction,
    count: u16,
) -> Result<(), String> {
    let first = tokio::task::spawn_blocking({
        let socket = socket.clone();
        let session = session.clone();
        move || run_tmux_viewport_action_once(&socket, &session, action, count)
    })
    .await
    .map_err(|error| error.to_string())?;
    if first.is_ok() {
        return first;
    }

    tokio::time::sleep(Duration::from_millis(30)).await;
    tokio::task::spawn_blocking(move || {
        run_tmux_viewport_action_once(&socket, &session, action, count)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn parse_tmux_viewport_state(raw: &str) -> Option<TmuxViewportStateSnapshot> {
    let mut parts = raw.trim().split('|');
    let pane_in_mode = parts.next()?;
    let scroll_position = parts.next().unwrap_or("").trim();
    let history_size = parts.next().unwrap_or("").trim();

    let in_copy_mode = pane_in_mode.trim() == "1";
    let scroll_position = scroll_position.parse::<usize>().unwrap_or(0);
    let history_size = history_size.parse::<usize>().unwrap_or(0);
    Some(TmuxViewportStateSnapshot {
        in_copy_mode,
        scroll_position,
        history_size,
        following_live: !in_copy_mode || scroll_position == 0,
    })
}

fn query_tmux_viewport_state_blocking(
    socket: &str,
    session: &str,
) -> Option<TmuxViewportStateSnapshot> {
    for target in tmux_targets(session) {
        let output = std::process::Command::new("tmux")
            .arg("-L")
            .arg(socket)
            .arg("-f")
            .arg("/dev/null")
            .env_remove("TMUX")
            .arg("display-message")
            .arg("-p")
            .arg("-t")
            .arg(&target)
            .arg("#{pane_in_mode}|#{scroll_position}|#{history_size}")
            .output();
        if let Ok(out) = output {
            if !out.status.success() {
                continue;
            }
            if let Some(snapshot) = parse_tmux_viewport_state(&String::from_utf8_lossy(&out.stdout))
            {
                return Some(snapshot);
            }
        }
    }
    None
}

async fn query_tmux_viewport_state(
    socket: String,
    session: String,
) -> Option<TmuxViewportStateSnapshot> {
    tokio::task::spawn_blocking(move || query_tmux_viewport_state_blocking(&socket, &session))
        .await
        .ok()
        .flatten()
}

fn capture_tmux_history_blocking(
    socket: &str,
    session: &str,
    include_scrollback: bool,
    // Lines to request via `-S -N`. Ignored when include_scrollback is false.
    max_scrollback_lines: usize,
) -> Option<Vec<u8>> {
    let targets = [
        format!("{}:0.0", session),
        format!("{}:0", session),
        session.to_string(),
    ];
    for target in targets {
        let mut cmd = std::process::Command::new("tmux");
        cmd.arg("-L")
            .arg(socket)
            .arg("-f")
            .arg("/dev/null")
            .env_remove("TMUX")
            .arg("capture-pane")
            .arg("-p")
            .arg("-e");
        if include_scrollback {
            cmd.arg("-S").arg(format!("-{}", max_scrollback_lines));
        }
        cmd.arg("-t").arg(&target);
        let output = cmd.output();
        if let Ok(out) = output {
            if out.status.success() {
                return Some(out.stdout);
            }
        }
    }
    None
}

async fn capture_tmux_history(
    socket: String,
    session: String,
    include_scrollback: bool,
    max_scrollback_lines: usize,
) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        capture_tmux_history_blocking(&socket, &session, include_scrollback, max_scrollback_lines)
    })
    .await
    .ok()
    .flatten()
}

fn snapshot_has_visible_content(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
}

/// Simple viewport capture for scrolled content.
/// Captures the visible pane content in copy-mode formatted for mobile dimensions.
fn capture_tmux_viewport_simple_blocking(socket: &str, session: &str) -> Option<Vec<u8>> {
    for target in tmux_targets(session) {
        let mut cmd = std::process::Command::new("tmux");
        cmd.arg("-L")
            .arg(socket)
            .arg("-f")
            .arg("/dev/null")
            .env_remove("TMUX")
            .arg("capture-pane")
            .arg("-p")
            .arg("-e") // Preserve escape sequences (colors, etc.)
            .arg("-N") // No line numbers
            .arg("-t")
            .arg(&target);

        match cmd.output() {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                return Some(out.stdout);
            }
            _ => continue,
        }
    }
    None
}

async fn capture_tmux_viewport_simple(socket: String, session: String) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || capture_tmux_viewport_simple_blocking(&socket, &session))
        .await
        .ok()
        .flatten()
}

async fn capture_tmux_history_with_retry(
    socket: String,
    session: String,
    include_scrollback: bool,
    max_scrollback_lines: usize,
) -> Option<Vec<u8>> {
    if include_scrollback {
        return capture_tmux_history(socket, session, true, max_scrollback_lines).await;
    }

    const MAX_ATTEMPTS: usize = 8;
    const RETRY_DELAY_MS: u64 = 120;
    for attempt in 0..MAX_ATTEMPTS {
        if let Some(snapshot) =
            capture_tmux_history(socket.clone(), session.clone(), false, 0).await
        {
            if snapshot_has_visible_content(&snapshot) || attempt + 1 == MAX_ATTEMPTS {
                return Some(snapshot);
            }
        }
        tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
    }
    None
}

fn is_terminal_report_csi(body: &[u8], final_byte: u8) -> bool {
    match final_byte {
        // Strip only Device Attributes replies (e.g. ESC[>0;276;0c from xterm.js).
        // Keep CPR/DSR/mode reports intact because line editors (notably PSReadLine)
        // rely on them for cursor math and prompt repaint fidelity.
        b'c' => body.starts_with(b">") || body.starts_with(b"?"),
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

fn should_ignore_restore_resize(
    is_restore: bool,
    viewer_count: usize,
    sender_is_viewing: bool,
) -> bool {
    is_restore && (viewer_count > 1 || (viewer_count == 1 && !sender_is_viewing))
}

fn resolve_resize_reason(
    cols: u16,
    rows: u16,
    incoming: Option<PtyResizeReason>,
) -> PtyResizeReason {
    if cols == 0 || rows == 0 {
        return PtyResizeReason::DetachRestore;
    }

    match incoming {
        Some(reason @ PtyResizeReason::AttachInit)
        | Some(reason @ PtyResizeReason::GeometryChange)
        | Some(reason @ PtyResizeReason::ReconnectSync)
        | Some(reason @ PtyResizeReason::KeyboardOverlay) => reason,
        Some(PtyResizeReason::DetachRestore | PtyResizeReason::Unknown) => {
            PtyResizeReason::GeometryChange
        }
        None => PtyResizeReason::GeometryChange,
    }
}

fn is_noop_resize(last_applied: Option<(u16, u16)>, cols: u16, rows: u16) -> bool {
    last_applied == Some((cols, rows))
}

fn should_ignore_resize_without_viewers(
    is_restore: bool,
    viewer_count: usize,
    reason: PtyResizeReason,
) -> bool {
    if is_restore || viewer_count > 0 {
        return false;
    }

    !matches!(
        reason,
        PtyResizeReason::AttachInit | PtyResizeReason::ReconnectSync
    )
}

fn should_treat_as_tui_for_mobile(runtime: &str, in_alt_screen: bool) -> bool {
    // tmux runtime intentionally keeps alternate-screen disabled so pane
    // snapshots remain canonical and replay-safe.
    if runtime == "tmux" {
        return false;
    }
    in_alt_screen
}

fn should_mobile_enter_alt_screen(runtime: &str, in_alt_screen: bool) -> bool {
    // tmux runtime intentionally disables alternate-screen at the host level.
    // Ignore raw 1049/1047 bytes from pane output and keep mobile in main
    // buffer for deterministic replay + scrollback behavior.
    if runtime == "tmux" {
        return false;
    }
    in_alt_screen
}

fn is_stale_resize_epoch(last_epoch: u64, incoming_epoch: Option<u64>) -> bool {
    incoming_epoch.is_some_and(|epoch| epoch <= last_epoch)
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

        let (sessions_to_restore, sessions_detached) = match st.mobile_views.remove(&addr) {
            Some(sessions) => {
                let mut restore = Vec::new();
                let mut detached = Vec::new();
                for session_id in sessions {
                    detached.push(session_id.clone());
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
                (restore, detached)
            }
            None => (Vec::new(), Vec::new()),
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

        for session_id in &sessions_detached {
            if st.tmux_viewport_controllers.get(session_id).copied() == Some(addr) {
                st.tmux_viewport_controllers.remove(session_id);
                if let Some(next_addr) = st
                    .mobile_views
                    .iter()
                    .find_map(|(candidate, views)| views.contains(session_id).then_some(*candidate))
                {
                    st.tmux_viewport_controllers
                        .insert(session_id.clone(), next_addr);
                }
            }
        }

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
        broadcast_pty_resized, build_upload_destination_path, capture_tmux_history,
        clear_mobile_attach_for_session, is_noop_resize, is_stale_resize_epoch,
        is_windows_reserved_device_name, resolve_resize_reason, sanitize_upload_file_name,
        should_ignore_resize_without_viewers, should_ignore_restore_resize,
        should_mobile_enter_alt_screen, should_treat_as_tui_for_mobile, should_use_attach_v2,
        strip_terminal_report_sequences, strip_terminal_report_sequences_stateful,
        update_alt_screen_state, AttachProtocolMode, DaemonState, OverhaulFlags, PtyResizeReason,
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
    fn strip_terminal_reports_preserves_cursor_position_report() {
        let input = b"\x1b[12;45R";
        let (out, dropped) = strip_terminal_report_sequences(input);
        assert_eq!(dropped, 0);
        assert_eq!(out, input);
    }

    #[test]
    fn strip_terminal_reports_preserves_status_report() {
        let input = b"\x1b[0n";
        let (out, dropped) = strip_terminal_report_sequences(input);
        assert_eq!(dropped, 0);
        assert_eq!(out, input);
    }

    #[tokio::test]
    async fn tmux_capture_history_returns_snapshot_text() {
        if which::which("tmux").is_err() {
            return;
        }

        let token = &uuid::Uuid::new_v4().to_string()[..8];
        let socket = format!("mcli-test-{}-sock", token);
        let session = format!("mcli-test-{}-session", token);

        let spawn = std::process::Command::new("tmux")
            .arg("-L")
            .arg(&socket)
            .arg("-f")
            .arg("/dev/null")
            .env_remove("TMUX")
            .args([
                "new-session",
                "-d",
                "-s",
                &session,
                "printf CAPTURE_OK; sleep 2",
            ])
            .output()
            .expect("spawn tmux test session");
        assert!(
            spawn.status.success(),
            "failed to spawn tmux test session: {}",
            String::from_utf8_lossy(&spawn.stderr)
        );

        tokio::time::sleep(Duration::from_millis(150)).await;

        let snapshot = capture_tmux_history(socket.clone(), session.clone(), true, 1_000)
            .await
            .expect("tmux snapshot");
        let text = String::from_utf8_lossy(&snapshot);
        assert!(
            text.contains("CAPTURE_OK"),
            "snapshot missing expected marker: {}",
            text
        );

        let _ = std::process::Command::new("tmux")
            .arg("-L")
            .arg(&socket)
            .arg("-f")
            .arg("/dev/null")
            .env_remove("TMUX")
            .arg("kill-server")
            .status();
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
    fn restore_resize_rules_protect_active_viewers() {
        assert!(should_ignore_restore_resize(true, 2, true));
        assert!(should_ignore_restore_resize(true, 1, false));
        assert!(!should_ignore_restore_resize(true, 1, true));
        assert!(!should_ignore_restore_resize(false, 2, true));
    }

    #[test]
    fn stale_epoch_detection_rejects_older_or_equal_epochs() {
        assert!(!is_stale_resize_epoch(0, None));
        assert!(!is_stale_resize_epoch(5, Some(6)));
        assert!(is_stale_resize_epoch(5, Some(5)));
        assert!(is_stale_resize_epoch(5, Some(4)));
    }

    #[test]
    fn resolve_resize_reason_prefers_restore_for_zero_dimensions() {
        assert_eq!(
            resolve_resize_reason(0, 24, Some(PtyResizeReason::GeometryChange)),
            PtyResizeReason::DetachRestore
        );
        assert_eq!(
            resolve_resize_reason(80, 0, Some(PtyResizeReason::AttachInit)),
            PtyResizeReason::DetachRestore
        );
    }

    #[test]
    fn resolve_resize_reason_defaults_unknown_to_geometry_change() {
        assert_eq!(
            resolve_resize_reason(80, 24, None),
            PtyResizeReason::GeometryChange
        );
        assert_eq!(
            resolve_resize_reason(80, 24, Some(PtyResizeReason::Unknown)),
            PtyResizeReason::GeometryChange
        );
        assert_eq!(
            resolve_resize_reason(80, 24, Some(PtyResizeReason::DetachRestore)),
            PtyResizeReason::GeometryChange
        );
    }

    #[test]
    fn noop_resize_detection_matches_last_applied_dimensions() {
        assert!(is_noop_resize(Some((80, 24)), 80, 24));
        assert!(!is_noop_resize(Some((80, 24)), 81, 24));
        assert!(!is_noop_resize(None, 80, 24));
    }

    #[test]
    fn no_viewer_guard_allows_bootstrap_resizes() {
        assert!(should_ignore_resize_without_viewers(
            false,
            0,
            PtyResizeReason::GeometryChange
        ));
        assert!(!should_ignore_resize_without_viewers(
            false,
            0,
            PtyResizeReason::AttachInit
        ));
        assert!(!should_ignore_resize_without_viewers(
            false,
            0,
            PtyResizeReason::ReconnectSync
        ));
        assert!(!should_ignore_resize_without_viewers(
            true,
            0,
            PtyResizeReason::DetachRestore
        ));
        assert!(!should_ignore_resize_without_viewers(
            false,
            1,
            PtyResizeReason::GeometryChange
        ));
    }

    #[test]
    fn tui_gating_uses_alt_screen_only() {
        assert!(!should_treat_as_tui_for_mobile("tmux", false));
        assert!(!should_treat_as_tui_for_mobile("tmux", true));
        assert!(should_treat_as_tui_for_mobile("pty", true));
        assert!(!should_treat_as_tui_for_mobile("pty", false));
    }

    #[test]
    fn mobile_alt_screen_policy_preserves_tmux_scrollback() {
        assert!(!should_mobile_enter_alt_screen("tmux", false));
        assert!(!should_mobile_enter_alt_screen("tmux", true));
        assert!(should_mobile_enter_alt_screen("pty", true));
        assert!(!should_mobile_enter_alt_screen("pty", false));
    }

    #[test]
    fn attach_v2_gating_requires_flag_and_capability() {
        let v1_flags = OverhaulFlags {
            attach_protocol: AttachProtocolMode::V1,
            enable_local_echo: false,
            resize_simplified: false,
            tmux_shared_viewport: false,
        };
        let v2_flags = OverhaulFlags {
            attach_protocol: AttachProtocolMode::V2,
            enable_local_echo: false,
            resize_simplified: false,
            tmux_shared_viewport: false,
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
