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
    cmd.arg("-L")
        .arg(socket_name)
        .arg("-f")
        .arg("/dev/null")
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

fn jitter_resize_target(cols: u16, rows: u16) -> (u16, u16) {
    let jitter_cols = if cols > 1 { cols - 1 } else { cols + 1 };
    let jitter_rows = if rows > 1 { rows - 1 } else { rows + 1 };
    (jitter_cols, jitter_rows)
}

fn sanitize_tmux_token(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() {
        out.push_str("session");
    }
    out
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

fn setup_tmux_session(
    socket_name: &str,
    session_name: &str,
    command_path: &str,
    args: &[String],
    cwd: &str,
    cols: u16,
    rows: u16,
) -> Result<(), WrapError> {
    // Bootstrap a dedicated tmux server first so global options apply before
    // the wrapped CLI starts. This avoids races where frame CLIs enter
    // alternate-screen at launch and drop scrollback history.
    let mut start_server = tmux_base_command(socket_name);
    start_server.arg("start-server");
    let _ = run_tmux_checked(&mut start_server, "start-server");

    let mut pre_alt_screen = tmux_base_command(socket_name);
    pre_alt_screen
        .arg("set-window-option")
        .arg("-g")
        .arg("alternate-screen")
        .arg("off");
    if let Err(err) = run_tmux_checked(&mut pre_alt_screen, "alternate-screen") {
        tracing::debug!(
            socket = socket_name,
            session = session_name,
            error = %err,
            "Ignoring non-fatal pre-session alternate-screen option failure"
        );
    }

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

    // Best-effort options for deterministic rendering behavior.
    // NOTE: window-size must remain dynamic so wrapper PTY resizes propagate
    // into tmux panes when mobile dimensions change.
    let window_target = format!("{}:0", session_name);
    let option_sets: [(&str, &str, &str, &str); 5] = [
        ("set-option", session_name, "status", "off"),
        ("set-option", session_name, "allow-rename", "off"),
        ("set-option", session_name, "history-limit", "200000"),
        ("set-window-option", &window_target, "alternate-screen", "off"),
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
    let (ws_stream, _) = connect_async(&daemon_url)
        .await
        .map_err(|e| WrapError::DaemonConnection(format!("Failed to connect to daemon: {}", e)))?;

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
    ws_tx
        .send(Message::Text(register_msg.to_string()))
        .await
        .map_err(|e| WrapError::DaemonConnection(format!("Failed to register: {}", e)))?;

    // Wait for registration acknowledgment
    if let Some(Ok(Message::Text(text))) = ws_rx.next().await {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
            if msg["type"].as_str() != Some("registered") {
                return Err(WrapError::DaemonConnection(
                    "Unexpected response from daemon".to_string(),
                ));
            }
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
        )?;
        tmux_context = Some(ctx);
    }

    // Build command
    let mut cmd = if let Some(ctx) = &tmux_context {
        let mut c = CommandBuilder::new("tmux");
        c.args([
            "-L",
            ctx.socket_name.as_str(),
            "-f",
            "/dev/null",
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
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
        if let Some(ctx) = &tmux_context {
            cleanup_tmux_session(ctx);
        }
        WrapError::Pty(e.to_string())
    })?;

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

    // Configure terminal for raw mode
    let original_termios = setup_raw_mode();
    if original_termios.is_none() {
        tracing::warn!("Failed to set raw terminal mode. Input may be line-buffered.");
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
                let _ = stdout.write_all(&data);
                let _ = stdout.flush();

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
                                            if let Err(e) = writer.write_all(normalized.as_ref()) {
                                                tracing::debug!("Failed to write mobile input to PTY: {}", e);
                                            }
                                            let _ = writer.flush();
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
                                            // dimensions. Host terminal mirroring is opt-in via
                                            // MOBILECLI_DESKTOP_RESIZE_POLICY=mirror.
                                            if saved_local_size.is_none() {
                                                saved_local_size = get_terminal_size_opt();
                                            }
                                            if desktop_resize_policy == DesktopResizePolicy::Mirror {
                                                request_terminal_resize(c, r);
                                            }
                                        }

                                        let force_noop_refresh = runtime_mode == RuntimeMode::Pty
                                            && matches!(
                                                reason,
                                                PtyResizeReason::AttachInit
                                                    | PtyResizeReason::ReconnectSync
                                            )
                                            && last_applied_pty_size == Some((c, r));

                                        let should_resize =
                                            force_noop_refresh || last_applied_pty_size != Some((c, r));
                                        if should_resize {
                                            if force_noop_refresh {
                                                let (jitter_c, jitter_r) = jitter_resize_target(c, r);
                                                tracing::debug!(
                                                    session_id = %session_id,
                                                    cols = c,
                                                    rows = r,
                                                    jitter_cols = jitter_c,
                                                    jitter_rows = jitter_r,
                                                    epoch = ?epoch,
                                                    reason = reason.as_str(),
                                                    decision = "force_noop_redraw",
                                                    "Applying jitter resize before final target to force redraw"
                                                );
                                                let _ = master.resize(PtySize {
                                                    rows: jitter_r,
                                                    cols: jitter_c,
                                                    pixel_width: 0,
                                                    pixel_height: 0,
                                                });
                                            }

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
        cleanup_tmux_session, jitter_resize_target, resolve_resize_reason, resolve_runtime_mode,
        sanitize_tmux_token, setup_tmux_session, tmux_base_command, RuntimeMode, TmuxContext,
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
    fn jitter_target_always_changes_dimensions() {
        assert_eq!(jitter_resize_target(80, 24), (79, 23));
        assert_eq!(jitter_resize_target(1, 1), (2, 2));
    }

    #[test]
    fn runtime_mode_honors_explicit_pty_override() {
        std::env::set_var("MOBILECLI_RUNTIME", "pty");
        assert_eq!(resolve_runtime_mode(), RuntimeMode::Pty);
        std::env::remove_var("MOBILECLI_RUNTIME");
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
            "expected tmux alternate-screen to be disabled so scrollback is preserved"
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
}
