use std::sync::Arc;

use tempfile::TempDir;

use crate::protocol::{SortField, SortOrder};

use super::config::FileSystemConfig;
use super::operations::FileOperations;
use super::security::PathValidator;

fn temp_root(temp: &TempDir) -> std::path::PathBuf {
    temp.path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf())
}

#[test]
fn test_path_validation_blocks_traversal() {
    let temp = TempDir::new().unwrap();
    let root = temp_root(&temp);
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        ..Default::default()
    });
    let validator = PathValidator::new(config);
    let attempted = format!("{}/../blocked", root.display());
    assert!(validator.validate_existing(&attempted).is_err());
}

#[test]
fn test_path_validation_allows_valid_paths() {
    let temp = TempDir::new().unwrap();
    let root = temp_root(&temp);
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        ..Default::default()
    });
    let validator = PathValidator::new(config);
    let file_path = root.join("test.txt");
    std::fs::write(&file_path, "content").unwrap();
    assert!(validator
        .validate_existing(&file_path.to_string_lossy())
        .is_ok());
}

#[test]
fn test_path_validation_blocks_mobilecli_config_secrets() {
    let Some(home) = dirs_next::home_dir() else {
        return;
    };

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![home.clone()],
        ..Default::default()
    });
    let validator = PathValidator::new(config);
    let config_path = home.join(".mobilecli").join("config.json");
    assert!(validator.is_denied(&config_path));
}

#[test]
fn test_path_validation_allows_project_mobilecli_upload_cache() {
    let temp = TempDir::new().unwrap();
    let root = temp_root(&temp);
    let upload_path = root.join(".mobilecli/uploads/image.png");
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root],
        ..Default::default()
    });
    let validator = PathValidator::new(config);
    assert!(!validator.is_denied(&upload_path));
}

#[tokio::test]
async fn test_list_directory_sorts_directories_first() {
    let temp = TempDir::new().unwrap();
    let root = temp_root(&temp);
    std::fs::create_dir(root.join("dir_a")).unwrap();
    std::fs::create_dir(root.join("dir_b")).unwrap();
    std::fs::write(root.join("file_a.txt"), "a").unwrap();
    std::fs::write(root.join("file_b.txt"), "b").unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let (_path, entries, _total, _truncated) = ops
        .list_directory(
            &root.to_string_lossy(),
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
    let root = temp_root(&temp);
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    // Create a file
    let file_path = root.join("existing_file.txt");
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
            assert_eq!(path, super::path_utils::to_protocol_path(&file_path));
        }
        e => panic!("Expected NotADirectory error, got: {:?}", e),
    }
}

#[tokio::test]
async fn test_write_file_overwrite_existing_file_succeeds() {
    let temp = TempDir::new().unwrap();
    let root = temp_root(&temp);
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let file_path = root.join("overwrite.txt");
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
    let root = temp_root(&temp);
    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        read_only_patterns: vec![format!("{}/**", root.display())],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let dir_path = root.join("newdir");
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
    let root = temp_root(&temp);
    let src = root.join("src.txt");
    std::fs::write(&src, "hi").unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        read_only_patterns: vec![format!("{}/**", root.display())],
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let dest = root.join("dest.txt");
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
    let root = temp_root(&temp);
    let real_dir = root.join("real");
    std::fs::create_dir_all(&real_dir).unwrap();

    let link_dir = root.join("link");
    symlink(&real_dir, &link_dir).unwrap();

    let file_path = real_dir.join("file.txt");
    std::fs::write(&file_path, "content").unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root],
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
    let root = temp_root(&temp);

    let src_dir = root.join("srcdir");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("a.txt"), "a").unwrap();

    // Symlink inside the directory should be rejected for recursive copy.
    let target = root.join("real_target.txt");
    std::fs::write(&target, "target").unwrap();
    symlink(&target, src_dir.join("link.txt")).unwrap();

    let config = Arc::new(FileSystemConfig {
        allowed_roots: vec![root.clone()],
        follow_symlinks: false,
        ..Default::default()
    });
    let validator = Arc::new(PathValidator::new(config.clone()));
    let ops = FileOperations::new(validator, config);

    let dest_dir = root.join("destdir");
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
