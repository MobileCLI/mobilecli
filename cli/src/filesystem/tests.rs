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

#[tokio::test]
async fn test_write_file_overwrite_existing_file_succeeds() {
    let temp = TempDir::new().unwrap();
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let file_path = temp.path().join("overwrite.txt");
    let path = file_path.to_string_lossy().to_string();

    ops.write_file(&path, "first", crate::protocol::FileEncoding::Utf8, true)
        .await
        .unwrap();
    ops.write_file(&path, "second", crate::protocol::FileEncoding::Utf8, true)
        .await
        .unwrap();

    let final_content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(final_content, "second");
}

#[tokio::test]
async fn test_create_directory_respects_read_only_patterns() {
    let temp = TempDir::new().unwrap();
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        read_only_patterns: vec![format!("{}/**", temp.path().display())],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let dir_path = temp.path().join("newdir");
    let result = ops
        .create_directory(&dir_path.to_string_lossy(), true)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        crate::protocol::FileSystemError::PermissionDenied { .. } => {}
        e => panic!("Expected PermissionDenied, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_copy_path_respects_read_only_patterns() {
    let temp = TempDir::new().unwrap();
    let src = temp.path().join("src.txt");
    std::fs::write(&src, "hi").unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        read_only_patterns: vec![format!("{}/**", temp.path().display())],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let dest = temp.path().join("dest.txt");
    let result = ops
        .copy_path(&src.to_string_lossy(), &dest.to_string_lossy(), false)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        crate::protocol::FileSystemError::PermissionDenied { .. } => {}
        e => panic!("Expected PermissionDenied, got: {:?}", e),
    }
}

#[cfg(unix)]
#[test]
fn test_path_validation_blocks_symlink_paths_when_disabled() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let real_dir = temp.path().join("real");
    std::fs::create_dir_all(&real_dir).unwrap();

    let link_dir = temp.path().join("link");
    symlink(&real_dir, &link_dir).unwrap();

    let file_path = real_dir.join("file.txt");
    std::fs::write(&file_path, "content").unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        follow_symlinks: false,
        ..Default::default()
    });
    let validator = PathValidator::new(config);

    let via_link = link_dir.join("file.txt");
    let err = validator
        .validate_existing(&via_link.to_string_lossy())
        .unwrap_err();

    match err {
        crate::protocol::FileSystemError::PermissionDenied { .. } => {}
        other => panic!("expected PermissionDenied, got: {:?}", other),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_copy_directory_blocks_symlink_entries() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();

    let src_dir = temp.path().join("srcdir");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("a.txt"), "a").unwrap();

    // Symlink inside the directory should be rejected for recursive copy.
    let target = temp.path().join("real_target.txt");
    std::fs::write(&target, "target").unwrap();
    symlink(&target, src_dir.join("link.txt")).unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        follow_symlinks: false,
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let dest_dir = temp.path().join("destdir");
    let result = ops
        .copy_path(
            &src_dir.to_string_lossy(),
            &dest_dir.to_string_lossy(),
            true,
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        crate::protocol::FileSystemError::PermissionDenied { .. } => {}
        e => panic!("Expected PermissionDenied, got: {:?}", e),
    }
}
