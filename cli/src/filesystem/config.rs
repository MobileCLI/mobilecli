use std::path::PathBuf;

/// Configuration for file system access
#[derive(Debug, Clone)]
pub struct FileSystemConfig {
    /// Allowed root directories (default: current working directory)
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
        let cwd = std::env::current_dir().ok();
        let home = dirs_next::home_dir();
        let cwd_is_home = cwd.as_ref().is_some_and(|cwd| {
            home.as_ref().is_some_and(|home| {
                cwd.canonicalize().unwrap_or_else(|_| cwd.clone())
                    == home.canonicalize().unwrap_or_else(|_| home.clone())
            })
        });
        let allowed_roots = if cwd_is_home {
            Vec::new()
        } else {
            cwd.into_iter().collect()
        };
        let mut denied_patterns = vec![
            "**/.ssh/*".to_string(),
            "**/*.pem".to_string(),
            "**/*.key".to_string(),
            "**/id_rsa*".to_string(),
            "**/.gnupg/*".to_string(),
            "**/.aws/**".to_string(),
            "**/.kube/**".to_string(),
            "**/.docker/config.json".to_string(),
            "**/.docker/contexts/**".to_string(),
            "**/.config/gcloud/**".to_string(),
            "**/.env".to_string(),
            "**/.env.*".to_string(),
            "**/secrets.*".to_string(),
            "**/*.secret".to_string(),
            "**/token*".to_string(),
            "**/.npmrc".to_string(),
            "**/.pypirc".to_string(),
            "**/.bash_history".to_string(),
            "**/.zsh_history".to_string(),
            "**/.sh_history".to_string(),
            "**/.fish/fish_history".to_string(),
            "**/.local/share/fish/fish_history".to_string(),
            "**/.psql_history".to_string(),
            "**/.python_history".to_string(),
            "**/.node_repl_history".to_string(),
            "**/.irb_history".to_string(),
            "**/.git-credentials".to_string(),
            "**/.config/git/credentials".to_string(),
            "**/.netrc".to_string(),
            "**/.vault-token".to_string(),
            "**/*.tfstate".to_string(),
            "**/*.tfstate.backup".to_string(),
            "**/Library/Keychains/**".to_string(),
            "**/.config/google-chrome/**/Login Data".to_string(),
            "**/.config/chromium/**/Login Data".to_string(),
            "**/.mozilla/firefox/**/*.sqlite".to_string(),
        ];
        if let Some(home) = home.as_ref() {
            let mobilecli_dir = home.join(".mobilecli").to_string_lossy().replace('\\', "/");
            denied_patterns.push(format!("{}/**", mobilecli_dir));
            denied_patterns.push(format!("{}/*", mobilecli_dir));
        }
        Self {
            allowed_roots,
            denied_patterns,
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
                "C:/Windows/**".to_string(),
            ],
            max_list_entries: 10_000,
            max_search_results: 1_000,
        }
    }
}
