//! Shell auto-launch hook for MobileCLI.
//!
//! Injects (or removes) a small snippet into the user's shell configuration file
//! so that opening a new terminal automatically starts `mobilecli`.
//!
//! Supported shells:
//! - bash  (~/.bashrc, ~/.bash_profile on macOS)
//! - zsh   (~/.zshrc)
//! - fish  (~/.config/fish/config.fish)
//! - PowerShell ($PROFILE)
//! - cmd.exe (Registry AutoRun) — prints manual instructions only
//!
//! The snippet is wrapped in sentinel comments so we can find and remove it later.
//! An environment variable `MOBILECLI_SESSION` is set by the PTY wrapper to prevent
//! recursive auto-launch.  The user can also set `MOBILECLI_NO_AUTO_LAUNCH=1` as an
//! escape hatch.

use crate::platform;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

// ── Sentinel markers ──────────────────────────────────────────────────────────

const BEGIN_MARKER: &str = "# >>> mobilecli auto-launch >>>";
const END_MARKER: &str = "# <<< mobilecli auto-launch <<<";

const BEGIN_MARKER_PS: &str = "# >>> mobilecli auto-launch >>>";
const END_MARKER_PS: &str = "# <<< mobilecli auto-launch <<<";

// ── Shell snippets ────────────────────────────────────────────────────────────

fn bash_snippet() -> String {
    format!(
        r#"{BEGIN_MARKER}
# Automatically launch mobilecli in interactive terminals.
# To disable: export MOBILECLI_NO_AUTO_LAUNCH=1  (or run: mobilecli shell-hook uninstall)
if [ -z "$MOBILECLI_NO_AUTO_LAUNCH" ] && [ -z "$MOBILECLI_SESSION" ] && [ -t 0 ] && command -v mobilecli >/dev/null 2>&1; then
  exec mobilecli
fi
{END_MARKER}"#,
        BEGIN_MARKER = BEGIN_MARKER,
        END_MARKER = END_MARKER,
    )
}

fn fish_snippet() -> String {
    format!(
        r#"{BEGIN_MARKER}
# Automatically launch mobilecli in interactive terminals.
# To disable: set -gx MOBILECLI_NO_AUTO_LAUNCH 1  (or run: mobilecli shell-hook uninstall)
if not set -q MOBILECLI_NO_AUTO_LAUNCH; and not set -q MOBILECLI_SESSION; and isatty stdin; and command -v mobilecli >/dev/null 2>&1
    exec mobilecli
end
{END_MARKER}"#,
        BEGIN_MARKER = BEGIN_MARKER,
        END_MARKER = END_MARKER,
    )
}

fn powershell_snippet() -> String {
    format!(
        r#"{BEGIN_MARKER}
# Automatically launch mobilecli in interactive terminals.
# To disable: $env:MOBILECLI_NO_AUTO_LAUNCH = "1"  (or run: mobilecli shell-hook uninstall)
if (-not $env:MOBILECLI_NO_AUTO_LAUNCH -and -not $env:MOBILECLI_SESSION -and [Environment]::UserInteractive -and (Get-Command mobilecli -ErrorAction SilentlyContinue)) {{
    & mobilecli
    exit $LASTEXITCODE
}}
{END_MARKER}"#,
        BEGIN_MARKER = BEGIN_MARKER_PS,
        END_MARKER = END_MARKER_PS,
    )
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug, Clone)]
pub enum ShellHookCommand {
    /// Install auto-launch hook into your shell config
    Install,
    /// Remove auto-launch hook from your shell config
    Uninstall,
    /// Show whether the hook is installed and for which shell(s)
    Status,
}

pub fn run(cmd: ShellHookCommand) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ShellHookCommand::Install => install(),
        ShellHookCommand::Uninstall => uninstall(),
        ShellHookCommand::Status => status(),
    }
}

// ── Install ───────────────────────────────────────────────────────────────────

fn install() -> Result<(), Box<dyn std::error::Error>> {
    let targets = detect_shell_targets();
    if targets.is_empty() {
        eprintln!(
            "{} Could not detect your shell. Set $SHELL or run this inside bash/zsh/fish/PowerShell.",
            "✗".red()
        );
        print_cmd_instructions();
        return Ok(());
    }

    let mut any_installed = false;
    for target in &targets {
        match install_into(target) {
            Ok(true) => {
                println!(
                    "{} Installed auto-launch hook into {}",
                    "✓".green(),
                    target.path.display().to_string().cyan()
                );
                any_installed = true;
            }
            Ok(false) => {
                println!(
                    "{} Hook already present in {}",
                    "·".dimmed(),
                    target.path.display().to_string().dimmed()
                );
            }
            Err(e) => {
                eprintln!(
                    "{} Failed to write {}: {}",
                    "✗".red(),
                    target.path.display(),
                    e
                );
            }
        }
    }

    if any_installed {
        println!();
        println!(
            "{}",
            "Open a new terminal to start mobilecli automatically.".cyan()
        );
        println!(
            "  To disable temporarily: {}",
            "export MOBILECLI_NO_AUTO_LAUNCH=1".dimmed()
        );
        println!(
            "  To remove permanently:  {}",
            "mobilecli shell-hook uninstall".dimmed()
        );
    }

    // Print CMD instructions on Windows as a courtesy.
    #[cfg(windows)]
    print_cmd_instructions();

    Ok(())
}

// ── Uninstall ─────────────────────────────────────────────────────────────────

fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
    let targets = all_possible_targets();
    let mut any_removed = false;

    for target in &targets {
        if !target.path.exists() {
            continue;
        }
        match remove_from(target) {
            Ok(true) => {
                println!(
                    "{} Removed auto-launch hook from {}",
                    "✓".green(),
                    target.path.display().to_string().cyan()
                );
                any_removed = true;
            }
            Ok(false) => {} // not present, nothing to say
            Err(e) => {
                eprintln!(
                    "{} Failed to update {}: {}",
                    "✗".red(),
                    target.path.display(),
                    e
                );
            }
        }
    }

    if !any_removed {
        println!("{} No auto-launch hooks found to remove.", "·".dimmed());
    }

    Ok(())
}

// ── Status ────────────────────────────────────────────────────────────────────

fn status() -> Result<(), Box<dyn std::error::Error>> {
    let targets = all_possible_targets();
    let mut any_found = false;

    for target in &targets {
        if !target.path.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&target.path).unwrap_or_default();
        if content.contains(BEGIN_MARKER) {
            println!(
                "{} Auto-launch: installed ({}: {})",
                "✓".green(),
                target.shell_name,
                target.path.display().to_string().dimmed()
            );
            any_found = true;
        }
    }

    if !any_found {
        println!("{} Auto-launch: not installed", "○".dimmed());
        println!(
            "  Run {} to enable",
            "mobilecli shell-hook install".cyan()
        );
    }

    Ok(())
}

// ── Shell target detection ────────────────────────────────────────────────────

struct ShellTarget {
    shell_name: &'static str,
    path: PathBuf,
    snippet: String,
}

/// Detect which shell the user is running and return the target rc file(s).
fn detect_shell_targets() -> Vec<ShellTarget> {
    let home = match platform::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let shell = current_shell_name();
    let mut targets = Vec::new();

    match shell.as_str() {
        "bash" => {
            targets.push(ShellTarget {
                shell_name: "bash",
                path: home.join(".bashrc"),
                snippet: bash_snippet(),
            });
            // macOS login shells read .bash_profile, not .bashrc.
            // Add to .bash_profile as well if we're on macOS.
            #[cfg(target_os = "macos")]
            {
                targets.push(ShellTarget {
                    shell_name: "bash (login)",
                    path: home.join(".bash_profile"),
                    snippet: bash_snippet(),
                });
            }
        }
        "zsh" => {
            targets.push(ShellTarget {
                shell_name: "zsh",
                path: home.join(".zshrc"),
                snippet: bash_snippet(), // zsh uses the same POSIX syntax
            });
        }
        "fish" => {
            targets.push(ShellTarget {
                shell_name: "fish",
                path: home.join(".config/fish/config.fish"),
                snippet: fish_snippet(),
            });
        }
        "powershell" | "pwsh" => {
            if let Some(profile) = powershell_profile_path(&home) {
                targets.push(ShellTarget {
                    shell_name: "PowerShell",
                    path: profile,
                    snippet: powershell_snippet(),
                });
            }
        }
        _ => {
            // Unknown shell — try to detect from available rc files.
            // Add zshrc if it exists, bashrc if it exists.
            if home.join(".zshrc").exists() {
                targets.push(ShellTarget {
                    shell_name: "zsh",
                    path: home.join(".zshrc"),
                    snippet: bash_snippet(),
                });
            } else if home.join(".bashrc").exists() {
                targets.push(ShellTarget {
                    shell_name: "bash",
                    path: home.join(".bashrc"),
                    snippet: bash_snippet(),
                });
            }
        }
    }

    targets
}

/// Return all rc file paths we might have written to (for uninstall/status).
fn all_possible_targets() -> Vec<ShellTarget> {
    let home = match platform::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let mut targets = vec![
        ShellTarget {
            shell_name: "bash",
            path: home.join(".bashrc"),
            snippet: bash_snippet(),
        },
        ShellTarget {
            shell_name: "bash (login)",
            path: home.join(".bash_profile"),
            snippet: bash_snippet(),
        },
        ShellTarget {
            shell_name: "bash (profile)",
            path: home.join(".profile"),
            snippet: bash_snippet(),
        },
        ShellTarget {
            shell_name: "zsh",
            path: home.join(".zshrc"),
            snippet: bash_snippet(),
        },
        ShellTarget {
            shell_name: "fish",
            path: home.join(".config/fish/config.fish"),
            snippet: fish_snippet(),
        },
    ];

    if let Some(profile) = powershell_profile_path(&home) {
        targets.push(ShellTarget {
            shell_name: "PowerShell",
            path: profile,
            snippet: powershell_snippet(),
        });
    }

    targets
}

/// Detect the current shell name (lowercase, basename only).
fn current_shell_name() -> String {
    #[cfg(unix)]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            if let Some(name) = std::path::Path::new(&shell).file_name() {
                return name.to_string_lossy().to_lowercase();
            }
        }
    }

    #[cfg(windows)]
    {
        // Check parent process name or common env hints.
        if std::env::var("PSModulePath").is_ok() {
            return "powershell".to_string();
        }
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if comspec.to_lowercase().contains("cmd.exe") {
                return "cmd".to_string();
            }
        }
    }

    "unknown".to_string()
}

/// Determine the PowerShell profile path.
fn powershell_profile_path(home: &std::path::Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        // Windows: ~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1
        let docs = home.join("Documents").join("PowerShell");
        return Some(docs.join("Microsoft.PowerShell_profile.ps1"));
    }

    #[cfg(unix)]
    {
        // Unix: ~/.config/powershell/Microsoft.PowerShell_profile.ps1
        let dir = home.join(".config").join("powershell");
        if dir.exists() || which::which("pwsh").is_ok() {
            return Some(dir.join("Microsoft.PowerShell_profile.ps1"));
        }
        return None;
    }

    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

// ── File manipulation ─────────────────────────────────────────────────────────

/// Install the snippet into the target file. Returns Ok(true) if written, Ok(false) if already present.
fn install_into(target: &ShellTarget) -> std::io::Result<bool> {
    // Read existing content (or empty if file doesn't exist yet).
    let existing = if target.path.exists() {
        std::fs::read_to_string(&target.path)?
    } else {
        String::new()
    };

    // Already installed?
    if existing.contains(BEGIN_MARKER) {
        return Ok(false);
    }

    // Create parent directories if needed.
    if let Some(parent) = target.path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Append the snippet with a blank line separator.
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(&target.snippet);
    content.push('\n');

    std::fs::write(&target.path, content)?;
    Ok(true)
}

/// Remove the snippet from the target file. Returns Ok(true) if removed, Ok(false) if not found.
fn remove_from(target: &ShellTarget) -> std::io::Result<bool> {
    let content = std::fs::read_to_string(&target.path)?;

    if !content.contains(BEGIN_MARKER) {
        return Ok(false);
    }

    // Remove everything between (and including) the sentinel markers.
    let mut result = String::with_capacity(content.len());
    let mut skipping = false;

    for line in content.lines() {
        if line.trim() == BEGIN_MARKER || line.trim() == BEGIN_MARKER_PS {
            skipping = true;
            continue;
        }
        if skipping && (line.trim() == END_MARKER || line.trim() == END_MARKER_PS) {
            skipping = false;
            continue;
        }
        if !skipping {
            result.push_str(line);
            result.push('\n');
        }
    }

    // Trim trailing blank lines that our insertion left behind.
    let trimmed = result.trim_end_matches('\n').to_string() + "\n";

    std::fs::write(&target.path, trimmed)?;
    Ok(true)
}

// ── CMD.exe helper ────────────────────────────────────────────────────────────

fn print_cmd_instructions() {
    #[cfg(windows)]
    {
        println!();
        println!(
            "{} For cmd.exe auto-launch, add this registry key manually:",
            "ℹ".cyan()
        );
        println!(
            "  {}",
            r#"reg add "HKCU\Software\Microsoft\Command Processor" /v AutoRun /t REG_SZ /d "mobilecli" /f"#
                .dimmed()
        );
        println!(
            "  To remove: {}",
            r#"reg delete "HKCU\Software\Microsoft\Command Processor" /v AutoRun /f"#.dimmed()
        );
    }
}

/// Convenience: install shell hook non-interactively (called from setup wizard).
pub fn install_quiet() -> bool {
    let targets = detect_shell_targets();
    let mut any_installed = false;
    for target in &targets {
        if let Ok(true) = install_into(&target) {
            any_installed = true;
        }
    }
    any_installed
}

/// Convenience: uninstall shell hook non-interactively.
pub fn uninstall_quiet() -> bool {
    let targets = all_possible_targets();
    let mut any_removed = false;
    for target in &targets {
        if target.path.exists() {
            if let Ok(true) = remove_from(&target) {
                any_removed = true;
            }
        }
    }
    any_removed
}
