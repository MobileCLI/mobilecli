use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ignore::WalkBuilder;

use crate::protocol::{ContentMatch, FileEntry, FileSystemError, SearchMatch};

use super::operations::FileOperations;
use super::path_utils;

const MAX_CONTENT_MATCHES_PER_FILE: usize = 20;

#[derive(Clone)]
pub struct FileSearch {
    ops: FileOperations,
}

impl FileSearch {
    pub fn new(ops: FileOperations) -> Self {
        Self { ops }
    }

    pub async fn search_files(
        &self,
        path: &str,
        pattern: &str,
        content_pattern: Option<&str>,
        max_depth: Option<u32>,
        max_results: u32,
    ) -> Result<(String, Vec<SearchMatch>, bool), FileSystemError> {
        let root = self.ops.validator().validate_existing(path)?;
        let max_read_size = self.ops.config().max_read_size;

        let walker = WalkBuilder::new(&root)
            .max_depth(max_depth.map(|d| d as usize))
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build_parallel();

        let matches = Arc::new(Mutex::new(Vec::new()));
        let match_count = Arc::new(AtomicUsize::new(0));
        let pattern = glob::Pattern::new(pattern).map_err(|e| FileSystemError::IoError {
            message: e.to_string(),
        })?;

        walker.run(|| {
            let matches = Arc::clone(&matches);
            let match_count = Arc::clone(&match_count);
            let pattern = pattern.clone();
            let content_pattern = content_pattern.map(|s| s.to_string());

            Box::new(move |entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                if match_count.load(Ordering::Relaxed) >= max_results as usize {
                    return ignore::WalkState::Quit;
                }

                let path = entry.path();
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if !pattern.matches(&name) {
                    return ignore::WalkState::Continue;
                }

                // Validate per-entry to enforce allowlist + denied patterns and prevent following symlinks.
                let canonical = match self
                    .ops
                    .validator()
                    .validate_existing(path.to_string_lossy().as_ref())
                {
                    Ok(p) => p,
                    Err(_) => return ignore::WalkState::Continue,
                };

                if self.ops.validator().is_denied(&canonical) {
                    return ignore::WalkState::Continue;
                }

                let content_matches = if let Some(ref content_pat) = content_pattern {
                    if canonical.is_file() {
                        if let Ok(meta) = std::fs::metadata(&canonical) {
                            // Avoid loading huge files into memory during search.
                            if meta.len() <= max_read_size {
                                search_file_content(&canonical, content_pat)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Ok(entry_info) = std::fs::metadata(&canonical) {
                    let mut reserved = false;
                    loop {
                        let current = match_count.load(Ordering::Relaxed);
                        if current >= max_results as usize {
                            break;
                        }
                        if match_count
                            .compare_exchange(
                                current,
                                current + 1,
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            )
                            .is_ok()
                        {
                            reserved = true;
                            break;
                        }
                    }

                    if !reserved {
                        return ignore::WalkState::Quit;
                    }

                    let file_entry = build_file_entry_sync(&canonical, &entry_info, &name);
                    matches.lock().unwrap().push(SearchMatch {
                        path: path_utils::to_protocol_path(&canonical),
                        entry: file_entry,
                        content_matches,
                    });
                }

                ignore::WalkState::Continue
            })
        });

        let matches = Arc::try_unwrap(matches).unwrap().into_inner().unwrap();
        let truncated = matches.len() >= max_results as usize;

        Ok((path_utils::to_protocol_path(&root), matches, truncated))
    }
}

fn search_file_content(path: &Path, pattern: &str) -> Option<Vec<ContentMatch>> {
    let data = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&data);

    let mut matches = Vec::new();

    for (i, line) in text.lines().enumerate() {
        if matches.len() >= MAX_CONTENT_MATCHES_PER_FILE {
            break;
        }
        if let Some(start) = line.find(pattern) {
            matches.push(ContentMatch {
                line_number: (i + 1) as u32,
                line_content: line.to_string(),
                match_start: start as u32,
                match_end: (start + pattern.len()) as u32,
            });
        }
    }

    if matches.is_empty() {
        None
    } else {
        Some(matches)
    }
}

fn build_file_entry_sync(path: &Path, metadata: &std::fs::Metadata, name: &str) -> FileEntry {
    let is_directory = metadata.is_dir();
    let size = if is_directory { 0 } else { metadata.len() };
    let modified = metadata
        .modified()
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64)
        .unwrap_or(0);
    let created = metadata
        .created()
        .ok()
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64);

    FileEntry {
        name: name.to_string(),
        path: path_utils::to_protocol_path(path),
        is_directory,
        is_symlink: false,
        is_hidden: super::platform::is_hidden(path),
        size,
        modified,
        created,
        mime_type: if is_directory {
            None
        } else {
            Some(super::mime::guess_mime_from_extension(name))
        },
        permissions: Some(super::platform::format_permissions(metadata)),
        symlink_target: None,
        git_status: None,
    }
}
