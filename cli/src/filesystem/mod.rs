//! File system service for MobileCLI

pub mod config;
pub mod git;
pub mod mime;
pub mod operations;
pub mod platform;
pub mod rate_limit;
pub mod search;
pub mod security;
pub mod watcher;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use config::FileSystemConfig;
use operations::FileOperations;
use search::FileSearch;
use security::PathValidator;
use watcher::FileWatcher;

pub struct FileSystemService {
    config: Arc<FileSystemConfig>,
    validator: Arc<PathValidator>,
    ops: FileOperations,
    search: FileSearch,
    watcher: FileWatcher,
}

impl FileSystemService {
    pub fn new(config: FileSystemConfig) -> Self {
        let config = Arc::new(config);
        let validator = Arc::new(PathValidator::new(config.clone()));
        let ops = FileOperations::new(validator.clone(), config.clone());
        let search = FileSearch::new(ops.clone());
        let watcher = FileWatcher::new(250);
        Self {
            config,
            validator,
            ops,
            search,
            watcher,
        }
    }

    pub fn config(&self) -> &FileSystemConfig {
        self.config.as_ref()
    }

    pub fn validator(&self) -> &PathValidator {
        self.validator.as_ref()
    }

    pub fn ops(&self) -> &FileOperations {
        &self.ops
    }

    pub fn search(&self) -> &FileSearch {
        &self.search
    }

    pub fn watcher(&self) -> &FileWatcher {
        &self.watcher
    }
}
