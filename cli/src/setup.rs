//! Interactive setup wizard for MobileCLI
//!
//! Handles first-time setup and connection configuration.

use crate::platform;
use crate::shell_hook;
use colored::Colorize;
use std::io::{self, Write};
use std::process::Command;

/// Connection mode for the CLI
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionMode {
    /// Local network (same WiFi)
    Local,
    /// Tailscale VPN
    Tailscale,
    /// Custom/manual configuration
    Custom(String),
}

/// Configuration stored for the CLI
#[derive(Debug, Clone)]
pub struct Config {
    pub device_id: String,
    pub device_name: String,
    pub connection_mode: ConnectionMode,
    pub tailscale_ip: Option<String>,
    pub local_ip: Option<String>,
    /// Optional token embedded into pairing QR metadata.
    ///
    /// Stored locally in `~/.mobilecli/config.json`.
    pub auth_token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: get_hostname(),
            connection_mode: ConnectionMode::Local,
            tailscale_ip: None,
            local_ip: None,
            auth_token: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Get the system hostname for device identification
pub fn get_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Check if this is the first run (no config exists)
pub fn is_first_run() -> bool {
    let config_path = get_config_path();
    !config_path.exists()
}

/// Get the config file path (cross-platform)
fn get_config_path() -> std::path::PathBuf {
    platform::config_dir().join("config.json")
}

/// Load saved configuration
pub fn load_config() -> Option<Config> {
    let config_path = get_config_path();
    if !config_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let mode = match json.get("connection_mode")?.as_str()? {
        "local" => ConnectionMode::Local,
        "tailscale" => ConnectionMode::Tailscale,
        s if s.starts_with("custom:") => ConnectionMode::Custom(s[7..].to_string()),
        _ => return None,
    };

    // Get or generate device_id (for backwards compatibility with old configs).
    // Note: If config is deleted/corrupted, a new device_id is generated, which will
    // require re-pairing with the mobile app. This is intentional - preserving the
    // device_id separately would add complexity without much benefit since re-pairing
    // is a simple QR scan.
    let device_id = json
        .get("device_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Get or detect device_name
    let device_name = json
        .get("device_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(get_hostname);

    let mut needs_save = false;
    let auth_token = json
        .get("auth_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            needs_save = true;
            uuid::Uuid::new_v4().to_string()
        });

    let config = Config {
        device_id,
        device_name,
        connection_mode: mode,
        tailscale_ip: json
            .get("tailscale_ip")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        local_ip: json
            .get("local_ip")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        auth_token,
    };

    if needs_save {
        let _ = save_config(&config);
    }

    Some(config)
}

/// Save configuration
pub fn save_config(config: &Config) -> io::Result<()> {
    let config_path = get_config_path();
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mode_str = match &config.connection_mode {
        ConnectionMode::Local => "local".to_string(),
        ConnectionMode::Tailscale => "tailscale".to_string(),
        ConnectionMode::Custom(url) => format!("custom:{}", url),
    };

    let json = serde_json::json!({
        "device_id": config.device_id,
        "device_name": config.device_name,
        "connection_mode": mode_str,
        "tailscale_ip": config.tailscale_ip,
        "local_ip": config.local_ip,
        "auth_token": config.auth_token,
    });

    std::fs::write(&config_path, serde_json::to_string_pretty(&json)?)?;
    Ok(())
}

/// Check Tailscale status
#[derive(Debug)]
pub struct TailscaleStatus {
    pub installed: bool,
    pub logged_in: bool,
    pub ip: Option<String>,
}

pub fn check_tailscale() -> TailscaleStatus {
    // Check if tailscale command exists (cross-platform using which crate)
    let installed = which::which("tailscale").is_ok();

    if !installed {
        return TailscaleStatus {
            installed: false,
            logged_in: false,
            ip: None,
        };
    }

    // Check tailscale status
    let status_output = Command::new("tailscale")
        .arg("status")
        .arg("--json")
        .output();

    match status_output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                let backend_state = json
                    .get("BackendState")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let running = backend_state == "Running";
                let logged_in = running && json.get("Self").is_some();

                // Get the Tailscale IP
                let ip = json
                    .get("TailscaleIPs")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                TailscaleStatus {
                    installed: true,
                    logged_in,
                    ip,
                }
            } else {
                TailscaleStatus {
                    installed: true,
                    logged_in: false,
                    ip: None,
                }
            }
        }
        _ => TailscaleStatus {
            installed: true,
            logged_in: false,
            ip: None,
        },
    }
}

/// Get local IP address
pub fn get_local_ip() -> Option<String> {
    local_ip_address::local_ip().ok().map(|ip| ip.to_string())
}

/// Prompt user for input
fn prompt(message: &str) -> String {
    print!("{}", message);
    if io::stdout().flush().is_err() {
        return String::new();
    }
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return String::new();
    }
    input.trim().to_string()
}

/// Prompt user for yes/no
fn prompt_yn(message: &str, default: bool) -> bool {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    let input = prompt(&format!("{} {} ", message, suffix));

    if input.is_empty() {
        return default;
    }

    matches!(input.to_lowercase().as_str(), "y" | "yes")
}

/// Install Tailscale (Linux)
///
/// Security note: Downloads and executes the official Tailscale install script.
/// User is prompted for confirmation before execution. For additional security,
/// users can manually install via their package manager or verify the script at
/// https://tailscale.com/install.sh before running.
#[cfg(target_os = "linux")]
fn install_tailscale_linux() -> io::Result<bool> {
    println!();
    println!("{}", "Installing Tailscale...".cyan());
    println!("This will download and run the official Tailscale installer.");
    println!("Script URL: {}", "https://tailscale.com/install.sh".cyan());
    println!();
    println!(
        "{}",
        "Alternatively, install manually: https://tailscale.com/download/linux".dimmed()
    );
    println!();

    if !prompt_yn("Download and run installer?", true) {
        return Ok(false);
    }

    // Download and run install script (user has confirmed)
    let status = Command::new("sh")
        .arg("-c")
        .arg("curl -fsSL https://tailscale.com/install.sh | sh")
        .status()?;

    if status.success() {
        println!("{}", "✓ Tailscale installed successfully!".green());
        Ok(true)
    } else {
        println!("{}", "✗ Tailscale installation failed".red());
        Ok(false)
    }
}

/// Install Tailscale (macOS)
#[cfg(target_os = "macos")]
fn install_tailscale_macos() -> io::Result<bool> {
    println!();
    println!("{}", "Installing Tailscale via Homebrew...".cyan());

    // Check if brew is available (cross-platform using which crate)
    let brew_available = which::which("brew").is_ok();

    if !brew_available {
        println!(
            "{}",
            "Homebrew not found. Please install Tailscale manually:".yellow()
        );
        println!("  https://tailscale.com/download/mac");
        return Ok(false);
    }

    println!("This will run: brew install tailscale");
    println!();

    if !prompt_yn("Continue?", true) {
        return Ok(false);
    }

    let status = Command::new("brew")
        .args(["install", "tailscale"])
        .status()?;

    if status.success() {
        println!("{}", "✓ Tailscale installed successfully!".green());
        Ok(true)
    } else {
        println!("{}", "✗ Tailscale installation failed".red());
        Ok(false)
    }
}

/// Start Tailscale and login
fn start_tailscale() -> io::Result<bool> {
    println!();
    println!("{}", "Starting Tailscale...".cyan());

    // Try to start tailscaled (Linux)
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("sudo")
            .args(["systemctl", "start", "tailscaled"])
            .status();
    }

    // Run tailscale up
    println!("Running: tailscale up");
    println!(
        "{}",
        "This will open a browser for authentication.".dimmed()
    );
    println!();

    let status = Command::new("tailscale").arg("up").status()?;

    if status.success() {
        println!("{}", "✓ Tailscale connected!".green());
        Ok(true)
    } else {
        println!("{}", "✗ Tailscale connection failed".red());
        Ok(false)
    }
}

/// Run the interactive setup wizard
pub fn run_setup_wizard() -> io::Result<Config> {
    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║              📱 MobileCLI Setup Wizard                       ║".cyan()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════════╝".cyan()
    );
    println!();
    println!("How would you like to connect your mobile device?");
    println!();
    println!(
        "  {} {} - Same WiFi network (easiest)",
        "1.".bold(),
        "Local Network".green()
    );
    println!("     Good for home/office use");
    println!();
    println!(
        "  {} {} - Connect from anywhere (recommended)",
        "2.".bold(),
        "Tailscale VPN".green()
    );
    println!("     Secure, works on any network");
    println!();
    println!(
        "  {} {} - Enter your own WebSocket URL",
        "3.".bold(),
        "Custom".dimmed()
    );
    println!();

    let choice = loop {
        let input = prompt("Choose an option [1-3]: ");
        match input.as_str() {
            "1" => break 1,
            "2" => break 2,
            "3" => break 3,
            "" => break 1, // Default to local
            _ => println!("{}", "Please enter 1, 2, or 3".yellow()),
        }
    };

    let mut config = Config::default();

    match choice {
        1 => {
            // Local network
            config.connection_mode = ConnectionMode::Local;
            config.local_ip = get_local_ip();

            if let Some(ip) = &config.local_ip {
                println!();
                println!("{} Local IP: {}", "✓".green(), ip.cyan());
                println!();
                println!(
                    "{}",
                    "Make sure your phone is on the same WiFi network.".dimmed()
                );
            } else {
                println!();
                println!("{}", "⚠ Could not detect local IP address".yellow());
                println!("  You may need to find it manually (ifconfig / ip addr)");
            }
        }
        2 => {
            // Tailscale
            config.connection_mode = ConnectionMode::Tailscale;

            println!();
            println!("{}", "Checking Tailscale status...".dimmed());

            let mut ts_status = check_tailscale();

            // Install if needed
            if !ts_status.installed {
                println!();
                println!("{}", "Tailscale is not installed.".yellow());

                if prompt_yn("Would you like to install Tailscale now?", true) {
                    #[cfg(target_os = "macos")]
                    let installed = install_tailscale_macos()?;

                    #[cfg(target_os = "linux")]
                    let installed = install_tailscale_linux()?;

                    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                    let installed = {
                        println!(
                            "{}",
                            "Automatic installation not supported on this OS.".yellow()
                        );
                        println!(
                            "Please install Tailscale manually: https://tailscale.com/download"
                        );
                        false
                    };

                    if installed {
                        ts_status = check_tailscale();
                    }
                }
            }

            // Login if needed
            if ts_status.installed && !ts_status.logged_in {
                println!();
                println!("{}", "Tailscale is not logged in.".yellow());

                if prompt_yn("Would you like to login now?", true) && start_tailscale()? {
                    ts_status = check_tailscale();
                }
            }

            // Get IP
            if ts_status.logged_in {
                config.tailscale_ip = ts_status.ip.clone();

                if let Some(ip) = &ts_status.ip {
                    println!();
                    println!("{} Tailscale IP: {}", "✓".green(), ip.cyan());
                    println!();
                    println!("{}", "Your phone will need Tailscale installed and logged into the same account.".dimmed());
                }
            } else {
                println!();
                println!("{}", "⚠ Tailscale not fully configured.".yellow());
                println!("  Run 'tailscale up' to complete setup.");

                // Fall back to local
                config.local_ip = get_local_ip();
            }
        }
        3 => {
            // Custom
            println!();
            let url = prompt(
                "Enter WebSocket URL (e.g., ws://192.168.1.100:9847 or wss://demo.example.com): ",
            );
            config.connection_mode = ConnectionMode::Custom(url);
        }
        _ => unreachable!(),
    }

    // Save configuration
    save_config(&config)?;

    // Offer auto-launch hook installation
    println!();
    println!(
        "{}",
        "── Auto-Launch ──────────────────────────────────────────────".dimmed()
    );
    println!();
    println!(
        "Would you like mobilecli to start {} when you open a new terminal?",
        "automatically".green().bold()
    );
    println!(
        "{}",
        "This adds a small hook to your shell config (bashrc/zshrc/etc).".dimmed()
    );
    println!(
        "{}",
        "You can remove it anytime with: mobilecli shell-hook uninstall".dimmed()
    );
    println!();

    if prompt_yn("Enable auto-launch?", true) {
        if shell_hook::install_quiet() {
            println!(
                "{} Auto-launch enabled! New terminals will start mobilecli automatically.",
                "✓".green()
            );
            println!(
                "  To disable temporarily: {}",
                "export MOBILECLI_NO_AUTO_LAUNCH=1".dimmed()
            );
        } else {
            println!(
                "{} Auto-launch hook was already installed (or could not detect your shell).",
                "·".dimmed()
            );
        }
    } else {
        println!(
            "{}",
            "Skipped. You can enable later with: mobilecli shell-hook install".dimmed()
        );
    }

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════════╗".green()
    );
    println!(
        "{}",
        "║                    ✓ Setup Complete!                         ║".green()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════════╝".green()
    );
    println!();
    println!(
        "Run {} to start a terminal session.",
        "mobilecli".cyan().bold()
    );
    println!(
        "Run {} to change these settings.",
        "mobilecli setup".dimmed()
    );
    println!();

    Ok(config)
}
