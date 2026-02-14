/// Detect MIME type from file content and extension
pub fn detect_mime_type(buffer: &[u8], filename: &str) -> String {
    if let Some(kind) = infer::get(buffer) {
        return kind.mime_type().to_string();
    }
    let guessed = guess_mime_from_extension(filename);
    if guessed == "application/octet-stream" && is_probably_text(buffer) {
        return "text/plain".to_string();
    }
    guessed
}

/// Guess MIME type based on filename extension
pub fn guess_mime_from_extension(filename: &str) -> String {
    let filename_lower = filename.to_lowercase();

    // Special-case common extensionless filenames.
    match filename_lower.as_str() {
        "dockerfile" => return "text/x-dockerfile".to_string(),
        "makefile" => return "text/x-makefile".to_string(),
        "license" | "licence" | "copying" => return "text/plain".to_string(),
        _ => {}
    }

    // Common dotfiles (treat as text based on name; content sniffing in `detect_mime_type`
    // will still override for binary data).
    if filename_lower.starts_with(".env") {
        return "text/x-env".to_string();
    }
    match filename_lower.as_str() {
        ".ds_store" => return "application/octet-stream".to_string(),
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "text/plain".to_string(),
        ".npmrc" | ".yarnrc" | ".editorconfig" => return "text/plain".to_string(),
        ".prettierrc" | ".prettierignore" | ".eslintrc" | ".eslintignore" => {
            return "text/plain".to_string()
        }
        ".bashrc" | ".bash_profile" | ".bash_aliases" | ".profile" | ".zshrc" | ".zshenv"
        | ".zprofile" => return "text/x-shellscript".to_string(),
        ".env" => return "text/x-env".to_string(),
        _ => {}
    }

    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "js" => "application/javascript",
        "ts" => "text/typescript",
        "tsx" => "text/typescript-jsx",
        "jsx" => "text/javascript-jsx",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++",
        "rb" => "text/x-ruby",
        "php" => "text/x-php",
        "swift" => "text/x-swift",
        "kt" | "kts" => "text/x-kotlin",
        "scala" => "text/x-scala",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "ps1" => "text/x-powershell",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "scss" | "sass" => "text/x-scss",
        "less" => "text/x-less",
        "xml" => "application/xml",
        "json" => "application/json",
        "yaml" | "yml" => "text/x-yaml",
        "toml" => "text/x-toml",
        "md" | "markdown" => "text/markdown",
        "rst" => "text/x-rst",
        "tex" => "text/x-tex",
        "ini" | "cfg" | "conf" => "text/x-ini",
        "env" => "text/x-env",
        "dockerfile" => "text/x-dockerfile",
        "makefile" => "text/x-makefile",
        "gitignore" | "gitattributes" | "gitmodules" => "text/plain",
        "npmrc" | "yarnrc" | "editorconfig" => "text/plain",
        "eslintrc" | "eslintignore" | "prettierrc" | "prettierignore" => "text/plain",
        "txt" => "text/plain",
        "log" => "text/x-log",
        "csv" => "text/csv",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Best-effort heuristic for deciding if a buffer is "text enough" to render as UTF-8.
///
/// This is used to avoid treating common config files (dotfiles, extensionless files, etc)
/// as binary just because the extension is unknown.
pub fn is_probably_text(buffer: &[u8]) -> bool {
    if buffer.is_empty() {
        return true;
    }

    // NUL is a strong binary signal.
    if buffer.contains(&0) {
        return false;
    }

    // UTF-16 BOMs are text.
    if buffer.starts_with(&[0xFF, 0xFE]) || buffer.starts_with(&[0xFE, 0xFF]) {
        return true;
    }

    // If the bytes are valid UTF-8, assume text unless it's packed with control chars.
    let is_utf8 = std::str::from_utf8(buffer).is_ok();
    if !is_utf8 {
        return false;
    }

    let mut control_count = 0usize;
    for &b in buffer {
        // Allow common whitespace.
        if matches!(b, b'\t' | b'\n' | b'\r') {
            continue;
        }
        // Count other ASCII control bytes.
        if b < 0x20 {
            control_count += 1;
        }
    }

    // If >10% of bytes are control characters, treat as binary-ish.
    control_count * 10 <= buffer.len()
}

/// Check if MIME type is text-based (safe to display as text)
pub fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/javascript"
        || mime == "application/xml"
        || mime.ends_with("+xml")
        || mime.ends_with("+json")
}
