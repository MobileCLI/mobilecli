/// Sanitize arbitrary identifiers for tmux socket/session tokens.
pub fn sanitize_tmux_token(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if out.is_empty() {
        out.push_str("session");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sanitize_tmux_token;

    #[test]
    fn sanitize_tmux_token_replaces_unsafe_chars() {
        assert_eq!(sanitize_tmux_token("abc123"), "abc123");
        assert_eq!(sanitize_tmux_token("abc/def"), "abc-def");
        assert_eq!(sanitize_tmux_token(""), "session");
    }
}
