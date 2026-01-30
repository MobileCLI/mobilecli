use std::path::PathBuf;

/// Configuration for file system access
#[derive(Debug, Clone)]
pub struct FileSystemConfig {
    /// Allowed root directories (default: home directory)
    pub allowed_roots: Vec<PathBuf>,

    /// Denied file patterns (glob)
    pub denied_patterns: Vec<String>,

    /// Maximum file size for read operations (bytes)
    pub max_read_size: u64,

    /// Maximum file size for write operations (bytes)
    pub max_write_size: u64,

    /// Whether to follow symlinks
    pub follow_symlinks: bool,

    /// Read-only paths (can read but not modify)
    pub read_only_patterns: Vec<String>,

    /// Maximum entries in a directory listing before truncation
    pub max_list_entries: usize,

    /// Maximum search results
    pub max_search_results: u32,
}

impl Default for FileSystemConfig {
    fn default() -> Self {
        let home = dirs_next::home_dir().or_else(|| std::env::current_dir().ok());
        Self {
            allowed_roots: home.into_iter().collect(),
            denied_patterns: vec![
                "**/.ssh/*".to_string(),
                "**/*.pem".to_string(),
                "**/*.key".to_string(),
                "**/id_rsa*".to_string(),
                "**/.gnupg/*".to_string(),
                "**/.aws/credentials".to_string(),
                "**/.env".to_string(),
                "**/.env.*".to_string(),
                "**/secrets.*".to_string(),
                "**/*.secret".to_string(),
                "**/token*".to_string(),
                "**/.npmrc".to_string(),
                "**/.pypirc".to_string(),
            ],
            max_read_size: 50 * 1024 * 1024,
            max_write_size: 50 * 1024 * 1024,
            follow_symlinks: false,
            read_only_patterns: vec![
                "/etc/**".to_string(),
                "/usr/**".to_string(),
                "/bin/**".to_string(),
                "/sbin/**".to_string(),
                "/System/**".to_string(),
                "/Library/**".to_string(),
                "C:\\Windows\\**".to_string(),
            ],
            max_list_entries: 10_000,
            max_search_results: 1_000,
        }
    }
}
