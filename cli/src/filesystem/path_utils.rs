use std::path::{Path, PathBuf};
use crate::protocol::FileSystemError;

/// Check if a path component exists and is a file (not a directory)
/// This helps detect when a path like /path/to/file.txt/newdir is invalid
pub async fn validate_parent_components(path: &Path) -> Result<(), FileSystemError> {
    let mut current = PathBuf::new();
    
    for component in path.components() {
        current.push(component);
        
        if current.exists() && current.is_file() {
            // We found a file in the path where a directory should be
            return Err(FileSystemError::NotADirectory {
                path: current.display().to_string(),
            });
        }
    }
    
    Ok(())
}

/// Safely create parent directories, checking that no path component is a file
pub async fn create_parent_dirs_safe(path: &Path) -> Result<(), FileSystemError> {
    if let Some(parent) = path.parent() {
        // First validate that no component in the path is a file
        validate_parent_components(parent).await?;
        
        // Now safe to create directories
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| {
                // Check if error is ENOTDIR (error 20)
                if let Some(20) = e.raw_os_error() {
                    // Find which component is the file
                    let mut current = PathBuf::new();
                    for component in parent.components() {
                        current.push(component);
                        if current.exists() && current.is_file() {
                            return FileSystemError::NotADirectory {
                                path: current.display().to_string(),
                            };
                        }
                    }
                }
                FileSystemError::IoError {
                    message: e.to_string(),
                }
            })?;
    }
    
    Ok(())
}