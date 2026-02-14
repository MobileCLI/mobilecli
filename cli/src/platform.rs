//! Cross-platform utilities for MobileCLI
//!
//! This module provides platform-agnostic functions for:
//! - Home directory detection
//! - Config directory paths
//! - Default shell detection
//! - Process management (alive check, termination)
//!
//! Supports Linux, macOS, and Windows.

use std::path::PathBuf;

/// Get the user's home directory in a cross-platform way.
///
/// Uses the `dirs-next` crate which handles:
/// - Linux: `$HOME` or `/home/<user>`
/// - macOS: `$HOME` or `/Users/<user>`
/// - Windows: `{FOLDERID_Profile}` (typically `C:\Users\<user>`)
pub fn home_dir() -> Option<PathBuf> {
    dirs_next::home_dir()
}

/// Get the MobileCLI config directory.
///
/// Returns:
/// - Linux/macOS: `~/.mobilecli`
/// - Windows: `%USERPROFILE%\.mobilecli`
///
/// Note: We use a dot-prefix directory on all platforms for consistency.
/// On Windows, this won't be hidden by default, but keeps paths predictable.
pub fn config_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mobilecli")
}

/// Get the default shell for the current platform.
///
/// Returns:
/// - Unix (Linux/macOS): `$SHELL` environment variable, or `/bin/sh` as fallback
/// - Windows: PowerShell if available, otherwise `COMSPEC` (typically cmd.exe)
pub fn default_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }

    #[cfg(windows)]
    {
        // Check for PowerShell first (preferred on modern Windows)
        let powershell =
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        if powershell.exists() {
            return powershell.to_string_lossy().to_string();
        }
        // Fall back to COMSPEC (typically cmd.exe)
        if let Ok(comspec) = std::env::var("COMSPEC") {
            return comspec;
        }
        // Ultimate fallback
        "cmd.exe".to_string()
    }

    #[cfg(not(any(unix, windows)))]
    {
        // Unknown platform fallback
        "sh".to_string()
    }
}

/// Check if a process is still alive.
///
/// - Unix: Uses `kill(pid, 0)` signal test
/// - Windows: Uses Windows API to check process existence
#[cfg(unix)]
pub fn is_process_alive(pid: u32) -> bool {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    // kill with signal 0 checks if process exists without sending a signal
    kill(Pid::from_raw(pid as i32), None::<Signal>).is_ok()
}

#[cfg(windows)]
pub fn is_process_alive(pid: u32) -> bool {
    // PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    // OpenProcess and GetExitCodeProcess from Windows API
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut std::ffi::c_void;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
        fn GetExitCodeProcess(hProcess: *mut std::ffi::c_void, lpExitCode: *mut u32) -> i32;
    }

    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }

        let mut exit_code: u32 = 0;
        let result = GetExitCodeProcess(handle, &mut exit_code);

        // Store result before closing handle (cleaner pattern)
        let is_alive = result != 0 && exit_code == STILL_ACTIVE;
        CloseHandle(handle);

        is_alive
    }
}

#[cfg(not(any(unix, windows)))]
pub fn is_process_alive(_pid: u32) -> bool {
    // Conservative default on unknown platforms - assume alive
    true
}

/// Terminate a process by PID.
///
/// - Unix: Sends SIGTERM signal
/// - Windows: Uses TerminateProcess API
///
/// Returns true if the signal/termination was sent successfully.
#[cfg(unix)]
pub fn terminate_process(pid: u32) -> bool {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).is_ok()
}

#[cfg(windows)]
pub fn terminate_process(pid: u32) -> bool {
    const PROCESS_TERMINATE: u32 = 0x0001;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut std::ffi::c_void;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
        fn TerminateProcess(hProcess: *mut std::ffi::c_void, uExitCode: u32) -> i32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return false;
        }

        let result = TerminateProcess(handle, 1);
        CloseHandle(handle);
        result != 0
    }
}

#[cfg(not(any(unix, windows)))]
pub fn terminate_process(_pid: u32) -> bool {
    // Cannot terminate on unknown platforms
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_dir() {
        let dir = config_dir();
        assert!(dir.ends_with(".mobilecli"));
    }

    #[test]
    fn test_default_shell() {
        let shell = default_shell();
        assert!(!shell.is_empty());
    }
}
