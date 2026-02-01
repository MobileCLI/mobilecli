use std::path::{Component, Path, PathBuf};

use glob_match::glob_match;
use path_jail::Jail;

use crate::protocol::FileSystemError;

use super::config::FileSystemConfig;

/// Validates and sanitizes paths before any file operation
pub struct PathValidator {
    config: std::sync::Arc<FileSystemConfig>,
    jails: Vec<Jail>,
    symlink_cache: std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>,
}

impl PathValidator {
    pub fn new(config: std::sync::Arc<FileSystemConfig>) -> Self {
        let jails = config
            .allowed_roots
            .iter()
            .filter_map(|root| Jail::new(root).ok())
            .collect();
        Self {
            config,
            jails,
            symlink_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Validate a path that must already exist
    pub fn validate_existing(&self, path: &str) -> Result<PathBuf, FileSystemError> {
        let path = Path::new(path);

        if !path.is_absolute() || contains_parent_dir(path) {
            return Err(FileSystemError::PathTraversal {
                attempted_path: path.display().to_string(),
            });
        }

        let canonical = path
            .canonicalize()
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        self.ensure_allowed(&canonical)?;
        self.ensure_not_denied(&canonical)?;

        if !self.config.follow_symlinks && self.contains_symlink(&canonical) {
            return Err(FileSystemError::PermissionDenied {
                path: canonical.display().to_string(),
                reason: "Symlinked paths are not allowed".to_string(),
            });
        }

        Ok(canonical)
    }

    /// Resolve a path that may not exist yet (e.g. create/rename targets)
    pub fn resolve_new_path(&self, path: &str, allow_missing_parents: bool) -> Result<PathBuf, FileSystemError> {
        let path = Path::new(path);

        if !path.is_absolute() || contains_parent_dir(path) {
            return Err(FileSystemError::PathTraversal {
                attempted_path: path.display().to_string(),
            });
        }

        let parent = path.parent().ok_or_else(|| FileSystemError::PathTraversal {
            attempted_path: path.display().to_string(),
        })?;

        // Check if any component in the parent path exists as a file when it should be a directory
        let mut current = PathBuf::new();
        for component in parent.components() {
            current.push(component);
            if current.exists() && current.is_file() {
                return Err(FileSystemError::NotADirectory {
                    path: current.display().to_string(),
                });
            }
        }

        if !allow_missing_parents && !parent.exists() {
            return Err(FileSystemError::NotFound {
                path: parent.display().to_string(),
            });
        }

        let existing_ancestor = find_existing_ancestor(path).ok_or_else(|| FileSystemError::NotFound {
            path: path.display().to_string(),
        })?;

        let canonical_ancestor = existing_ancestor
            .canonicalize()
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        self.ensure_allowed(&canonical_ancestor)?;
        self.ensure_not_denied(&canonical_ancestor)?;

        if !self.config.follow_symlinks && self.contains_symlink(&canonical_ancestor) {
            return Err(FileSystemError::PermissionDenied {
                path: canonical_ancestor.display().to_string(),
                reason: "Symlinked paths are not allowed".to_string(),
            });
        }

        let relative = path
            .strip_prefix(&existing_ancestor)
            .unwrap_or_else(|_| Path::new(""));
        let resolved = canonical_ancestor.join(relative);

        self.ensure_not_denied(&resolved)?;

        Ok(resolved)
    }

    /// Check if path is writable (not in read-only patterns)
    pub fn is_writable(&self, path: &Path) -> bool {
        let normalized = normalize_for_match(path);
        for pattern in &self.config.read_only_patterns {
            if glob_match(pattern, &normalized) {
                return false;
            }
        }
        true
    }

    /// Check if path matches denied patterns
    pub fn is_denied(&self, path: &Path) -> bool {
        let normalized = normalize_for_match(path);
        self.config
            .denied_patterns
            .iter()
            .any(|pattern| glob_match(pattern, &normalized))
    }

    fn ensure_allowed(&self, path: &Path) -> Result<(), FileSystemError> {
        let is_allowed = self.jails.iter().any(|jail| jail.contains(path).is_ok());

        if !is_allowed {
            return Err(FileSystemError::PermissionDenied {
                path: path.display().to_string(),
                reason: "Path is outside allowed directories".to_string(),
            });
        }

        Ok(())
    }

    fn ensure_not_denied(&self, path: &Path) -> Result<(), FileSystemError> {
        let normalized = normalize_for_match(path);
        for pattern in &self.config.denied_patterns {
            if glob_match(pattern, &normalized) {
                return Err(FileSystemError::PermissionDenied {
                    path: path.display().to_string(),
                    reason: format!("Path matches denied pattern: {}", pattern),
                });
            }
        }
        Ok(())
    }

    fn contains_symlink(&self, path: &Path) -> bool {
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component.as_os_str());
            if let Some(cached) = self.symlink_cache.lock().ok().and_then(|cache| cache.get(&current).cloned()) {
                if cached {
                    return true;
                }
                continue;
            }
            let is_symlink = match std::fs::symlink_metadata(&current) {
                Ok(meta) => meta.file_type().is_symlink(),
                Err(_) => false,
            };
            if let Ok(mut cache) = self.symlink_cache.lock() {
                cache.insert(current.clone(), is_symlink);
            }
            if is_symlink {
                return true;
            }
        }
        false
    }
}

fn contains_parent_dir(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

fn normalize_for_match(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn find_existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.exists()).map(|p| p.to_path_buf())
}
