use std::path::{Path, PathBuf, MAIN_SEPARATOR};

/// Normalize path separators for cross-platform compatibility
pub fn normalize_path(path: &str) -> PathBuf {
    let normalized = path.replace('\\', "/");
    PathBuf::from(normalized)
}

/// Get path separator for current platform
pub fn path_separator() -> char {
    MAIN_SEPARATOR
}

/// Check if path is absolute (handles both Unix and Windows)
pub fn is_absolute(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        || (cfg!(windows)
            && (path
                .to_string_lossy()
                .chars()
                .nth(1)
                == Some(':')
                || path.to_string_lossy().starts_with("\\\\")))
}

/// Get home directory cross-platform
pub fn home_dir() -> Option<PathBuf> {
    dirs_next::home_dir()
}

#[cfg(unix)]
pub fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn is_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    if let Ok(metadata) = path.metadata() {
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        return metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(unix)]
pub fn format_permissions(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode();
    let user = format_rwx((mode >> 6) & 0o7);
    let group = format_rwx((mode >> 3) & 0o7);
    let other = format_rwx(mode & 0o7);

    format!("{}{}{}", user, group, other)
}

#[cfg(windows)]
pub fn format_permissions(metadata: &std::fs::Metadata) -> String {
    let readonly = metadata.permissions().readonly();
    if readonly { "r--" } else { "rw-" }.to_string()
}

fn format_rwx(bits: u32) -> String {
    format!(
        "{}{}{}",
        if bits & 4 != 0 { "r" } else { "-" },
        if bits & 2 != 0 { "w" } else { "-" },
        if bits & 1 != 0 { "x" } else { "-" },
    )
}
