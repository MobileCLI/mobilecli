//! PTY wrapper - spawns commands in a PTY and streams to the daemon
//!
//! This module:
//! 1. Spawns the target command (or shell) in a PTY we control
//! 2. Connects to the daemon via WebSocket
//! 3. Streams PTY output to both local terminal AND daemon
//! 4. Relays input from daemon (mobile) to the PTY
//! 5. Handles terminal resize events

use crate::daemon::{get_port, DEFAULT_PORT};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use colored::Colorize;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{IsTerminal, Read, Write};
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

/// Run a command wrapped with mobile streaming via daemon
pub async fn run_wrapped(config: WrapConfig) -> Result<i32, WrapError> {
    // Resolve the command path
    let cmd_path = resolve_command(&config.command)
        .ok_or_else(|| WrapError::CommandNotFound(config.command.clone()))?;

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
            "{} {} {}",
            "📱".green(),
            "Connected!".green().bold(),
            format!(
                "Session '{}' is now visible on your phone",
                config.session_name
            )
            .dimmed()
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

    // Build command
    let mut cmd = CommandBuilder::new(&cmd_path);
    cmd.args(&config.args);
    cmd.cwd(&cwd);

    // Set up environment for interactive shell
    cmd.env(
        "TERM",
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()),
    );

    // Spawn the command
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| WrapError::Pty(e.to_string()))?;

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
    let mut saved_local_size: Option<(u16, u16)> = None;
    let mut exit_code: i32 = 0;

    loop {
        tokio::select! {
            // PTY output
            Some(data) = output_rx.recv() => {
                // Write to local terminal
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
                if let Err(e) = writer.write_all(&input) {
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
                                            if let Err(e) = writer.write_all(&bytes) {
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
                                        let (cols, rows) = if cols == 0 || rows == 0 {
                                            // Mobile disconnected: restore local size if we captured it.
                                            if let Some((local_cols, local_rows)) = saved_local_size.take() {
                                                request_terminal_resize(local_cols, local_rows);
                                                (local_cols, local_rows)
                                            } else {
                                                get_terminal_size()
                                            }
                                        } else {
                                            // Mobile active: resize PTY to mobile dimensions and request local terminal resize
                                            if saved_local_size.is_none() {
                                                saved_local_size = get_terminal_size_opt();
                                            }
                                            let cols = cols as u16;
                                            let rows = rows as u16;
                                            request_terminal_resize(cols, rows);
                                            (cols, rows)
                                        };
                                        let _ = master.resize(PtySize {
                                            rows,
                                            cols,
                                            pixel_width: 0,
                                            pixel_height: 0,
                                        });
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
