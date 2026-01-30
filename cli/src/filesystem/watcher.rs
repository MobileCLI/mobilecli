use std::path::PathBuf;

use dashmap::DashMap;
use notify::{RecursiveMode, Watcher};
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, DebouncedEventKind, Debouncer};
use tokio::sync::broadcast;

use crate::protocol::{ChangeType, FileChanged, FileSystemError};

pub struct FileWatcher {
    watchers: DashMap<String, Debouncer<notify::RecommendedWatcher>>,
    event_tx: broadcast::Sender<FileChanged>,
    debounce_ms: u64,
}

impl FileWatcher {
    pub fn new(debounce_ms: u64) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            watchers: DashMap::new(),
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

        let mut debouncer = new_debouncer(
            std::time::Duration::from_millis(self.debounce_ms),
            move |res: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| {
                if let Ok(events) = res {
                    for event in events {
                        let change = classify_event(&event);
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

fn classify_event(event: &DebouncedEvent) -> FileChanged {
    let path = event.path.display().to_string();
    let change_type = match event.kind {
        DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous => {
            if event.path.exists() {
                let is_recent = event
                    .path
                    .metadata()
                    .ok()
                    .and_then(|m| m.created().ok())
                    .and_then(|t| t.elapsed().ok())
                    .map(|d| d.as_secs() < 2)
                    .unwrap_or(false);
                if is_recent {
                    ChangeType::Created
                } else {
                    ChangeType::Modified
                }
            } else {
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
