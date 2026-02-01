use std::sync::Arc;

use tempfile::TempDir;

use crate::protocol::{SortField, SortOrder};

use super::config::FileSystemConfig;
use super::operations::FileOperations;
use super::security::PathValidator;

#[test]
fn test_path_validation_blocks_traversal() {
    let temp = TempDir::new().unwrap();
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        ..Default::default()
    });
    let validator = PathValidator::new(config);
    let attempted = format!("{}/../blocked", temp.path().display());
    assert!(validator.validate_existing(&attempted).is_err());
}

#[test]
fn test_path_validation_allows_valid_paths() {
    let temp = TempDir::new().unwrap();
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        ..Default::default()
    });
    let validator = PathValidator::new(config);
    let file_path = temp.path().join("test.txt");
    std::fs::write(&file_path, "content").unwrap();
    assert!(validator
        .validate_existing(&file_path.to_string_lossy())
        .is_ok());
}

#[tokio::test]
async fn test_list_directory_sorts_directories_first() {
    let temp = TempDir::new().unwrap();
    std::fs::create_dir(temp.path().join("dir_a")).unwrap();
    std::fs::create_dir(temp.path().join("dir_b")).unwrap();
    std::fs::write(temp.path().join("file_a.txt"), "a").unwrap();
    std::fs::write(temp.path().join("file_b.txt"), "b").unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let (_path, entries, _total, _truncated) = ops
        .list_directory(
            &temp.path().to_string_lossy(),
            false,
            Some(SortField::Name),
            Some(SortOrder::Asc),
        )
        .await
        .unwrap();

    assert!(entries.len() >= 4);
    assert!(entries[0].is_directory);
    assert!(entries[1].is_directory);
    assert!(!entries[2].is_directory);
}

#[tokio::test]
async fn test_write_file_fails_when_parent_is_file() {
    let temp = TempDir::new().unwrap();
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    // Create a file
    let file_path = temp.path().join("existing_file.txt");
    std::fs::write(&file_path, "content").unwrap();

    // Try to write a file with the existing file in the parent path
    let invalid_path = file_path.join("should_fail.txt");
    let result = ops
        .write_file(
            &invalid_path.to_string_lossy(),
            "test content",
            crate::protocol::FileEncoding::Utf8,
            true, // create_parents = true
        )
        .await;

    // Should fail with NotADirectory error
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::protocol::FileSystemError::NotADirectory { path } => {
            assert_eq!(path, file_path.to_string_lossy());
        }
        e => panic!("Expected NotADirectory error, got: {:?}", e),
    }
}
