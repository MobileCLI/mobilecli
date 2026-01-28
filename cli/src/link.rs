//! Link command - attach to an existing session
//!
//! Similar to `screen -x` or `tmux attach` - joins an existing PTY session.

use crate::daemon;
use crate::protocol::{ClientMessage, ServerMessage, SessionListItem};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use colored::Colorize;
use futures_util::{SinkExt, StreamExt};
use std::io::{self, Read, Write};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Run the link command
pub async fn run(session_id: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    // Ensure daemon is running
    if !daemon::is_running() {
        return Err("Daemon is not running. Start a session with 'mobilecli' first.".into());
    }

    let port = daemon::get_port().unwrap_or(daemon::DEFAULT_PORT);
    let ws_url = format!("ws://127.0.0.1:{}", port);

    // Connect to daemon to get session list
    let (mut ws, _) = connect_async(&ws_url).await?;

    // Send hello
    let hello = ClientMessage::Hello {
        auth_token: None,
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    ws.send(Message::Text(serde_json::to_string(&hello)?))
        .await?;

    // Wait for welcome and sessions list
    let mut sessions: Vec<SessionListItem> = Vec::new();

    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(text) => {
                if let Ok(server_msg) = serde_json::from_str::<ServerMessage>(&text) {
                    match server_msg {
                        ServerMessage::Welcome { .. } => continue,
                        ServerMessage::Sessions { sessions: s } => {
                            sessions = s;
                            break;
                        }
                        _ => continue,
                    }
                }
            }
            _ => continue,
        }
    }

    // Close initial connection
    let _ = ws.close(None).await;

    if sessions.is_empty() {
        println!("{}", "No active sessions to link to.".yellow());
        println!("Start a session with {} first.", "mobilecli".cyan());
        return Ok(());
    }

    // Find session to link to
    let session = if let Some(ref id_or_name) = session_id {
        // Try to find by ID prefix or name
        sessions.iter().find(|s| {
            s.session_id.starts_with(id_or_name)
                || s.name.to_lowercase().contains(&id_or_name.to_lowercase())
        })
    } else if sessions.len() == 1 {
        // Auto-select if only one session
        sessions.first()
    } else {
        // Interactive picker
        let session_refs: Vec<&SessionListItem> = sessions.iter().collect();
        show_session_picker(&session_refs)?
    };

    let session = match session {
        Some(s) => s.clone(),
        None => {
            println!("{}", "No session selected.".dimmed());
            return Ok(());
        }
    };

    println!(
        "{} Linking to {} ({})",
        "→".cyan(),
        session.name.bold(),
        session.session_id[..8.min(session.session_id.len())].dimmed()
    );

    // Run linked mode
    run_linked_mode(&ws_url, &session).await
}

/// Interactive session picker
fn show_session_picker<'a>(
    sessions: &[&'a SessionListItem],
) -> io::Result<Option<&'a SessionListItem>> {
    println!();
    println!(
        "{}",
        "╔═════════════════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║              MobileCLI - Link to Session                    ║".cyan()
    );
    println!(
        "{}",
        "╚═════════════════════════════════════════════════════════════╝".cyan()
    );
    println!();
    println!("Select a session to link:");
    println!();

    for (i, session) in sessions.iter().enumerate() {
        let age = chrono::Utc::now().signed_duration_since(
            chrono::DateTime::parse_from_rfc3339(&session.started_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now()),
        );
        let age_str = if age.num_hours() > 0 {
            format!("{}h", age.num_hours())
        } else {
            format!("{}m", age.num_minutes())
        };

        println!(
            "  {}. {} [{}] - {}",
            (i + 1).to_string().bold(),
            session.name.green(),
            age_str.dimmed(),
            session.project_path.dimmed()
        );
    }

    println!();
    print!(
        "Enter session number [1-{}] or 'q' to quit: ",
        sessions.len()
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input == "q" || input == "Q" {
        return Ok(None);
    }

    match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= sessions.len() => Ok(Some(sessions[n - 1])),
        _ => {
            println!("{}", "Invalid selection.".red());
            Ok(None)
        }
    }
}

/// Run in linked terminal mode
async fn run_linked_mode(
    ws_url: &str,
    session: &SessionListItem,
) -> Result<(), Box<dyn std::error::Error>> {
    // Connect to daemon
    let (ws, _) = connect_async(ws_url).await?;
    let (mut tx, mut rx) = ws.split();

    // Send hello
    let hello = ClientMessage::Hello {
        auth_token: None,
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    tx.send(Message::Text(serde_json::to_string(&hello)?))
        .await?;

    // Subscribe to session
    let subscribe = ClientMessage::Subscribe {
        session_id: session.session_id.clone(),
    };
    tx.send(Message::Text(serde_json::to_string(&subscribe)?))
        .await?;

    // Request session history
    let history_req = ClientMessage::GetSessionHistory {
        session_id: session.session_id.clone(),
        max_bytes: None,
    };
    tx.send(Message::Text(serde_json::to_string(&history_req)?))
        .await?;

    // Set up raw terminal mode (Unix only for now)
    #[cfg(unix)]
    let original_termios = {
        let stdin_fd = std::io::stdin().as_raw_fd();
        setup_raw_mode(stdin_fd)?
    };

    #[cfg(unix)]
    use std::os::unix::io::AsRawFd;

    println!("\r{}", "─".repeat(60).dimmed());
    println!(
        "\r{} Press {} to disconnect",
        "Linked:".green().bold(),
        "Ctrl+D".cyan().bold()
    );
    println!("\r{}", "─".repeat(60).dimmed());

    // Set up stdin reader
    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    // Spawn stdin reader thread with error handling
    std::thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 1024];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => {
                    // EOF - graceful shutdown
                    tracing::debug!("stdin EOF, shutting down input reader");
                    break;
                }
                Ok(n) => {
                    // Check for Ctrl+D (EOF character) - only when sent alone
                    // Unix terminals treat Ctrl+D as EOF only on empty line
                    if n == 1 && buf[0] == 0x04 {
                        tracing::debug!("Ctrl+D received, disconnecting");
                        break;
                    }
                    if input_tx.send(buf[..n].to_vec()).is_err() {
                        // Channel closed - main task has shut down
                        tracing::debug!("Input channel closed, shutting down reader");
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!("stdin read error: {}, shutting down reader", e);
                    break;
                }
            }
        }
    });

    let session_id = session.session_id.clone();
    let mut session_ended = false;

    loop {
        tokio::select! {
            // WebSocket messages from daemon
            result = rx.next() => {
                match result {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<ServerMessage>(&text) {
                            match msg {
                                ServerMessage::PtyBytes { session_id: sid, data } if sid == session_id => {
                                    if let Ok(bytes) = BASE64.decode(&data) {
                                        let mut stdout = io::stdout();
                                        let _ = stdout.write_all(&bytes);
                                        let _ = stdout.flush();
                                    }
                                }
                                ServerMessage::SessionHistory { session_id: sid, data, .. } if sid == session_id => {
                                    // Display history (catch-up)
                                    if let Ok(bytes) = BASE64.decode(&data) {
                                        let mut stdout = io::stdout();
                                        let _ = stdout.write_all(&bytes);
                                        let _ = stdout.flush();
                                    }
                                }
                                ServerMessage::SessionEnded { session_id: sid, exit_code } if sid == session_id => {
                                    session_ended = true;
                                    println!("\r\n{} Session ended (exit code: {})", "─".repeat(40).dimmed(), exit_code);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        println!("\r\n{}", "Connection closed.".yellow());
                        break;
                    }
                    _ => {}
                }
            }

            // Local stdin input
            Some(input) = input_rx.recv() => {
                let msg = ClientMessage::SendInput {
                    session_id: session_id.clone(),
                    text: String::from_utf8_lossy(&input).to_string(),
                    raw: true,
                    client_msg_id: None,
                };
                if tx.send(Message::Text(serde_json::to_string(&msg)?)).await.is_err() {
                    break;
                }
            }

            // Timeout/disconnect check
            else => break,
        }
    }

    // Restore terminal mode
    #[cfg(unix)]
    {
        let stdin_fd = std::io::stdin().as_raw_fd();
        let _ = restore_terminal_mode(stdin_fd, &original_termios);
    }

    if !session_ended {
        println!("\r\n{}", "Disconnected from session.".dimmed());
    }

    Ok(())
}

/// Set up raw terminal mode (Unix)
#[cfg(unix)]
fn setup_raw_mode(fd: i32) -> io::Result<nix::sys::termios::Termios> {
    use nix::sys::termios::{self, InputFlags, LocalFlags, SetArg};
    use std::os::fd::BorrowedFd;

    // SAFETY: fd is a valid file descriptor from stdin
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
    let original =
        termios::tcgetattr(borrowed_fd).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut raw = original.clone();

    // Disable canonical mode and echo
    raw.local_flags.remove(LocalFlags::ICANON);
    raw.local_flags.remove(LocalFlags::ECHO);
    raw.local_flags.remove(LocalFlags::ISIG);

    // Disable input processing
    raw.input_flags.remove(InputFlags::ICRNL);
    raw.input_flags.remove(InputFlags::IXON);

    termios::tcsetattr(borrowed_fd, SetArg::TCSANOW, &raw)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(original)
}

/// Restore terminal mode (Unix)
#[cfg(unix)]
fn restore_terminal_mode(fd: i32, original: &nix::sys::termios::Termios) -> io::Result<()> {
    use nix::sys::termios::{self, SetArg};
    use std::os::fd::BorrowedFd;

    // SAFETY: fd is a valid file descriptor from stdin
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
    termios::tcsetattr(borrowed_fd, SetArg::TCSANOW, original)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(())
}
