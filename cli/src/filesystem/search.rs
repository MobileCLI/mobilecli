use std::path::Path;
use std::sync::{Arc, Mutex};

use ignore::WalkBuilder;

use crate::protocol::{ContentMatch, FileEntry, FileSystemError, SearchMatch};

use super::operations::FileOperations;

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

        let walker = WalkBuilder::new(&root)
            .max_depth(max_depth.map(|d| d as usize))
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build_parallel();

        let matches = Arc::new(Mutex::new(Vec::new()));
        let pattern = glob::Pattern::new(pattern).map_err(|e| FileSystemError::IoError {
            message: e.to_string(),
        })?;

        walker.run(|| {
            let matches = Arc::clone(&matches);
            let pattern = pattern.clone();
            let content_pattern = content_pattern.map(|s| s.to_string());

            Box::new(move |entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return ignore::WalkState::Continue,
                };

                if matches.lock().unwrap().len() >= max_results as usize {
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
                if self.ops.validator().is_denied(path) {
                    return ignore::WalkState::Continue;
                }

                let content_matches = if let Some(ref content_pat) = content_pattern {
                    if path.is_file() {
                        search_file_content(path, content_pat)
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Ok(entry_info) = std::fs::metadata(path) {
                    let file_entry = build_file_entry_sync(path, &entry_info, &name);
                    matches.lock().unwrap().push(SearchMatch {
                        path: path.display().to_string(),
                        entry: file_entry,
                        content_matches,
                    });
                }

                ignore::WalkState::Continue
            })
        });

        let matches = Arc::try_unwrap(matches).unwrap().into_inner().unwrap();
        let truncated = matches.len() >= max_results as usize;

        Ok((root.display().to_string(), matches, truncated))
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
        path: path.display().to_string(),
        is_directory,
        is_symlink: metadata.file_type().is_symlink(),
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
        symlink_target: if metadata.file_type().is_symlink() {
            std::fs::read_link(path).ok().map(|p| p.display().to_string())
        } else {
            None
        },
        git_status: None,
    }
}
