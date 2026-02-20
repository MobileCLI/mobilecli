//! Install/uninstall an autostart mechanism for the MobileCLI daemon.
//!
//! This is intentionally best-effort: if systemd/launchd commands are unavailable,
//! we still write the service file and print manual next steps.

use crate::{daemon, platform};
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

    #[cfg(target_os = "windows")]
    {
        install_windows_task()?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        eprintln!("{} Autostart is unsupported on this OS.", "✗".red());
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

    #[cfg(target_os = "windows")]
    {
        uninstall_windows_task()?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        eprintln!("{} Autostart is unsupported on this OS.", "✗".red());
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

    #[cfg(target_os = "windows")]
    {
        let task_exists = windows_task_exists();
        let script_path = windows_task_script_path();
        if task_exists {
            println!("{} Autostart: installed (Task Scheduler)", "✓".green());
            println!("  Task: {}", WINDOWS_TASK_NAME.cyan());
            println!("  Script: {}", script_path.display().to_string().dimmed());
            if !script_path.exists() {
                println!(
                    "{} Startup script is missing. Re-run {} to repair.",
                    "!".yellow(),
                    "mobilecli autostart install".cyan()
                );
            }
        } else {
            println!("{} Autostart: not installed", "○".dimmed());
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
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
    let has_systemctl = Command::new("systemctl")
        .arg("--user")
        .arg("status")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();

    if has_systemctl {
        let _ = Command::new("systemctl")
            .arg("--user")
            .arg("daemon-reload")
            .status();
        let _ = Command::new("systemctl")
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
    let _ = Command::new("systemctl")
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

    let _ = Command::new("systemctl")
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

    // Best-effort enable using modern launchctl first, then legacy fallback.
    let mut enabled = false;
    if let Some(gui_domain) = macos_gui_domain() {
        let service = format!("{}/{}", gui_domain, LAUNCHD_LABEL);
        let plist_arg = plist_path.to_string_lossy().to_string();

        let _ = Command::new("launchctl")
            .args(["bootout", &service])
            .status();
        let bootstrap_status = Command::new("launchctl")
            .args(["bootstrap", &gui_domain, &plist_arg])
            .status();

        if matches!(bootstrap_status, Ok(s) if s.success()) {
            let _ = Command::new("launchctl")
                .args(["enable", &service])
                .status();
            let _ = Command::new("launchctl")
                .args(["kickstart", "-k", &service])
                .status();
            enabled = true;
        }
    }

    if !enabled {
        let _ = Command::new("launchctl")
            .arg("unload")
            .arg("-w")
            .arg(&plist_path)
            .status();
        let status = Command::new("launchctl")
            .arg("load")
            .arg("-w")
            .arg(&plist_path)
            .status();
        enabled = matches!(status, Ok(s) if s.success());
    }

    if enabled {
        println!("{} Enabled autostart via launchd", "✓".green());
    } else {
        println!("{} Could not enable autostart automatically.", "!".yellow());
        println!(
            "  Run: {}",
            format!("launchctl load -w {}", plist_path.display()).cyan()
        );
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launchd_agent() -> Result<(), Box<dyn std::error::Error>> {
    let plist_path = launchd_plist_path();

    if let Some(gui_domain) = macos_gui_domain() {
        let service = format!("{}/{}", gui_domain, LAUNCHD_LABEL);
        let _ = Command::new("launchctl")
            .args(["bootout", &service])
            .status();
        let _ = Command::new("launchctl")
            .args(["disable", &service])
            .status();
    }

    let _ = Command::new("launchctl")
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

#[cfg(target_os = "macos")]
fn macos_gui_domain() -> Option<String> {
    let output = Command::new("id").arg("-u").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if uid.is_empty() {
        None
    } else {
        Some(format!("gui/{}", uid))
    }
}

#[cfg(target_os = "windows")]
const WINDOWS_TASK_NAME: &str = "MobileCLIDaemon";

#[cfg(target_os = "windows")]
fn windows_task_script_path() -> PathBuf {
    platform::config_dir()
        .join("autostart")
        .join("mobilecli-daemon.ps1")
}

#[cfg(target_os = "windows")]
fn escape_powershell_single_quoted(input: &str) -> String {
    input.replace('\'', "''")
}

#[cfg(target_os = "windows")]
fn build_windows_autostart_script(exe_path: &std::path::Path) -> String {
    let exe = escape_powershell_single_quoted(&exe_path.to_string_lossy());
    format!(
        "$ErrorActionPreference = 'SilentlyContinue'\n$exe = '{exe}'\nif (-not (Test-Path -LiteralPath $exe)) {{ exit 1 }}\n$arguments = @('daemon', '--port', '{port}')\nStart-Process -FilePath $exe -ArgumentList $arguments -WindowStyle Hidden\n",
        exe = exe,
        port = daemon::DEFAULT_PORT,
    )
}

#[cfg(target_os = "windows")]
fn windows_task_exists() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", WINDOWS_TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn schtasks_available() -> bool {
    Command::new("schtasks")
        .arg("/?")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn install_windows_task() -> Result<(), Box<dyn std::error::Error>> {
    if !schtasks_available() {
        return Err("Windows Task Scheduler (schtasks.exe) is not available".into());
    }

    let script_path = windows_task_script_path();
    if let Some(parent) = script_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe()?;
    let script = build_windows_autostart_script(&exe);
    std::fs::write(&script_path, script)?;
    println!(
        "{} Wrote startup script: {}",
        "✓".green(),
        script_path.display()
    );

    let task_cmd = format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"",
        script_path.display()
    );

    let create = Command::new("schtasks")
        .args([
            "/Create",
            "/F",
            "/SC",
            "ONLOGON",
            "/RL",
            "LIMITED",
            "/TN",
            WINDOWS_TASK_NAME,
            "/TR",
            &task_cmd,
        ])
        .output()?;

    if !create.status.success() {
        let stderr = String::from_utf8_lossy(&create.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&create.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("Failed to create scheduled task: {}", detail).into());
    }

    // Best-effort run now so users get a daemon immediately without relogin.
    let _ = Command::new("schtasks")
        .args(["/Run", "/TN", WINDOWS_TASK_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    println!(
        "{} Enabled autostart via Windows Task Scheduler",
        "✓".green()
    );
    println!(
        "  Check: {}",
        format!("schtasks /Query /TN {}", WINDOWS_TASK_NAME).cyan()
    );

    Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall_windows_task() -> Result<(), Box<dyn std::error::Error>> {
    if windows_task_exists() {
        let delete = Command::new("schtasks")
            .args(["/Delete", "/TN", WINDOWS_TASK_NAME, "/F"])
            .output()?;
        if delete.status.success() {
            println!(
                "{} Removed scheduled task: {}",
                "✓".green(),
                WINDOWS_TASK_NAME.cyan()
            );
        } else {
            let stderr = String::from_utf8_lossy(&delete.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&delete.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            println!(
                "{} Could not remove scheduled task: {}",
                "!".yellow(),
                detail
            );
        }
    } else {
        println!(
            "{} Scheduled task not found: {}",
            "○".dimmed(),
            WINDOWS_TASK_NAME.cyan()
        );
    }

    let script_path = windows_task_script_path();
    if script_path.exists() {
        std::fs::remove_file(&script_path)?;
        println!("{} Removed script: {}", "✓".green(), script_path.display());
    } else {
        println!(
            "{} Script not found: {}",
            "○".dimmed(),
            script_path.display()
        );
    }

    // Match Linux/macOS uninstall semantics by stopping the daemon now.
    if let Some(pid) = daemon::get_pid() {
        if platform::terminate_process(pid) {
            println!("{} Stopped running daemon (PID: {})", "✓".green(), pid);
        }
    }

    Ok(())
}
