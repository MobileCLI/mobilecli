use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::protocol::GitStatus;

pub async fn status_map_for_path(path: &Path) -> Option<HashMap<PathBuf, GitStatus>> {
    let repo_root = find_repo_root(path).await?;
    status_map(&repo_root).await
}

pub async fn status_for_path(path: &Path) -> Option<GitStatus> {
    let repo_root = find_repo_root(path).await?;
    let rel = path.strip_prefix(&repo_root).ok()?;

    let output = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .arg("status")
        .arg("--porcelain")
        .arg("--ignored")
        .arg("--untracked-files=normal")
        .arg("--")
        .arg(rel)
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next()?;
    parse_status_line(line)
}

async fn status_map(repo_root: &Path) -> Option<HashMap<PathBuf, GitStatus>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("status")
        .arg("--porcelain")
        .arg("--ignored")
        .arg("--untracked-files=normal")
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }
        let status = match parse_status_line(line) {
            Some(status) => status,
            None => continue,
        };
        let rel = parse_path_from_status(line);
        if rel.is_empty() {
            continue;
        }
        let abs = repo_root.join(rel);
        map.insert(abs, status);
    }
    Some(map)
}

async fn find_repo_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        // Use async metadata check instead of blocking exists()
        let git_path = current.join(".git");
        match tokio::fs::metadata(&git_path).await {
            Ok(meta) if meta.is_dir() => return Some(current),
            _ => {}
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn parse_status_line(line: &str) -> Option<GitStatus> {
    let status = line.get(0..2)?;
    match status {
        "??" => Some(GitStatus::Untracked),
        "!!" => Some(GitStatus::Ignored),
        s if s.contains('D') => Some(GitStatus::Deleted),
        s if s.contains('A') => Some(GitStatus::Added),
        s if s.contains('M') => Some(GitStatus::Modified),
        s if s.contains('R') || s.contains('C') => Some(GitStatus::Modified),
        _ => Some(GitStatus::Modified),
    }
}

fn parse_path_from_status(line: &str) -> String {
    let raw = line.get(3..).unwrap_or("").trim();
    if let Some((_, new_path)) = raw.split_once(" -> ") {
        return new_path.trim().to_string();
    }
    raw.trim_matches('"').to_string()
}
