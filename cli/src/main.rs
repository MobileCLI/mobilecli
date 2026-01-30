//! MobileCLI - Stream any terminal session to your phone
//!
//! Usage:
//!   mobilecli              # Start your shell with mobile streaming
//!   mobilecli <command>    # Run a command with mobile streaming
//!   mobilecli -n "Work"    # Name your session
//!   mobilecli setup        # Run setup wizard (shows QR code)
//!   mobilecli status       # Show active sessions
//!   mobilecli daemon       # Run the background server
//!   mobilecli --help       # Show help

mod daemon;
mod detection;
mod link;
mod platform;
mod protocol;
mod pty_wrapper;
mod qr;
mod session;
mod setup;

use clap::{Parser, Subcommand};
use colored::Colorize;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "mobilecli")]
#[command(author = "bigphoot")]
#[command(version)]
#[command(about = "Stream any terminal session to your phone", long_about = None)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    run_args: Option<RunArgs>,
}

/// Arguments for running a command with mobile streaming
#[derive(Debug, Clone, Default, clap::Args)]
struct RunArgs {
    /// Command to run (defaults to your shell if not specified)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,

    /// Name for this session (shown in mobile app)
    #[arg(short = 'n', long = "name")]
    session_name: Option<String>,

    /// Don't show connection status on startup
    #[arg(long = "quiet", short = 'q')]
    quiet: bool,

    /// Working directory for the session
    #[arg(long = "dir")]
    working_dir: Option<String>,

    /// Run setup wizard and show QR code for pairing
    #[arg(long = "setup")]
    setup: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Show active streaming sessions
    Status,
    /// Run the setup wizard and show QR code for pairing
    Setup,
    /// Show QR code for mobile pairing
    Pair,
    /// Start the background daemon server
    Daemon {
        /// Port to listen on
        #[arg(short, long, default_value_t = daemon::DEFAULT_PORT)]
        port: u16,
    },
    /// Stop the background daemon
    Stop,
    /// Link to an existing session (like screen -x or tmux attach)
    Link {
        /// Session ID or name to link to (optional - shows picker if omitted)
        session: Option<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mobilecli=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Handle --setup flag (shortcut for setup subcommand)
    if let Some(ref run_args) = cli.run_args {
        if run_args.setup {
            return match run_setup().await {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}: {}", "Setup error".red().bold(), e);
                    ExitCode::FAILURE
                }
            };
        }
    }

    // Handle subcommands
    if let Some(command) = &cli.command {
        return match command {
            Commands::Status => {
                show_status();
                ExitCode::SUCCESS
            }
            Commands::Setup => match run_setup().await {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}: {}", "Setup error".red().bold(), e);
                    ExitCode::FAILURE
                }
            },
            Commands::Pair => match show_pair_qr().await {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    ExitCode::FAILURE
                }
            },
            Commands::Daemon { port } => {
                if daemon::is_running() {
                    eprintln!("{}", "Daemon is already running".yellow());
                    return ExitCode::FAILURE;
                }
                println!("{} Starting daemon on port {}...", "▶".green(), port);
                match daemon::run(*port).await {
                    Ok(_) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("{}: {}", "Daemon error".red().bold(), e);
                        ExitCode::FAILURE
                    }
                }
            }
            Commands::Stop => {
                stop_daemon();
                ExitCode::SUCCESS
            }
            Commands::Link { session } => match link::run(session.clone()).await {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}: {}", "Link error".red().bold(), e);
                    ExitCode::FAILURE
                }
            },
        };
    }

    // Get run args (or defaults)
    let run_args = cli.run_args.unwrap_or_default();

    // Ensure daemon is running
    if !daemon::is_running() {
        // Start daemon in background
        if let Err(e) = start_daemon_background().await {
            eprintln!("{}: {}", "Failed to start daemon".red().bold(), e);
            return ExitCode::FAILURE;
        }
    }

    // Check for first run - show setup wizard
    if setup::is_first_run() && run_args.args.is_empty() {
        println!();
        println!(
            "{}",
            "Welcome to MobileCLI! Let's get you set up.".cyan().bold()
        );
        match run_setup().await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("{}: {}", "Setup error".red().bold(), e);
                return ExitCode::FAILURE;
            }
        }
    }

    // Determine what command to run
    let (command, args) = if run_args.args.is_empty() {
        // Use cross-platform shell detection
        let shell = platform::default_shell();
        (shell, vec![])
    } else {
        let mut args = run_args.args;
        let command = args.remove(0);
        (command, args)
    };

    // Generate session name
    let session_name = run_args.session_name.unwrap_or_else(|| {
        std::path::Path::new(&command)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Terminal".to_string())
    });

    // Run the wrapped command (connects to daemon)
    let wrap_config = pty_wrapper::WrapConfig {
        command,
        args,
        session_name: session_name.clone(),
        quiet: run_args.quiet,
        working_dir: run_args.working_dir.clone(),
    };

    match pty_wrapper::run_wrapped(wrap_config).await {
        Ok(exit_code) => ExitCode::from(exit_code as u8),
        Err(e) => {
            eprintln!("{}: {}", "Error".red().bold(), e);
            ExitCode::FAILURE
        }
    }
}

/// Start daemon in background
async fn start_daemon_background() -> std::io::Result<()> {
    #[cfg(unix)]
    use nix::unistd::setsid;
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    // Get path to self
    let exe = std::env::current_exe()?;

    // Create log file for daemon stderr (cross-platform config directory)
    let log_dir = platform::config_dir();
    std::fs::create_dir_all(&log_dir)?;
    let log_file = std::fs::File::create(log_dir.join("daemon.log"))?;

    // Spawn daemon as background process with stderr logged for debugging
    let mut cmd = Command::new(&exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log_file));

    // Detach from controlling terminal so the daemon survives terminal closes.
    #[cfg(unix)]
    {
        // SAFETY: setsid() is async-signal-safe and does not call any non-reentrant functions.
        // Note: The daemon process inherits environment variables and open file descriptors
        // from the parent process. Stdin/stdout/stderr are explicitly redirected above.
        unsafe {
            cmd.pre_exec(|| {
                setsid().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                Ok(())
            });
        }
    }

    // Windows: Use CREATE_NO_WINDOW and DETACHED_PROCESS flags
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW = 0x08000000, DETACHED_PROCESS = 0x00000008
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    cmd.spawn()?;

    // Wait for daemon to start with retry
    let mut delay_ms = 100;
    for _ in 0..5 {
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        if daemon::is_running() {
            return Ok(());
        }
        delay_ms = (delay_ms * 2).min(1000); // Exponential backoff, max 1s
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Daemon failed to start (check ~/.mobilecli/daemon.log)",
    ))
}

/// Stop the daemon
fn stop_daemon() {
    if let Some(pid) = daemon::get_pid() {
        // Terminate process (cross-platform)
        if platform::terminate_process(pid) {
            println!("{} Daemon stopped", "✓".green());
        } else {
            println!("{}", "Failed to stop daemon".red());
        }
    } else {
        println!("{}", "Daemon is not running".dimmed());
    }
}

/// Show status of daemon and sessions
fn show_status() {
    if daemon::is_running() {
        if let Some(pid) = daemon::get_pid() {
            let port = daemon::get_port().unwrap_or(daemon::DEFAULT_PORT);
            println!(
                "{} Daemon running (PID: {}, port: {})",
                "●".green(),
                pid,
                port
            );
        }
    } else {
        println!("{} Daemon not running", "○".dimmed());
        println!("  Run {} to start", "mobilecli".cyan());
        return;
    }

    // Show sessions from session file (for now)
    let sessions = session::list_active_sessions();
    if sessions.is_empty() {
        println!("{}", "  No active sessions".dimmed());
    } else {
        println!(
            "\n{} {} active session(s):",
            "Sessions:".bold(),
            sessions.len()
        );
        for s in sessions {
            println!(
                "  {} {} - {}",
                "→".cyan(),
                s.name.bold(),
                s.command.dimmed()
            );
        }
    }
}

/// Run the setup wizard
async fn run_setup() -> Result<(), Box<dyn std::error::Error>> {
    // Run the interactive setup
    let _config = setup::run_setup_wizard()?;

    // Ensure daemon is running
    if !daemon::is_running() {
        start_daemon_background().await?;
    }

    // Show QR code for pairing
    println!();
    println!(
        "{}",
        "Scan this QR code with the MobileCLI app:".cyan().bold()
    );
    println!();

    show_pair_qr().await?;

    Ok(())
}

/// Show QR code for pairing
async fn show_pair_qr() -> Result<(), Box<dyn std::error::Error>> {
    // Get connection config (includes device_id and device_name)
    let config = setup::load_config().unwrap_or_default();

    let ip = match &config.connection_mode {
        setup::ConnectionMode::Local => setup::get_local_ip(),
        setup::ConnectionMode::Tailscale => {
            let ts = setup::check_tailscale();
            if ts.logged_in {
                ts.ip.or_else(setup::get_local_ip)
            } else {
                eprintln!("{}", "⚠ Tailscale not connected".yellow());
                setup::get_local_ip()
            }
        }
        setup::ConnectionMode::Custom(_) => setup::get_connection_ip(&config),
    };

    // Get the actual daemon port (fallback to default if not running)
    let port = daemon::get_port().unwrap_or(daemon::DEFAULT_PORT);

    if let Some(ip) = ip {
        let info = protocol::ConnectionInfo {
            ws_url: format!("ws://{}:{}", ip, port),
            session_id: String::new(), // Not session-specific
            session_name: None,
            encryption_key: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            device_id: Some(config.device_id),
            device_name: Some(config.device_name),
        };

        qr::display_session_qr(&info);
    } else {
        println!("  {} ws://localhost:{}", "Connect:".dimmed(), port);
    }

    Ok(())
}
