use std::ffi::OsString;
use std::path::Path;

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::protocol::{
    FileContent, FileEncoding, FileEntry, FileSystemError, GitStatus, SortField, SortOrder,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use super::config::FileSystemConfig;
use super::mime;
use super::platform;
use super::security::PathValidator;

#[derive(Clone)]
pub struct FileOperations {
    validator: std::sync::Arc<PathValidator>,
    config: std::sync::Arc<FileSystemConfig>,
}

impl FileOperations {
    pub fn new(
        validator: std::sync::Arc<PathValidator>,
        config: std::sync::Arc<FileSystemConfig>,
    ) -> Self {
        Self { validator, config }
    }

    pub fn validator(&self) -> &PathValidator {
        &self.validator
    }

    /// List directory contents
    pub async fn list_directory(
        &self,
        path: &str,
        include_hidden: bool,
        sort_by: Option<SortField>,
        sort_order: Option<SortOrder>,
    ) -> Result<(String, Vec<FileEntry>, usize, bool), FileSystemError> {
        let path = self.validator.validate_existing(path)?;

        if !path.is_dir() {
            return Err(FileSystemError::NotADirectory {
                path: path.display().to_string(),
            });
        }

        let mut entries = Vec::new();
        let git_statuses = super::git::status_map_for_path(&path).await;
        let mut read_dir = fs::read_dir(&path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?
        {
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if !include_hidden && platform::is_hidden(&entry_path) {
                continue;
            }
            if self.validator.is_denied(&entry_path) {
                continue;
            }
            let git_status = git_statuses
                .as_ref()
                .and_then(|map| map.get(&entry_path).cloned());
            match self.build_file_entry(&entry_path, &name, git_status).await {
                Ok(file_entry) => entries.push(file_entry),
                Err(_) => continue,
            }
        }

        sort_entries(&mut entries, sort_by, sort_order);

        let total_count = entries.len();
        let truncated = total_count > self.config.max_list_entries;
        if truncated {
            entries.truncate(self.config.max_list_entries);
        }

        Ok((path.display().to_string(), entries, total_count, truncated))
    }

    /// Read file contents
    pub async fn read_file(
        &self,
        path: &str,
        offset: Option<u64>,
        length: Option<u64>,
        encoding: FileEncoding,
    ) -> Result<FileContent, FileSystemError> {
        let path = self.validator.validate_existing(path)?;

        if !path.is_file() {
            return Err(FileSystemError::NotAFile {
                path: path.display().to_string(),
            });
        }

        let metadata = fs::metadata(&path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        let size = metadata.len();

        if size > self.config.max_read_size {
            return Err(FileSystemError::FileTooLarge {
                path: path.display().to_string(),
                size,
                max_size: self.config.max_read_size,
            });
        }

        let mut file = fs::File::open(&path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        let start = offset.unwrap_or(0);
        if start > size {
            return Err(FileSystemError::IoError {
                message: "Offset beyond end of file".to_string(),
            });
        }

        if start > 0 {
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        }

        let remaining = size - start;
        let read_length = length.unwrap_or(remaining).min(remaining);
        let mut buffer = vec![0u8; read_length as usize];
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;
        buffer.truncate(bytes_read);

        let mime_type = mime::detect_mime_type(&buffer, path.to_string_lossy().as_ref());

        let (content, actual_encoding) = match encoding {
            FileEncoding::Utf8 => {
                if mime::is_text_mime(&mime_type) {
                    if let Some(text) = decode_text_buffer(&buffer) {
                        (text, FileEncoding::Utf8)
                    } else {
                        (String::from_utf8_lossy(&buffer).to_string(), FileEncoding::Utf8)
                    }
                } else {
                    (BASE64.encode(&buffer), FileEncoding::Base64)
                }
            }
            FileEncoding::Base64 => (BASE64.encode(&buffer), FileEncoding::Base64),
        };

        let modified = metadata
            .modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64)
            .unwrap_or(0);

        Ok(FileContent {
            path: path.display().to_string(),
            content,
            encoding: actual_encoding,
            mime_type,
            size,
            modified,
            truncated_at: if bytes_read < read_length as usize {
                Some(bytes_read as u64)
            } else {
                None
            },
        })
    }

    pub async fn read_file_chunk(
        &self,
        path: &str,
        chunk_index: u64,
        chunk_size: u64,
    ) -> Result<(String, u64, u64, u64, String, String, bool), FileSystemError> {
        if chunk_size == 0 {
            return Err(FileSystemError::IoError {
                message: "Chunk size must be greater than zero".to_string(),
            });
        }
        let path = self.validator.validate_existing(path)?;

        if !path.is_file() {
            return Err(FileSystemError::NotAFile {
                path: path.display().to_string(),
            });
        }

        let metadata = fs::metadata(&path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        let size = metadata.len();

        if size > self.config.max_read_size {
            return Err(FileSystemError::FileTooLarge {
                path: path.display().to_string(),
                size,
                max_size: self.config.max_read_size,
            });
        }

        let total_chunks = if size == 0 {
            1
        } else {
            (size + chunk_size - 1) / chunk_size
        };

        let offset = chunk_index.saturating_mul(chunk_size);
        if offset >= size && size != 0 {
            return Err(FileSystemError::NotFound {
                path: path.display().to_string(),
            });
        }

        let mut file = fs::File::open(&path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        }

        let read_len = if size == 0 { 0 } else { chunk_size.min(size - offset) };
        let mut buffer = vec![0u8; read_len as usize];
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;
        buffer.truncate(bytes_read);

        let checksum = format!("{:x}", md5::compute(&buffer));
        let data = BASE64.encode(&buffer);
        let is_last = chunk_index + 1 >= total_chunks;

        Ok((
            path.display().to_string(),
            total_chunks,
            size,
            chunk_index,
            data,
            checksum,
            is_last,
        ))
    }

    /// Write file contents
    pub async fn write_file(
        &self,
        path: &str,
        content: &str,
        encoding: FileEncoding,
        create_parents: bool,
    ) -> Result<(), FileSystemError> {
        let path = self.validator.resolve_new_path(path, create_parents)?;

        if !self.validator.is_writable(&path) {
            return Err(FileSystemError::PermissionDenied {
                path: path.display().to_string(),
                reason: "Path is read-only".to_string(),
            });
        }

        if path.exists() && path.is_dir() {
            return Err(FileSystemError::NotAFile {
                path: path.display().to_string(),
            });
        }

        let bytes = match encoding {
            FileEncoding::Utf8 => content.as_bytes().to_vec(),
            FileEncoding::Base64 => BASE64.decode(content).map_err(|_| FileSystemError::InvalidEncoding {
                path: path.display().to_string(),
            })?,
        };

        if bytes.len() as u64 > self.config.max_write_size {
            return Err(FileSystemError::FileTooLarge {
                path: path.display().to_string(),
                size: bytes.len() as u64,
                max_size: self.config.max_write_size,
            });
        }

        if create_parents {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|e| FileSystemError::IoError {
                        message: e.to_string(),
                    })?;
            }
        }

        let temp_path = sibling_with_suffix(&path, &format!("tmp-{}", uuid::Uuid::new_v4()));

        if let Err(e) = fs::write(&temp_path, &bytes).await {
            return Err(FileSystemError::IoError {
                message: e.to_string(),
            });
        }

        let mut backup_path = None;
        if path.exists() {
            let backup = sibling_with_suffix(&path, "bak");
            let _ = fs::remove_file(&backup).await;
            if let Err(e) = fs::rename(&path, &backup).await {
                let _ = fs::remove_file(&temp_path).await;
                return Err(FileSystemError::IoError {
                    message: format!("Failed to backup existing file: {}", e),
                });
            }
            backup_path = Some(backup);
        }

        if let Err(e) = fs::rename(&temp_path, &path).await {
            let mut restore_error = None;
            if let Some(ref backup) = backup_path {
                if let Err(restore) = fs::rename(backup, &path).await {
                    if let Err(copy_err) = fs::copy(backup, &path).await {
                        restore_error = Some(format!(
                            "Failed to restore backup at {}: {}; copy failed: {}",
                            backup.display(),
                            restore,
                            copy_err
                        ));
                    } else {
                        restore_error = Some(format!(
                            "Restored from backup copy after rename failure; backup retained at {}",
                            backup.display()
                        ));
                    }
                }
            }
            let _ = fs::remove_file(&temp_path).await;
            let message = if let Some(restore_error) = restore_error {
                format!("Failed to replace file: {}; {}", e, restore_error)
            } else {
                e.to_string()
            };
            return Err(FileSystemError::IoError { message });
        }

        if let Some(backup) = backup_path {
            let _ = fs::remove_file(backup).await;
        }

        Ok(())
    }

    /// Create directory
    pub async fn create_directory(&self, path: &str, recursive: bool) -> Result<(), FileSystemError> {
        let path = self.validator.resolve_new_path(path, recursive)?;

        if path.exists() {
            return Err(FileSystemError::AlreadyExists {
                path: path.display().to_string(),
            });
        }

        if recursive {
            fs::create_dir_all(&path)
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        } else {
            fs::create_dir(&path)
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        }

        Ok(())
    }

    /// Delete file or directory
    pub async fn delete_path(&self, path: &str, recursive: bool) -> Result<(), FileSystemError> {
        let path = self.validator.validate_existing(path)?;

        if !self.validator.is_writable(&path) {
            return Err(FileSystemError::PermissionDenied {
                path: path.display().to_string(),
                reason: "Path is read-only".to_string(),
            });
        }

        if path.is_dir() {
            if !recursive {
                let mut read_dir = fs::read_dir(&path)
                    .await
                    .map_err(|e| FileSystemError::IoError {
                        message: e.to_string(),
                    })?;
                if read_dir.next_entry().await.map_err(|e| FileSystemError::IoError { message: e.to_string() })?.is_some() {
                    return Err(FileSystemError::NotEmpty {
                        path: path.display().to_string(),
                    });
                }
                fs::remove_dir(&path)
                    .await
                    .map_err(|e| FileSystemError::IoError {
                        message: e.to_string(),
                    })?;
            } else {
                fs::remove_dir_all(&path)
                    .await
                    .map_err(|e| FileSystemError::IoError {
                        message: e.to_string(),
                    })?;
            }
        } else {
            fs::remove_file(&path)
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        }

        Ok(())
    }

    /// Rename file or directory
    pub async fn rename_path(&self, old_path: &str, new_path: &str) -> Result<(), FileSystemError> {
        let old_path = self.validator.validate_existing(old_path)?;
        let new_path = self.validator.resolve_new_path(new_path, false)?;

        if !self.validator.is_writable(&old_path) || !self.validator.is_writable(&new_path) {
            return Err(FileSystemError::PermissionDenied {
                path: new_path.display().to_string(),
                reason: "Path is read-only".to_string(),
            });
        }

        if new_path.exists() {
            return Err(FileSystemError::AlreadyExists {
                path: new_path.display().to_string(),
            });
        }

        fs::rename(&old_path, &new_path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;

        Ok(())
    }

    /// Copy file or directory
    pub async fn copy_path(&self, source: &str, destination: &str, recursive: bool) -> Result<(), FileSystemError> {
        let source = self.validator.validate_existing(source)?;
        let destination = self.validator.resolve_new_path(destination, recursive)?;

        if destination.exists() {
            return Err(FileSystemError::AlreadyExists {
                path: destination.display().to_string(),
            });
        }

        if source.is_dir() {
            if !recursive {
                return Err(FileSystemError::NotADirectory {
                    path: source.display().to_string(),
                });
            }
            copy_dir_recursive(&source, &destination).await?;
        } else {
            fs::copy(&source, &destination)
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        }

        Ok(())
    }

    /// Get file info
    pub async fn get_file_info(&self, path: &str) -> Result<FileEntry, FileSystemError> {
        let path = self.validator.validate_existing(path)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let git_status = super::git::status_for_path(&path).await;
        self.build_file_entry(&path, &name, git_status).await
    }

    pub async fn build_file_entry(
        &self,
        path: &Path,
        name: &str,
        git_status: Option<GitStatus>,
    ) -> Result<FileEntry, FileSystemError> {
        let metadata = fs::symlink_metadata(path)
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;
        let is_symlink = metadata.file_type().is_symlink();
        let file_metadata = fs::metadata(path)
            .await
            .unwrap_or(metadata.clone());

        let is_directory = file_metadata.is_dir();
        let size = if is_directory { 0 } else { file_metadata.len() };
        let modified = file_metadata
            .modified()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64)
            .unwrap_or(0);
        let created = file_metadata
            .created()
            .ok()
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64);

        let is_hidden = platform::is_hidden(path);
        let permissions = Some(platform::format_permissions(&file_metadata));

        let mime_type = if is_directory {
            None
        } else {
            Some(mime::guess_mime_from_extension(name))
        };

        let symlink_target = if is_symlink {
            std::fs::read_link(path).ok().map(|p| p.display().to_string())
        } else {
            None
        };

        Ok(FileEntry {
            name: name.to_string(),
            path: path.display().to_string(),
            is_directory,
            is_symlink,
            is_hidden,
            size,
            modified,
            created,
            mime_type,
            permissions,
            symlink_target,
            git_status,
        })
    }
}

fn decode_text_buffer(buffer: &[u8]) -> Option<String> {
    if let Ok(content) = std::str::from_utf8(buffer) {
        return Some(content.to_string());
    }

    if buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(content) = std::str::from_utf8(&buffer[3..]) {
            return Some(content.to_string());
        }
    }

    if buffer.starts_with(&[0xFF, 0xFE]) {
        let utf16: Vec<u16> = buffer[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Ok(content) = String::from_utf16(&utf16) {
            return Some(content);
        }
    }

    if buffer.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = buffer[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        if let Ok(content) = String::from_utf16(&utf16) {
            return Some(content);
        }
    }

    None
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut file_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("file"));
    file_name.push(".");
    file_name.push(suffix);
    path.with_file_name(file_name)
}

fn sort_entries(entries: &mut [FileEntry], sort_by: Option<SortField>, sort_order: Option<SortOrder>) {
    use std::cmp::Ordering;

    let sort_by = sort_by.unwrap_or(SortField::Name);
    let sort_order = sort_order.unwrap_or(SortOrder::Asc);

    entries.sort_by(|a, b| {
        let dir_cmp = b.is_directory.cmp(&a.is_directory);
        if dir_cmp != Ordering::Equal {
            return dir_cmp;
        }

        let ord = match sort_by {
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Size => a.size.cmp(&b.size),
            SortField::Modified => a.modified.cmp(&b.modified),
            SortField::Type => extension_of(&a.name).cmp(&extension_of(&b.name)),
        };

        match sort_order {
            SortOrder::Asc => ord,
            SortOrder::Desc => ord.reverse(),
        }
    });
}

fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), FileSystemError> {
    fs::create_dir_all(dst)
        .await
        .map_err(|e| FileSystemError::IoError {
            message: e.to_string(),
        })?;

    let mut read_dir = fs::read_dir(src)
        .await
        .map_err(|e| FileSystemError::IoError {
            message: e.to_string(),
        })?;

    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|e| FileSystemError::IoError {
            message: e.to_string(),
        })?
    {
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        let meta = entry
            .metadata()
            .await
            .map_err(|e| FileSystemError::IoError {
                message: e.to_string(),
            })?;
        if meta.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path).await?;
        } else {
            fs::copy(&entry_path, &dest_path)
                .await
                .map_err(|e| FileSystemError::IoError {
                    message: e.to_string(),
                })?;
        }
    }

    Ok(())
}
