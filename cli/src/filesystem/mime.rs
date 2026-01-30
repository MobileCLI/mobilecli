/// Detect MIME type from file content and extension
pub fn detect_mime_type(buffer: &[u8], filename: &str) -> String {
    if let Some(kind) = infer::get(buffer) {
        return kind.mime_type().to_string();
    }
    guess_mime_from_extension(filename)
}

/// Guess MIME type based on filename extension
pub fn guess_mime_from_extension(filename: &str) -> String {
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

/// Check if MIME type is text-based (safe to display as text)
pub fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/javascript"
        || mime == "application/xml"
        || mime.ends_with("+xml")
        || mime.ends_with("+json")
}
