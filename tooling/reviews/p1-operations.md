# Review: operations.rs
Model: Strand-Rust-Coder-14B (RunPod) | 2026-02-20T17:55:06-08:00

## operations.rs (913 lines, 2 chunks)

### Chunk 1/2 (lines 1-500)

**Finding 1:** Path traversal vulnerability in `write_file` when creating parent directories – the function calls `path_utils::create_parent_dirs_safe`, which does not enforce path restrictions and could allow creation of files outside the jail.

*Fix:* Add a call to `self.validator.validate_existing(parent)` before delegating to `create_parent_dirs_safe`.

**Finding 2:** TOCTOU race in directory deletion – between checking for children and deleting, another process can create new entries, leading to partial deletions or permission errors.

*Fix:* Use atomic operations (e.g., rename) when possible or add a retry loop with exponential backoff that rechecks the state before proceeding.

**Finding 3:** Sensitive file exposure via `read_file` – files matching `.env`, `.ssh/*`, etc. are not explicitly blocked, allowing accidental leakage of secrets.

*Fix:* Extend `PathValidator::is_denied` to reject known sensitive patterns or add a dedicated check in the read path that returns an error for such files.

**Finding 4:** Missing authentication checks – all file operations bypass any session validation, enabling unauthenticated access and potential data exfiltration.

*Fix:* Introduce per‑operation auth wrappers (e.g., `auth_required`) that verify user credentials before delegating to the underlying implementation.

### Chunk 2/2 (lines 501-913)

**Finding 1:** The `rename_path` method validates the source path but does not check that the destination is writable before renaming a directory. This allows an attacker to rename directories they do not have permission to modify.

*Severity:* Medium

*Description:* When renaming a directory, only the source's writability is verified; the destination's permissions are ignored. An authenticated user could attempt to move a file into a read‑only location and succeed if the server lacks proper checks for the target path.

**Fix:** Add an explicit permission check on `new_path` before performing the rename operation:

```rust
if !self.validator.is_writable(&old_path) || !self.validator.is_writable(&new_path) {
    return Err(FileSystemError::PermissionDenied { /* ... */ });
}
```

This ensures that both source and destination paths are writable before allowing a directory move.

**Finding 2:** The `copy_dir_recursive` function does not verify the writability of intermediate parent directories when creating nested sub‑directories. An attacker could exploit this to create files in read‑only locations by crafting malicious path components.

*Severity:* Medium

*Description:* When copying a recursive tree, only leaf nodes are validated for write permissions; any missing or unwritable parent directory is created without checking its writability first. This can lead to TOCTOU races where an attacker creates a file between the existence check and creation of required directories.

**Fix:** Add permission checks on each intermediate path before creating it:

```rust
for component in current_dst.components() {
    let dir = PathBuf::from_iter(component);
    if !validator.is_writable(&dir) {
        return Err(FileSystemError::PermissionDenied { /* ... */ });
    }
}
```

This ensures that all parent directories are writable before any directory is created during the recursive copy.

**Finding 3:** The `get_file_info` method reads up to 8 KB of a file's content without verifying its writability. An attacker could request information on files they do not have permission to read, potentially exposing sensitive data or triggering unexpected I/O errors.

*Severity:* Medium

*Description:* When retrieving file metadata for non‑directory entries, the function reads up to 8 KB of the file's content without checking whether the caller has read permissions. This can leak information about files that should be hidden from them and may cause unnecessary access violations.

**Fix:** Add a permission check before performing any I/O on the target path:

```rust
if !self.validator.is_readable(&path) {
    return Err(FileSystemError::PermissionDenied { /* ... */ });
}
```

This ensures that only files for which the caller has explicit read permissions are accessed during metadata retrieval.

**Finding 4:** The `copy_dir_recursive` function does not verify whether a destination path already exists before creating it. An attacker could exploit this to overwrite existing files without being notified, leading to data loss or unintended file replacements.

*Severity:* Medium

*Description:* When copying directories recursively, the implementation creates each target directory using `fs::create_dir_all`, which succeeds even if the destination already exists as a regular file. This can cause silent overwrites of important files and may lead to accidental data corruption.

**Fix:** Add an existence check before creating any parent or leaf directories:

```rust
if dst.exists() {
    return Err(FileSystemError::AlreadyExists { /* ... */ });
}
```

This prevents the function from silently replacing existing paths with new directory structures, ensuring that callers are aware of potential conflicts.

REVIEW_COMPLETE

---
Complete: 2026-02-20T17:55:26-08:00
