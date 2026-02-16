//! Install/uninstall an autostart mechanism for the MobileCLI daemon.
//!
//! This is intentionally best-effort: if systemd/launchd commands are unavailable,
//! we still write the service file and print manual next steps.

use crate::{daemon, platform};
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand, Debug, Clone)]
pub enum AutostartCommand {
    /// Install and enable daemon autostart on login
    Install,
    /// Disable and remove daemon autostart
    Uninstall,
    /// Show whether autostart is installed and whether the daemon is currently running
    Status,
}

pub fn run(cmd: AutostartCommand) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        AutostartCommand::Install => install(),
        AutostartCommand::Uninstall => uninstall(),
        AutostartCommand::Status => status(),
    }
}

fn install() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        install_systemd_user()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        install_launchd_agent()?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        eprintln!(
            "{} Autostart is only supported on Linux (systemd) and macOS (launchd) right now.",
            "✗".red()
        );
        Ok(())
    }
}

fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        uninstall_systemd_user()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        uninstall_launchd_agent()?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        eprintln!(
            "{} Autostart is only supported on Linux (systemd) and macOS (launchd) right now.",
            "✗".red()
        );
        Ok(())
    }
}

fn status() -> Result<(), Box<dyn std::error::Error>> {
    let running = daemon::is_running();
    if running {
        println!("{} Daemon: running", "●".green());
    } else {
        println!("{} Daemon: not running", "○".dimmed());
    }

    #[cfg(target_os = "linux")]
    {
        let unit_path = systemd_unit_path();
        if unit_path.exists() {
            println!("{} Autostart: installed (systemd user unit)", "✓".green());
            println!("  Unit: {}", unit_path.display().to_string().dimmed());
        } else {
            println!("{} Autostart: not installed", "○".dimmed());
        }
    }

    #[cfg(target_os = "macos")]
    {
        let plist_path = launchd_plist_path();
        if plist_path.exists() {
            println!("{} Autostart: installed (launchd agent)", "✓".green());
            println!("  Plist: {}", plist_path.display().to_string().dimmed());
        } else {
            println!("{} Autostart: not installed", "○".dimmed());
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        println!("{} Autostart: unsupported on this OS", "○".dimmed());
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn systemd_unit_path() -> PathBuf {
    platform::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/systemd/user/mobilecli.service")
}

#[cfg(target_os = "linux")]
fn install_systemd_user() -> Result<(), Box<dyn std::error::Error>> {
    let unit_path = systemd_unit_path();
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe()?;
    let log_dir = platform::config_dir();
    std::fs::create_dir_all(&log_dir)?;
    let log_file = log_dir.join("daemon.log");

    let unit = format!(
        "[Unit]\nDescription=MobileCLI daemon\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nExecStart={} daemon --port {}\nRestart=always\nRestartSec=2\nStandardOutput=append:{}\nStandardError=append:{}\n\n[Install]\nWantedBy=default.target\n",
        exe.display(),
        daemon::DEFAULT_PORT,
        log_file.display(),
        log_file.display(),
    );

    std::fs::write(&unit_path, unit)?;
    println!(
        "{} Wrote systemd user unit: {}",
        "✓".green(),
        unit_path.display()
    );

    // Best-effort enable/start.
    let has_systemctl = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("status")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok();

    if has_systemctl {
        let _ = std::process::Command::new("systemctl")
            .arg("--user")
            .arg("daemon-reload")
            .status();
        let _ = std::process::Command::new("systemctl")
            .arg("--user")
            .arg("enable")
            .arg("--now")
            .arg("mobilecli.service")
            .status();

        println!("{} Enabled autostart via systemd", "✓".green());
        println!(
            "  Check: {}",
            "systemctl --user status mobilecli.service".cyan()
        );
    } else {
        println!(
            "{} systemctl not available; unit written but not enabled automatically.",
            "!".yellow()
        );
        println!("  Run: {}", "systemctl --user daemon-reload".cyan());
        println!(
            "  Run: {}",
            "systemctl --user enable --now mobilecli.service".cyan()
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_systemd_user() -> Result<(), Box<dyn std::error::Error>> {
    let unit_path = systemd_unit_path();

    // Best-effort disable/stop.
    let _ = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("disable")
        .arg("--now")
        .arg("mobilecli.service")
        .status();

    if unit_path.exists() {
        std::fs::remove_file(&unit_path)?;
        println!("{} Removed unit: {}", "✓".green(), unit_path.display());
    } else {
        println!("{} Unit not found: {}", "○".dimmed(), unit_path.display());
    }

    let _ = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload")
        .status();

    Ok(())
}

#[cfg(target_os = "macos")]
const LAUNCHD_LABEL: &str = "com.mobilecli.daemon";

#[cfg(target_os = "macos")]
fn launchd_plist_path() -> PathBuf {
    platform::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(format!("Library/LaunchAgents/{}.plist", LAUNCHD_LABEL))
}

#[cfg(target_os = "macos")]
fn install_launchd_agent() -> Result<(), Box<dyn std::error::Error>> {
    let plist_path = launchd_plist_path();
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe()?;
    let log_dir = platform::config_dir();
    std::fs::create_dir_all(&log_dir)?;
    let log_file = log_dir.join("daemon.log");

    let plist = format!(
        r#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">
<plist version=\"1.0\">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>daemon</string>
    <string>--port</string>
    <string>{port}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = LAUNCHD_LABEL,
        exe = exe.display(),
        port = daemon::DEFAULT_PORT,
        log = log_file.display(),
    );

    std::fs::write(&plist_path, plist)?;
    println!(
        "{} Wrote launchd agent: {}",
        "✓".green(),
        plist_path.display()
    );

    // Best-effort load.
    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg("-w")
        .arg(&plist_path)
        .status();
    let status = std::process::Command::new("launchctl")
        .arg("load")
        .arg("-w")
        .arg(&plist_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("{} Enabled autostart via launchd", "✓".green());
        }
        _ => {
            println!("{} Could not enable autostart automatically.", "!".yellow());
            println!(
                "  Run: {}",
                format!("launchctl load -w {}", plist_path.display()).cyan()
            );
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launchd_agent() -> Result<(), Box<dyn std::error::Error>> {
    let plist_path = launchd_plist_path();

    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg("-w")
        .arg(&plist_path)
        .status();

    if plist_path.exists() {
        std::fs::remove_file(&plist_path)?;
        println!("{} Removed plist: {}", "✓".green(), plist_path.display());
    } else {
        println!("{} Plist not found: {}", "○".dimmed(), plist_path.display());
    }

    Ok(())
}
