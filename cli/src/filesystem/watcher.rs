use std::path::PathBuf;

use dashmap::{DashMap, DashSet};
use notify::{RecursiveMode, Watcher};
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, DebouncedEventKind, Debouncer};
use tokio::sync::broadcast;

use crate::protocol::{ChangeType, FileChanged, FileSystemError};

pub struct FileWatcher {
    watchers: DashMap<String, Debouncer<notify::RecommendedWatcher>>,
    known_paths: std::sync::Arc<DashSet<String>>,
    event_tx: broadcast::Sender<FileChanged>,
    debounce_ms: u64,
}

impl FileWatcher {
    pub fn new(debounce_ms: u64) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            watchers: DashMap::new(),
            known_paths: std::sync::Arc::new(DashSet::new()),
            event_tx,
            debounce_ms,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FileChanged> {
        self.event_tx.subscribe()
    }

    pub fn watch(&self, path: &str) -> Result<(), FileSystemError> {
        if self.watchers.contains_key(path) {
            return Ok(());
        }

        let path_buf = PathBuf::from(path);
        let event_tx = self.event_tx.clone();
        let known_paths = self.known_paths.clone();

        known_paths.insert(path_buf.display().to_string());
        if let Ok(entries) = std::fs::read_dir(&path_buf) {
            for entry in entries.flatten() {
                known_paths.insert(entry.path().display().to_string());
            }
        }

        let mut debouncer = new_debouncer(
            std::time::Duration::from_millis(self.debounce_ms),
            move |res: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| {
                if let Ok(events) = res {
                    for event in events {
                        let change = classify_event(&event, &known_paths);
                        let _ = event_tx.send(change);
                    }
                }
            },
        )
        .map_err(|e| FileSystemError::IoError {
            message: e.to_string(),
        })?;

        debouncer
            .watcher()
            .watch(&path_buf, RecursiveMode::NonRecursive)
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        self.watchers.insert(path.to_string(), debouncer);
        Ok(())
    }

    pub fn unwatch(&self, path: &str) -> Result<(), FileSystemError> {
        self.watchers.remove(path);
        Ok(())
    }
}

fn classify_event(event: &DebouncedEvent, known_paths: &DashSet<String>) -> FileChanged {
    let path = event.path.display().to_string();
    let change_type = match event.kind {
        DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous => {
            if event.path.exists() {
                if !known_paths.contains(&path) {
                    known_paths.insert(path.clone());
                    ChangeType::Created
                } else {
                    ChangeType::Modified
                }
            } else {
                known_paths.remove(&path);
                ChangeType::Deleted
            }
        }
    };

    FileChanged {
        path,
        change_type,
        new_entry: None,
    }
}
