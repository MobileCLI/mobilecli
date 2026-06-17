//! Native uninstaller for MobileCLI.
//!
//! Reverses everything `install.sh` and the setup wizard put in place:
//! - Stops the running daemon
//! - Removes daemon autostart (systemd / launchd / Task Scheduler)
//! - Removes the shell auto-launch hook from rc files
//! - Deletes the config directory (`~/.mobilecli`), including paired credentials
//! - Removes the `mobilecli` binary itself
//!
//! Use `--yes` to skip the confirmation prompt and `--keep-config` to preserve
//! `~/.mobilecli` (paired credentials, sessions, logs).

use crate::{autostart, daemon, platform, shell_hook};
use clap::Args;
use colored::Colorize;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Args)]
pub struct UninstallArgs {
    /// Skip the confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Keep the config directory (~/.mobilecli) and paired credentials
    #[arg(long = "keep-config")]
    pub keep_config: bool,

    /// Don't remove the mobilecli binary itself
    #[arg(long = "keep-binary")]
    pub keep_binary: bool,
}

pub fn run(args: UninstallArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = platform::config_dir();
    let binary = std::env::current_exe().ok();

    println!("{}", "This will remove MobileCLI from your system:".bold());
    println!("  • Stop the background daemon");
    println!("  • Remove daemon autostart (login service)");
    println!("  • Remove the shell auto-launch hook from your shell config");
    if !args.keep_config {
        println!(
            "  • Delete {} {}",
            config_dir.display().to_string().cyan(),
            "(paired credentials, sessions, logs)".dimmed()
        );
    }
    if !args.keep_binary {
        if let Some(ref exe) = binary {
            println!(
                "  • Remove the binary at {}",
                exe.display().to_string().cyan()
            );
        }
    }
    println!();

    if !args.yes && !confirm("Proceed with uninstall?")? {
        println!("{} Uninstall cancelled.", "○".dimmed());
        return Ok(());
    }

    // 1. Stop the daemon so no process keeps the port or config files busy.
    if let Some(pid) = daemon::get_pid() {
        if platform::terminate_process(pid) {
            println!("{} Stopped daemon (PID: {})", "✓".green(), pid);
        } else {
            eprintln!("{} Could not stop daemon (PID: {})", "!".yellow(), pid);
        }
    } else {
        println!("{} Daemon not running", "·".dimmed());
    }

    // 2. Remove autostart (best-effort; reuses platform-specific logic).
    if let Err(e) = autostart::run(autostart::AutostartCommand::Uninstall) {
        eprintln!("{} Autostart cleanup: {}", "!".yellow(), e);
    }

    // 3. Remove the shell auto-launch hook from rc files.
    if let Err(e) = shell_hook::run(shell_hook::ShellHookCommand::Uninstall) {
        eprintln!("{} Shell hook cleanup: {}", "!".yellow(), e);
    }

    // 4. Delete the config directory (credentials, sessions, logs, uploads).
    if args.keep_config {
        println!(
            "{} Kept config directory: {}",
            "·".dimmed(),
            config_dir.display().to_string().dimmed()
        );
    } else if config_dir.exists() {
        match std::fs::remove_dir_all(&config_dir) {
            Ok(_) => println!(
                "{} Removed config directory: {}",
                "✓".green(),
                config_dir.display()
            ),
            Err(e) => eprintln!(
                "{} Failed to remove {}: {}",
                "✗".red(),
                config_dir.display(),
                e
            ),
        }
    } else {
        println!("{} Config directory already gone", "·".dimmed());
    }

    // 5. Remove the binary itself.
    if !args.keep_binary {
        if let Some(exe) = binary {
            remove_binary(&exe);
        }
    }

    println!();
    println!(
        "{}",
        "MobileCLI has been uninstalled. Thanks for trying it!".green()
    );
    Ok(())
}

/// Remove the running binary. On Unix the file can be unlinked while the process
/// is still executing (the inode survives until exit). On Windows a running
/// executable cannot be deleted, so we fall back to printing manual instructions.
fn remove_binary(exe: &PathBuf) {
    #[cfg(windows)]
    {
        // Windows locks the running executable; schedule a best-effort delete and
        // tell the user how to finish if it fails.
        match std::fs::remove_file(exe) {
            Ok(_) => println!("{} Removed binary: {}", "✓".green(), exe.display()),
            Err(_) => {
                println!(
                    "{} Could not remove the binary while it is running.",
                    "!".yellow()
                );
                println!(
                    "  Delete it manually after this process exits: {}",
                    exe.display().to_string().cyan()
                );
            }
        }
    }

    #[cfg(not(windows))]
    {
        match std::fs::remove_file(exe) {
            Ok(_) => println!("{} Removed binary: {}", "✓".green(), exe.display()),
            Err(e) => {
                eprintln!("{} Failed to remove {}: {}", "✗".red(), exe.display(), e);
                println!(
                    "  Remove it manually: {}",
                    format!("rm {}", exe.display()).cyan()
                );
            }
        }
    }
}

/// Prompt the user for a yes/no confirmation. Defaults to "no" on EOF or empty input.
fn confirm(question: &str) -> std::io::Result<bool> {
    print!("{} [y/N] ", question);
    std::io::stdout().flush()?;

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        return Ok(false);
    }

    let answer = answer.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}
