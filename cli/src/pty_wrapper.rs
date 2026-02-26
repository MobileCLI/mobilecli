//! PTY wrapper - spawns commands in a PTY and streams to the daemon
//!
//! This module:
//! 1. Spawns the target command (or shell) in a PTY we control
//! 2. Connects to the daemon via WebSocket
//! 3. Streams PTY output to both local terminal AND daemon
//! 4. Relays input from daemon (mobile) to the PTY
//! 5. Handles terminal resize events

use crate::daemon::{get_port, DEFAULT_PORT};
use crate::protocol::PtyResizeReason;
use crate::tmux::sanitize_tmux_token;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use colored::Colorize;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::borrow::Cow;
use std::io::{IsTerminal, Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Error, Debug)]
pub enum WrapError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Daemon connection error: {0}")]
    DaemonConnection(String),
}

/// Configuration for running a wrapped command
pub struct WrapConfig {
    pub command: String,
    pub args: Vec<String>,
    pub session_name: String,
    pub quiet: bool,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeMode {
    Pty,
    Tmux,
}

impl RuntimeMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pty => "pty",
            Self::Tmux => "tmux",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TmuxMouseMode {
    On,
    Off,
}

impl TmuxMouseMode {
    fn as_tmux_value(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }

    fn default_for_platform() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Off
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::On
        }
    }
}

#[derive(Debug, Clone)]
struct TmuxContext {
    socket_name: String,
    session_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopResizePolicy {
    /// Keep the host terminal window unchanged; only resize the child PTY.
    Preserve,
    /// Mirror mobile dimensions into the host terminal window.
    Mirror,
}

impl DesktopResizePolicy {
    fn from_env() -> Self {
        let raw = std::env::var("MOBILECLI_DESKTOP_RESIZE_POLICY")
            .unwrap_or_else(|_| "preserve".to_string())
            .to_lowercase();
        match raw.as_str() {
            "mirror" | "strict" | "legacy" => Self::Mirror,
            "preserve" | "off" | "none" | "" => Self::Preserve,
            other => {
                tracing::warn!(
                    "Unknown MOBILECLI_DESKTOP_RESIZE_POLICY='{}'. Defaulting to 'preserve'.",
                    other
                );
                Self::Preserve
            }
        }
    }
}

/// Resolve a command to its full path
fn resolve_command(cmd: &str) -> Option<String> {
    // First check if it's already an absolute path
    if std::path::Path::new(cmd).is_absolute() && std::path::Path::new(cmd).exists() {
        return Some(cmd.to_string());
    }

    // Use which to find in PATH
    which::which(cmd)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Get terminal size from the current terminal
fn get_terminal_size() -> (u16, u16) {
    get_terminal_size_opt().unwrap_or((80, 24))
}

#[cfg(unix)]
fn get_terminal_size_opt() -> Option<(u16, u16)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            return Some((ws.ws_col, ws.ws_row));
        }
    }
    term_size::dimensions().map(|(w, h)| (w as u16, h as u16))
}

#[cfg(not(unix))]
fn get_terminal_size_opt() -> Option<(u16, u16)> {
    term_size::dimensions().map(|(w, h)| (w as u16, h as u16))
}

#[cfg(unix)]
fn set_stdout_winsize(cols: u16, rows: u16) {
    unsafe {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let _ = libc::ioctl(libc::STDOUT_FILENO, libc::TIOCSWINSZ, &ws);
    }
}

#[cfg(not(unix))]
fn set_stdout_winsize(_cols: u16, _rows: u16) {}

fn request_terminal_resize(cols: u16, rows: u16) {
    if !std::io::stdout().is_terminal() {
        return;
    }
    let seq = format!("\x1b[8;{};{}t", rows, cols);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(seq.as_bytes());
    let _ = stdout.flush();
    set_stdout_winsize(cols, rows);
}

fn tmux_base_command(socket_name: &str) -> Command {
    let mut cmd = Command::new("tmux");
    // Use platform-appropriate null device
    #[cfg(windows)]
    let null_dev = "NUL";
    #[cfg(not(windows))]
    let null_dev = "/dev/null";
    
    cmd.arg("-L")
        .arg(socket_name)
        .arg("-f")
        .arg(null_dev)
        .env_remove("TMUX");
    cmd
}

fn run_tmux_checked(cmd: &mut Command, context: &str) -> Result<(), WrapError> {
    let output = cmd
        .output()
        .map_err(|e| WrapError::Pty(format!("Failed to run tmux ({context}): {e}")))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    Err(WrapError::Pty(format!(
        "tmux command failed ({context}): {detail}"
    )))
}

fn resolve_runtime_mode() -> RuntimeMode {
    let requested = std::env::var("MOBILECLI_RUNTIME")
        .unwrap_or_else(|_| "auto".to_string())
        .to_lowercase();

    match requested.as_str() {
        "pty" | "legacy" => RuntimeMode::Pty,
        "tmux" => {
            if which::which("tmux").is_ok() {
                RuntimeMode::Tmux
            } else {
                tracing::warn!(
                    "MOBILECLI_RUNTIME=tmux requested, but tmux is not installed; falling back to pty runtime"
                );
                RuntimeMode::Pty
            }
        }
        // Default auto policy prefers tmux when available because frame-rendered
        // TUIs (Codex/OpenCode/etc.) need multiplexer semantics for reliable reattach.
        "auto" | "" => {
            if which::which("tmux").is_ok() {
                RuntimeMode::Tmux
            } else {
                RuntimeMode::Pty
            }
        }
        other => {
            tracing::warn!(
                "Unknown MOBILECLI_RUNTIME='{}'. Falling back to auto policy.",
                other
            );
            if which::which("tmux").is_ok() {
                RuntimeMode::Tmux
            } else {
                RuntimeMode::Pty
            }
        }
    }
}

fn parse_tmux_mouse_mode(raw: &str) -> Option<TmuxMouseMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Some(TmuxMouseMode::On),
        "off" | "0" | "false" | "no" => Some(TmuxMouseMode::Off),
        _ => None,
    }
}

fn parse_bool_env_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn resolve_raw_mode() -> bool {
    let Ok(raw) = std::env::var("MOBILECLI_RAW_MODE") else {
        return false;
    };

    if raw.trim().is_empty() {
        return false;
    }

    if let Some(enabled) = parse_bool_env_flag(&raw) {
        return enabled;
    }

    tracing::warn!(
        "Unknown MOBILECLI_RAW_MODE='{}'. Defaulting to line-buffered mode (disabled).",
        raw
    );
    false
}

fn resolve_tmux_mouse_mode() -> TmuxMouseMode {
    let default_mode = TmuxMouseMode::default_for_platform();
    let Ok(raw) = std::env::var("MOBILECLI_TMUX_MOUSE") else {
        return default_mode;
    };
    if raw.trim().is_empty() {
        return default_mode;
    }
    if let Some(mode) = parse_tmux_mouse_mode(&raw) {
        return mode;
    }
    tracing::warn!(
        "Unknown MOBILECLI_TMUX_MOUSE='{}'. Defaulting to '{}'.",
        raw,
        default_mode.as_tmux_value()
    );
    default_mode
}

fn setup_tmux_session(
    socket_name: &str,
    session_name: &str,
    command_path: &str,
    args: &[String],
    cwd: &str,
    cols: u16,
    rows: u16,
    tmux_mouse_mode: TmuxMouseMode,
) -> Result<(), WrapError> {
    // Bootstrap a dedicated tmux server first so global options apply before
    // the wrapped CLI starts. We also disable alternate-screen globally here
    // because the pty_wrapper runs tmux attach-session inside the host
    // terminal (e.g. Konsole). If alternate-screen is enabled, tmux forwards
    // \x1b[?1049h to the host terminal when TUI apps run, causing the host to
    // enter its own alt-screen and lose its scrollback. Disabling it keeps
    // all output in the main buffer where capture-pane can reach it.
    //
    // NOTE: We create the session first, then set options. The first window
    // will have default history-limit (2000), but we set global options for
    // future windows. This is a tmux limitation - history-limit can only be
    // set globally and affects windows created after it's set.
    let mut new_session = tmux_base_command(socket_name);
    new_session
        .arg("new-session")
        .arg("-d")
        .arg("-s")
        .arg(session_name)
        .arg("-x")
        .arg(cols.to_string())
        .arg("-y")
        .arg(rows.to_string())
        .arg("--")
        .arg(command_path)
        .args(args)
        .current_dir(cwd)
        .env("TERM", "xterm-256color")
        .env("MOBILECLI_SESSION", "1");
    run_tmux_checked(&mut new_session, "new-session")?;

    // Set global options for future windows. The first window already
    // exists with default settings, but new windows will inherit these.
    let mut set_history = tmux_base_command(socket_name);
    set_history
        .arg("set-option")
        .arg("-g")
        .arg("history-limit")
        .arg("200000");
    let _ = run_tmux_checked(&mut set_history, "history-limit");

    // Best-effort options for deterministic rendering behavior.
    // NOTE: window-size must remain dynamic so wrapper PTY resizes propagate
    // into tmux panes when mobile dimensions change.
    let window_target = format!("{}:0", session_name);
    // Disable smcup/rmcup so tmux itself does NOT enter Konsole's alternate
    // screen on attach. Without this, Konsole has zero scrollback while tmux
    // is attached (alt-screen has no history buffer). With it disabled, tmux
    // renders in Konsole's main buffer and the scrollbar works.
    let mut disable_altscreen_client = tmux_base_command(socket_name);
    disable_altscreen_client
        .arg("set-option")
        .arg("-g")
        .arg("terminal-overrides")
        .arg("xterm*:smcup@:rmcup@");
    let _ = run_tmux_checked(&mut disable_altscreen_client, "terminal-overrides");

    // Mouse mode is configurable. Linux defaults to off so desktop emulators
    // (e.g. Konsole) preserve normal drag-select clipboard behavior.
    let mut set_mouse_mode = tmux_base_command(socket_name);
    set_mouse_mode
        .arg("set-option")
        .arg("-g")
        .arg("mouse")
        .arg(tmux_mouse_mode.as_tmux_value());
    let _ = run_tmux_checked(&mut set_mouse_mode, "mouse");

    // history-limit is set globally before new-session so the window is
    // allocated with the full 200K-line buffer from the start.
    let option_sets: [(&str, &str, &str, &str); 4] = [
        ("set-option", session_name, "status", "off"),
        ("set-option", session_name, "allow-rename", "off"),
        (
            "set-window-option",
            &window_target,
            "alternate-screen",
            "off",
        ),
        ("set-window-option", &window_target, "window-size", "latest"),
    ];
    for (command, target, key, value) in option_sets {
        let mut option_cmd = tmux_base_command(socket_name);
        option_cmd
            .arg(command)
            .arg("-t")
            .arg(target)
            .arg(key)
            .arg(value);
        if let Err(err) = run_tmux_checked(&mut option_cmd, key) {
            tracing::debug!(
                socket = socket_name,
                session = session_name,
                option = key,
                error = %err,
                "Ignoring non-fatal tmux option failure"
            );
        }
    }
    Ok(())
}

fn cleanup_tmux_session(ctx: &TmuxContext) {
    let mut cmd = tmux_base_command(&ctx.socket_name);
    cmd.arg("kill-server");
    if let Err(err) = run_tmux_checked(&mut cmd, "kill-server") {
        tracing::debug!(
            socket = %ctx.socket_name,
            session = %ctx.session_name,
            error = %err,
            "Best-effort tmux cleanup failed"
        );
    }
}

fn resolve_resize_reason(cols: u16, rows: u16, reason: Option<&str>) -> PtyResizeReason {
    if cols == 0 || rows == 0 {
        return PtyResizeReason::DetachRestore;
    }

    match reason {
        Some("attach_init") => PtyResizeReason::AttachInit,
        Some("geometry_change") => PtyResizeReason::GeometryChange,
        Some("reconnect_sync") => PtyResizeReason::ReconnectSync,
        Some("keyboard_overlay") => PtyResizeReason::KeyboardOverlay,
        Some("detach_restore") => PtyResizeReason::DetachRestore,
        _ => PtyResizeReason::GeometryChange,
    }
}

/// Normalize newline input sequences before writing into the PTY.
///
/// Different environments (raw/canonical mode, ICRNL, etc.) may send Enter as
/// `\r` or `\n`. Converting any newline variant (LF/CR/CRLF) to `\r` keeps PTY
/// input behavior consistent and avoids "line feed without carriage return"
/// artifacts in some shells.
fn normalize_input_newlines(input: &[u8]) -> Cow<'_, [u8]> {
    if !input.contains(&b'\r') && !input.contains(&b'\n') {
        return Cow::Borrowed(input);
    }

    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == b'\r' {
            // If CRLF, collapse to a single CR.
            if i + 1 < input.len() && input[i + 1] == b'\n' {
                out.push(b'\r');
                i += 2;
                continue;
            }
            out.push(b'\r');
            i += 1;
            continue;
        }
        if b == b'\n' {
            out.push(b'\r');
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    Cow::Owned(out)
}

/// Run a command wrapped with mobile streaming via daemon
pub async fn run_wrapped(config: WrapConfig) -> Result<i32, WrapError> {
    // Resolve the command path
    let cmd_path = resolve_command(&config.command)
        .ok_or_else(|| WrapError::CommandNotFound(config.command.clone()))?;
    let runtime_mode = resolve_runtime_mode();
    let tmux_mouse_mode = resolve_tmux_mouse_mode();
    let tmux_mouse_source = match std::env::var("MOBILECLI_TMUX_MOUSE") {
        Ok(raw) if parse_tmux_mouse_mode(&raw).is_some() => "env_override",
        Ok(raw) if raw.trim().is_empty() => "platform_default",
        Ok(_) => "platform_default_invalid_env",
        Err(_) => "platform_default",
    };

    // Generate session ID (12 chars for better collision resistance)
    let session_id = uuid::Uuid::new_v4().to_string()[..12].to_string();

    // Get current working directory (allow override)
    let cwd = config.working_dir.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    // Connect to daemon (use actual port from file, fallback to default)
    let port = get_port().unwrap_or(DEFAULT_PORT);
    let daemon_url = format!("ws://127.0.0.1:{}", port);
    tracing::info!("Connecting to daemon at {}", daemon_url);
    
    let (ws_stream, _) = connect_async(&daemon_url)
        .await
        .map_err(|e| {
            tracing::error!("Failed to connect to daemon at {}: {}", daemon_url, e);
            WrapError::DaemonConnection(format!("Failed to connect to daemon: {}", e))
        })?;
    tracing::info!("WebSocket connected to daemon");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Register with daemon as a PTY session
    let register_msg = serde_json::json!({
        "type": "register_pty",
        "session_id": session_id,
        "name": config.session_name,
        "command": config.command,
        "project_path": cwd,
        "runtime": runtime_mode.as_str(),
    });
    tracing::info!("Sending registration message: {}", register_msg);
    
    ws_tx
        .send(Message::Text(register_msg.to_string()))
        .await
        .map_err(|e| {
            tracing::error!("Failed to send registration message: {}", e);
            WrapError::DaemonConnection(format!("Failed to register: {}", e))
        })?;
    tracing::info!("Registration message sent, waiting for acknowledgment");

    // Wait for registration acknowledgment
    match ws_rx.next().await {
        Some(Ok(Message::Text(text))) => {
            tracing::info!("Received response from daemon: {}", text);
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                if msg["type"].as_str() != Some("registered") {
                    tracing::error!("Unexpected response type from daemon: {:?}", msg["type"]);
                    return Err(WrapError::DaemonConnection(
                        "Unexpected response from daemon".to_string(),
                    ));
                }
                tracing::info!("Successfully registered with daemon");
            } else {
                tracing::error!("Failed to parse daemon response");
            }
        }
        Some(Err(e)) => {
            tracing::error!("WebSocket error while waiting for registration: {}", e);
            return Err(WrapError::DaemonConnection(format!("WebSocket error: {}", e)));
        }
        None => {
            tracing::error!("WebSocket closed before registration response");
            return Err(WrapError::DaemonConnection("WebSocket closed unexpectedly".to_string()));
        }
        _ => {
            tracing::warn!("Received non-text WebSocket message, ignoring");
            return Err(WrapError::DaemonConnection("Unexpected message type from daemon".to_string()));
        }
    }

    if !config.quiet {
        println!(
            "{} {} {} {}",
            "📱".green(),
            "Connected!".green().bold(),
            format!(
                "Session '{}' is now visible on your phone",
                config.session_name
            )
            .dimmed(),
            format!("[runtime:{}]", runtime_mode.as_str()).dimmed()
        );
    }

    // Create PTY
    let pty_system = native_pty_system();
    let (cols, rows) = get_terminal_size();

    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| WrapError::Pty(e.to_string()))?;

    let mut tmux_context: Option<TmuxContext> = None;
    if runtime_mode == RuntimeMode::Tmux {
        tracing::info!(
            mode = tmux_mouse_mode.as_tmux_value(),
            source = tmux_mouse_source,
            "Resolved tmux mouse policy"
        );
        let token = sanitize_tmux_token(&session_id);
        let ctx = TmuxContext {
            socket_name: format!("mcli-{}", token),
            session_name: format!("mcli-{}", token),
        };
        setup_tmux_session(
            &ctx.socket_name,
            &ctx.session_name,
            &cmd_path,
            &config.args,
            &cwd,
            cols,
            rows,
            tmux_mouse_mode,
        )?;
        tmux_context = Some(ctx);
    }

    // Build command
    let mut cmd = if let Some(ctx) = &tmux_context {
        let mut c = CommandBuilder::new("tmux");
        #[cfg(windows)]
        let null_dev = "NUL";
        #[cfg(not(windows))]
        let null_dev = "/dev/null";
        
        c.args([
            "-L",
            ctx.socket_name.as_str(),
            "-f",
            null_dev,
            "attach-session",
            "-t",
            ctx.session_name.as_str(),
        ]);
        c
    } else {
        let mut c = CommandBuilder::new(&cmd_path);
        c.args(&config.args);
        c
    };
    cmd.cwd(&cwd);

    // Set up environment for interactive shell
    cmd.env(
        "TERM",
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()),
    );

    // Prevent recursive auto-launch when shell hook is installed
    cmd.env("MOBILECLI_SESSION", "1");

    // Spawn the command
    tracing::info!("Spawning PTY command: {} {:?}", cmd_path, config.args);
    
    #[cfg(windows)]
    {
        // On Windows, try to hide the console window for a cleaner experience
        // Note: portable-pty doesn't expose creation_flags directly, so we rely on
        // the PTY itself being created properly
        tracing::info!("Spawning on Windows - PTY size: {}x{}", cols, rows);
    }
    
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
        tracing::error!("Failed to spawn PTY command: {}", e);
        if let Some(ctx) = &tmux_context {
            cleanup_tmux_session(ctx);
        }
        WrapError::Pty(e.to_string())
    })?;
    
    tracing::info!("PTY command spawned successfully, PID: {:?}", child.process_id());

    // Drop the slave - we communicate through the master
    drop(pair.slave);

    // Get master reader/writer
    let master = pair.master;
    let mut reader = master
        .try_clone_reader()
        .map_err(|e| WrapError::Pty(e.to_string()))?;
    let mut writer = master
        .take_writer()
        .map_err(|e| WrapError::Pty(e.to_string()))?;

    // Flag to signal shutdown
    let running = Arc::new(AtomicBool::new(true));
    let running_reader = running.clone();

    // Channel for PTY output
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Spawn thread to read from PTY
    let reader_handle = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while running_reader.load(Ordering::SeqCst) {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let _ = output_tx.send(buf[..n].to_vec());
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::Interrupted {
                        break;
                    }
                }
            }
        }
    });

    // Set up Ctrl+C handler
    let running_ctrlc = running.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        running_ctrlc.store(false, Ordering::SeqCst);
    }) {
        tracing::warn!(
            "Failed to set Ctrl+C handler: {}. Graceful shutdown may not work.",
            e
        );
    }

    // Set up stdin reading (for local terminal input)
    let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let running_stdin = running.clone();

    // Configure terminal for raw mode (Unix only - Windows uses different mechanism)
    // NOTE: Raw mode is disabled by default to preserve terminal selection/copy behavior.
    // When raw mode is enabled, terminal emulators cannot handle text selection properly.
    // Input will be line-buffered instead (press Enter to send to PTY).
    // To enable raw mode, set MOBILECLI_RAW_MODE=1 environment variable.
    #[cfg(unix)]
    let original_termios = if resolve_raw_mode() {
        setup_raw_mode()
    } else {
        tracing::info!("Running in line-buffered mode (set MOBILECLI_RAW_MODE=1 for raw mode)");
        None
    };
    #[cfg(not(unix))]
    let original_termios: Option<()> = None;
    
    // On Windows, ensure console handles ANSI codes
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let handle = std::io::stdout().as_raw_handle();
            const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
            let mut mode: u32 = 0;
            
            type GetConsoleModeFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut u32) -> i32;
            type SetConsoleModeFn = unsafe extern "system" fn(*mut std::ffi::c_void, u32) -> i32;
            
            let kernel32 = winapi::um::libloaderapi::GetModuleHandleA(b"kernel32.dll\0".as_ptr() as *const i8);
            if !kernel32.is_null() {
                let get_mode: GetConsoleModeFn = std::mem::transmute(
                    winapi::um::libloaderapi::GetProcAddress(kernel32, b"GetConsoleMode\0".as_ptr() as *const i8)
                );
                let set_mode: SetConsoleModeFn = std::mem::transmute(
                    winapi::um::libloaderapi::GetProcAddress(kernel32, b"SetConsoleMode\0".as_ptr() as *const i8)
                );
                
                if get_mode(handle as *mut _, &mut mode) != 0 {
                    let _ = set_mode(handle as *mut _, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                    tracing::info!("Enabled Windows virtual terminal processing");
                }
            }
        }
    }

    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 1024];
        while running_stdin.load(Ordering::SeqCst) {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = stdin_tx.send(buf[..n].to_vec());
                }
                Err(_) => break,
            }
        }
    });

    // Main event loop
    let mut stdout = std::io::stdout();
    let desktop_resize_policy = DesktopResizePolicy::from_env();
    let mut saved_local_size: Option<(u16, u16)> = None;
    let mut last_applied_pty_size: Option<(u16, u16)> = Some((cols, rows));
    let mut exit_code: i32 = 0;

    loop {
        tokio::select! {
            // PTY output
            Some(data) = output_rx.recv() => {
                // Always write to local terminal output.
                if let Err(e) = stdout.write_all(&data) {
                    tracing::error!("Failed to write PTY output to stdout: {}", e);
                }
                if let Err(e) = stdout.flush() {
                    tracing::error!("Failed to flush stdout: {}", e);
                }

                // Send to daemon
                let msg = serde_json::json!({
                    "type": "pty_output",
                    "data": BASE64.encode(&data),
                });
                if ws_tx.send(Message::Text(msg.to_string())).await.is_err() {
                    tracing::debug!("Failed to send PTY output to daemon");
                }
            }

            // Local stdin input
            Some(input) = stdin_rx.recv() => {
                let normalized = normalize_input_newlines(&input);
                if let Err(e) = writer.write_all(normalized.as_ref()) {
                    tracing::debug!("Failed to write stdin to PTY: {}", e);
                }
                let _ = writer.flush();
            }

            // Messages from daemon (input/resize from mobile)
            result = ws_rx.next() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                            match msg["type"].as_str() {
                                Some("input") => {
                                    if let Some(data) = msg["data"].as_str() {
                                        if let Ok(bytes) = BASE64.decode(data) {
                                            let normalized = normalize_input_newlines(&bytes);
                                            tracing::debug!(
                                                "Writing mobile input to PTY: {} bytes",
                                                normalized.len()
                                            );
                                            if let Err(e) = writer.write_all(normalized.as_ref()) {
                                                tracing::error!("Failed to write mobile input to PTY: {}", e);
                                            } else {
                                                tracing::debug!("Successfully wrote input to PTY");
                                            }
                                            if let Err(e) = writer.flush() {
                                                tracing::error!("Failed to flush PTY writer: {}", e);
                                            }
                                        } else {
                                            tracing::warn!("Failed to base64 decode input data");
                                        }
                                    }
                                }
                                Some("resize") => {
                                    if let (Some(cols), Some(rows)) = (
                                        msg["cols"].as_u64(),
                                        msg["rows"].as_u64(),
                                    ) {
                                        let mut c = cols.min(u16::MAX as u64) as u16;
                                        let mut r = rows.min(u16::MAX as u64) as u16;
                                        let epoch = msg["epoch"].as_u64();
                                        let reason =
                                            resolve_resize_reason(c, r, msg["reason"].as_str());
                                        tracing::debug!(
                                            session_id = %session_id,
                                            cols = c,
                                            rows = r,
                                            epoch = ?epoch,
                                            reason = reason.as_str(),
                                            policy = ?desktop_resize_policy,
                                            "Wrapper resize request"
                                        );

                                        if reason == PtyResizeReason::KeyboardOverlay {
                                            let (ack_c, ack_r) = last_applied_pty_size.unwrap_or((c, r));
                                            tracing::debug!(
                                                session_id = %session_id,
                                                cols = c,
                                                rows = r,
                                                epoch = ?epoch,
                                                reason = reason.as_str(),
                                                decision = "ignored_keyboard_overlay",
                                                "Ignoring keyboard-only resize in wrapper"
                                            );

                                            let mut resized_msg = serde_json::json!({
                                                "type": "pty_resized",
                                                "cols": ack_c,
                                                "rows": ack_r,
                                            });
                                            if let Some(epoch) = epoch {
                                                resized_msg["epoch"] = serde_json::json!(epoch);
                                            }
                                            let _ = ws_tx.send(Message::Text(resized_msg.to_string())).await;
                                            continue;
                                        }

                                        if reason == PtyResizeReason::DetachRestore {
                                            // Mobile detached. Restore PTY dimensions to the host
                                            // terminal's pre-mobile size when available. In mirror
                                            // mode, also restore the host window.
                                            let (restore_c, restore_r) = match desktop_resize_policy {
                                                DesktopResizePolicy::Mirror => {
                                                    if let Some((lc, lr)) = saved_local_size.take() {
                                                        request_terminal_resize(lc, lr);
                                                        (lc, lr)
                                                    } else {
                                                        get_terminal_size()
                                                    }
                                                }
                                                DesktopResizePolicy::Preserve => {
                                                    if let Some((lc, lr)) = saved_local_size.take() {
                                                        (lc, lr)
                                                    } else {
                                                        get_terminal_size()
                                                    }
                                                }
                                            };
                                            c = restore_c;
                                            r = restore_r;
                                        } else {
                                            // Mobile active: always resize child PTY to mobile
                                            // dimensions. Host terminal mirroring is the default;
                                            // set MOBILECLI_DESKTOP_RESIZE_POLICY=preserve to opt out.
                                            if saved_local_size.is_none() {
                                                saved_local_size = get_terminal_size_opt();
                                            }
                                            if desktop_resize_policy == DesktopResizePolicy::Mirror {
                                                request_terminal_resize(c, r);
                                            }
                                        }

                                        let should_resize = last_applied_pty_size != Some((c, r));
                                        if should_resize {
                                            let resized = master.resize(PtySize {
                                                rows: r,
                                                cols: c,
                                                pixel_width: 0,
                                                pixel_height: 0,
                                            });
                                            if resized.is_ok() {
                                                last_applied_pty_size = Some((c, r));
                                                tracing::debug!(
                                                    session_id = %session_id,
                                                    cols = c,
                                                    rows = r,
                                                    epoch = ?epoch,
                                                    reason = reason.as_str(),
                                                    decision = "applied",
                                                    "Applied PTY resize"
                                                );
                                            } else {
                                                tracing::warn!(
                                                    session_id = %session_id,
                                                    cols = c,
                                                    rows = r,
                                                    epoch = ?epoch,
                                                    reason = reason.as_str(),
                                                    decision = "apply_failed",
                                                    "Failed to apply PTY resize"
                                                );
                                                continue;
                                            }
                                        } else {
                                            tracing::debug!(
                                                session_id = %session_id,
                                                cols = c,
                                                rows = r,
                                                epoch = ?epoch,
                                                reason = reason.as_str(),
                                                decision = "noop",
                                                "Skipping no-op PTY resize"
                                            );
                                        }

                                        let mut resized_msg = serde_json::json!({
                                            "type": "pty_resized",
                                            "cols": c,
                                            "rows": r,
                                        });
                                        if let Some(epoch) = epoch {
                                            resized_msg["epoch"] = serde_json::json!(epoch);
                                        }
                                        let _ = ws_tx.send(Message::Text(resized_msg.to_string())).await;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!("Daemon connection closed");
                        break;
                    }
                    _ => {}
                }
            }

            // Check if child exited
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                if let Ok(Some(status)) = child.try_wait() {
                    // Cap exit code at 255 to prevent i32 overflow
                    exit_code = status.exit_code().min(255) as i32;
                    break;
                }
                if !running.load(Ordering::SeqCst) {
                    // Ctrl+C pressed, kill child
                    let _ = child.kill();
                    exit_code = 130; // Standard exit code for SIGINT
                    break;
                }
            }
        }
    }

    // Restore terminal mode
    restore_terminal_mode(original_termios);

    // Cleanup
    running.store(false, Ordering::SeqCst);

    // Notify daemon that the session ended (so mobile closes it promptly)
    let msg = serde_json::json!({
        "type": "session_ended",
        "exit_code": exit_code,
    });
    let _ = ws_tx.send(Message::Text(msg.to_string())).await;

    // Close WebSocket
    let _ = ws_tx.close().await;

    // Wait for reader thread
    let _ = reader_handle.join();

    if let Some(ctx) = &tmux_context {
        cleanup_tmux_session(ctx);
    }

    // Print exit message
    println!();
    if exit_code == 0 {
        println!("{} Session ended", "✓".green());
    } else if exit_code == 130 {
        println!("{} Session interrupted", "•".yellow());
    } else {
        println!("{} Session ended with code {}", "✗".red(), exit_code);
    }

    Ok(exit_code)
}

/// Set up raw terminal mode for proper input handling
#[cfg(unix)]
fn setup_raw_mode() -> Option<nix::sys::termios::Termios> {
    use nix::sys::termios::{self, LocalFlags, SetArg};
    use std::os::fd::AsFd;

    let stdin = std::io::stdin();

    if let Ok(original) = termios::tcgetattr(stdin.as_fd()) {
        let mut raw = original.clone();
        // Disable canonical mode and echo
        raw.local_flags.remove(LocalFlags::ICANON);
        raw.local_flags.remove(LocalFlags::ECHO);
        raw.local_flags.remove(LocalFlags::ISIG);

        if termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw).is_ok() {
            return Some(original);
        }
    }
    None
}

#[cfg(not(unix))]
fn setup_raw_mode() -> Option<()> {
    None
}

/// Restore terminal mode
#[cfg(unix)]
fn restore_terminal_mode(original: Option<nix::sys::termios::Termios>) {
    use nix::sys::termios::{self, SetArg};
    use std::os::fd::AsFd;

    if let Some(termios_settings) = original {
        let stdin = std::io::stdin();
        let _ = termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &termios_settings);
    }
}

#[cfg(not(unix))]
fn restore_terminal_mode(_original: Option<()>) {}

#[cfg(test)]
mod tests {
    use super::{
        cleanup_tmux_session, parse_bool_env_flag, parse_tmux_mouse_mode, resolve_resize_reason,
        resolve_runtime_mode, sanitize_tmux_token, setup_tmux_session, tmux_base_command,
        RuntimeMode, TmuxContext, TmuxMouseMode,
    };
    use crate::protocol::PtyResizeReason;

    #[test]
    fn wrapper_reason_resolves_restore_from_zero_dimensions() {
        assert_eq!(
            resolve_resize_reason(0, 24, Some("geometry_change")),
            PtyResizeReason::DetachRestore
        );
        assert_eq!(
            resolve_resize_reason(80, 0, Some("attach_init")),
            PtyResizeReason::DetachRestore
        );
    }

    #[test]
    fn wrapper_reason_maps_known_and_defaults_unknown() {
        assert_eq!(
            resolve_resize_reason(80, 24, Some("attach_init")),
            PtyResizeReason::AttachInit
        );
        assert_eq!(
            resolve_resize_reason(80, 24, Some("keyboard_overlay")),
            PtyResizeReason::KeyboardOverlay
        );
        assert_eq!(
            resolve_resize_reason(80, 24, Some("detach_restore")),
            PtyResizeReason::DetachRestore
        );
        assert_eq!(
            resolve_resize_reason(80, 24, Some("future_reason")),
            PtyResizeReason::GeometryChange
        );
    }

    #[test]
    fn tmux_token_sanitizes_non_identifier_chars() {
        assert_eq!(sanitize_tmux_token("abc123"), "abc123");
        assert_eq!(sanitize_tmux_token("abc/def"), "abc-def");
        assert_eq!(sanitize_tmux_token(""), "session");
    }

    #[test]
    fn runtime_mode_honors_explicit_pty_override() {
        std::env::set_var("MOBILECLI_RUNTIME", "pty");
        assert_eq!(resolve_runtime_mode(), RuntimeMode::Pty);
        std::env::remove_var("MOBILECLI_RUNTIME");
    }

    #[test]
    fn tmux_mouse_mode_parser_accepts_common_aliases() {
        assert_eq!(parse_tmux_mouse_mode("on"), Some(TmuxMouseMode::On));
        assert_eq!(parse_tmux_mouse_mode("true"), Some(TmuxMouseMode::On));
        assert_eq!(parse_tmux_mouse_mode("1"), Some(TmuxMouseMode::On));
        assert_eq!(parse_tmux_mouse_mode("yes"), Some(TmuxMouseMode::On));
        assert_eq!(parse_tmux_mouse_mode("off"), Some(TmuxMouseMode::Off));
        assert_eq!(parse_tmux_mouse_mode("false"), Some(TmuxMouseMode::Off));
        assert_eq!(parse_tmux_mouse_mode("0"), Some(TmuxMouseMode::Off));
        assert_eq!(parse_tmux_mouse_mode("no"), Some(TmuxMouseMode::Off));
        assert_eq!(parse_tmux_mouse_mode("maybe"), None);
    }

    #[test]
    fn tmux_mouse_mode_default_matches_platform_policy() {
        #[cfg(target_os = "linux")]
        assert_eq!(TmuxMouseMode::default_for_platform(), TmuxMouseMode::Off);

        #[cfg(not(target_os = "linux"))]
        assert_eq!(TmuxMouseMode::default_for_platform(), TmuxMouseMode::On);
    }

    #[test]
    fn bool_flag_parser_accepts_common_aliases() {
        assert_eq!(parse_bool_env_flag("on"), Some(true));
        assert_eq!(parse_bool_env_flag("true"), Some(true));
        assert_eq!(parse_bool_env_flag("1"), Some(true));
        assert_eq!(parse_bool_env_flag("yes"), Some(true));

        assert_eq!(parse_bool_env_flag("off"), Some(false));
        assert_eq!(parse_bool_env_flag("false"), Some(false));
        assert_eq!(parse_bool_env_flag("0"), Some(false));
        assert_eq!(parse_bool_env_flag("no"), Some(false));

        assert_eq!(parse_bool_env_flag(""), None);
        assert_eq!(parse_bool_env_flag("maybe"), None);
    }

    #[test]
    fn tmux_session_bootstrap_and_cleanup_roundtrip() {
        if which::which("tmux").is_err() {
            return;
        }

        let token = sanitize_tmux_token(&uuid::Uuid::new_v4().to_string()[..12]);
        let ctx = TmuxContext {
            socket_name: format!("mcli-test-{}", token),
            session_name: format!("mcli-test-{}", token),
        };

        setup_tmux_session(
            &ctx.socket_name,
            &ctx.session_name,
            "/bin/sh",
            // Keep the session alive long enough for CI has-session checks.
            &vec!["-lc".to_string(), "sleep 10 & wait".to_string()],
            ".",
            80,
            24,
            TmuxMouseMode::default_for_platform(),
        )
        .expect("setup tmux session");

        let mut has_session = tmux_base_command(&ctx.socket_name);
        let output = has_session
            .arg("has-session")
            .arg("-t")
            .arg(&ctx.session_name)
            .output()
            .expect("has-session output");
        assert!(
            output.status.success(),
            "expected has-session success: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let mut show_window_size = tmux_base_command(&ctx.socket_name);
        let output = show_window_size
            .arg("show-window-options")
            .arg("-v")
            .arg("-t")
            .arg(format!("{}:0", &ctx.session_name))
            .arg("window-size")
            .output()
            .expect("show-window-options output");
        assert!(
            output.status.success(),
            "expected show-window-options success: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let window_size_mode = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            window_size_mode, "latest",
            "expected tmux window-size to remain dynamic for client-driven resize propagation"
        );

        let mut show_alt_screen = tmux_base_command(&ctx.socket_name);
        let output = show_alt_screen
            .arg("show-window-options")
            .arg("-v")
            .arg("-t")
            .arg(format!("{}:0", &ctx.session_name))
            .arg("alternate-screen")
            .output()
            .expect("show-window-options alternate-screen output");
        assert!(
            output.status.success(),
            "expected alternate-screen query success: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let alt_screen_mode = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            alt_screen_mode, "off",
            "expected tmux alternate-screen to be disabled to protect host terminal scrollback"
        );

        let mut show_mouse_mode = tmux_base_command(&ctx.socket_name);
        let output = show_mouse_mode
            .arg("show-options")
            .arg("-gv")
            .arg("mouse")
            .output()
            .expect("show-options mouse output");
        assert!(
            output.status.success(),
            "expected mouse option query success: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mouse_mode = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            mouse_mode,
            TmuxMouseMode::default_for_platform().as_tmux_value(),
            "expected tmux mouse option to follow platform default policy"
        );

        cleanup_tmux_session(&ctx);

        let mut has_after_cleanup = tmux_base_command(&ctx.socket_name);
        let output = has_after_cleanup
            .arg("has-session")
            .arg("-t")
            .arg(&ctx.session_name)
            .output()
            .expect("has-session output");
        assert!(
            !output.status.success(),
            "expected tmux session cleanup to remove server/session"
        );
    }

    #[test]
    fn tmux_session_respects_explicit_mouse_override() {
        if which::which("tmux").is_err() {
            return;
        }

        let token = sanitize_tmux_token(&uuid::Uuid::new_v4().to_string()[..12]);
        let ctx = TmuxContext {
            socket_name: format!("mcli-test-{}", token),
            session_name: format!("mcli-test-{}", token),
        };

        setup_tmux_session(
            &ctx.socket_name,
            &ctx.session_name,
            "/bin/sh",
            &vec!["-lc".to_string(), "sleep 10 & wait".to_string()],
            ".",
            80,
            24,
            TmuxMouseMode::On,
        )
        .expect("setup tmux session with mouse override");

        let mut show_mouse_mode = tmux_base_command(&ctx.socket_name);
        let output = show_mouse_mode
            .arg("show-options")
            .arg("-gv")
            .arg("mouse")
            .output()
            .expect("show-options mouse output");
        assert!(
            output.status.success(),
            "expected mouse option query success: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mouse_mode = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            mouse_mode, "on",
            "expected explicit tmux mouse override to enable mouse mode"
        );

        cleanup_tmux_session(&ctx);
    }
}
