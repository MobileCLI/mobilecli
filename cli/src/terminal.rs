//! Terminal-native content analysis
//! 
//! Pure escape sequence detection - no CLI-specific logic.
//! Used to determine if scrollback content is safe to replay at different
//! terminal sizes.

/// Detect if buffer contains ANSI cursor positioning sequences.
/// 
/// Content with cursor positioning (CUP, HVP, alternate screen entry)
/// is NOT safe to replay at different terminal sizes because absolute
/// coordinates will position incorrectly.
/// 
/// # Examples
/// 
/// ```
/// use mobilecli::terminal::contains_cursor_positioning;
/// 
/// // Plain text is safe
/// assert!(!contains_cursor_positioning(b"hello world"));
/// 
/// // Cursor positioning makes it unsafe
/// assert!(contains_cursor_positioning(b"\x1b[10;20H"));
/// ```
pub fn contains_cursor_positioning(data: &[u8]) -> bool {
    // CSI sequences that indicate cursor positioning:
    // - ESC [ row ; col H  (Cursor Position - CUP)
    // - ESC [ row ; col f  (Horizontal Vertical Position - HVP)
    // - ESC [ ? 1049 h/l   (Enter/Exit alternate screen)
    // - ESC [ ? 1047 h/l   (Enter/Exit alternate screen - xterm)
    // - ESC [ ? 47 h/l     (Enter/Exit alternate screen - legacy)
    // - ESC 7 / ESC 8      (Save/Restore cursor - DECSC/DECRC)
    
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b && i + 1 < data.len() {
            match data[i + 1] {
                b'[' => {
                    // CSI sequence - check for cursor positioning
                    if let Some(csi_type) = detect_csi_type(&data[i..]) {
                        match csi_type {
                            CsiType::CursorPosition | CsiType::AlternateScreen => return true,
                            _ => {}
                        }
                    }
                }
                b'7' | b'8' => return true, // DECSC/DECRC - Save/Restore cursor
                _ => {}
            }
        }
        i += 1;
    }
    false
}

/// Check if scrollback is safe to replay (plain text only).
/// 
/// Returns true if the content contains no cursor positioning sequences
/// and can be safely replayed at any terminal size.
pub fn is_safe_to_replay(data: &[u8]) -> bool {
    !contains_cursor_positioning(data)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CsiType {
    CursorPosition,    // H or f - moves cursor to absolute position
    AlternateScreen,   // ?1049h/l, ?1047h/l, ?47h/l
    Other,             // Any other CSI sequence (colors, etc.)
}

/// Parse a CSI sequence starting with ESC [ and determine its type.
/// 
/// Returns None if the sequence is incomplete or malformed.
fn detect_csi_type(data: &[u8]) -> Option<CsiType> {
    // data[0] should be ESC, data[1] should be [
    if data.len() < 3 || data[0] != 0x1b || data[1] != b'[' {
        return None;
    }
    
    let mut i = 2;
    
    // Skip parameter bytes (0x30-0x3F: digits, semicolons, ?)
    while i < data.len() {
        let b = data[i];
        if (b >= 0x30 && b <= 0x3F) || b == b';' {
            i += 1;
        } else {
            break;
        }
    }
    
    // Check for private marker '?' (alternate screen sequences)
    if i > 2 && data[2] == b'?' {
        // Look for 1049, 1047, or 47 followed by h or l
        let params = &data[3..i];
        if params == b"1049" || params == b"1047" || params == b"47" {
            if i < data.len() && (data[i] == b'h' || data[i] == b'l') {
                return Some(CsiType::AlternateScreen);
            }
        }
    }
    
    // Check final byte for cursor positioning
    if i < data.len() {
        match data[i] {
            b'H' | b'f' => return Some(CsiType::CursorPosition),
            _ => return Some(CsiType::Other),
        }
    }
    
    // Incomplete sequence
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_safe() {
        assert!(!contains_cursor_positioning(b"hello world"));
        assert!(!contains_cursor_positioning(b"Hello, World!\nMultiple lines\n"));
        assert!(!contains_cursor_positioning(b"Special chars: \t\r\n"));
    }

    #[test]
    fn test_color_sequences_safe() {
        // Color sequences don't move cursor, so they're safe
        assert!(!contains_cursor_positioning(b"\x1b[31mred text\x1b[0m"));
        assert!(!contains_cursor_positioning(b"\x1b[1;31;40mbold red on black\x1b[0m"));
    }

    #[test]
    fn test_cursor_position_unsafe() {
        assert!(contains_cursor_positioning(b"\x1b[10;20H"));
        assert!(contains_cursor_positioning(b"\x1b[H"));
        assert!(contains_cursor_positioning(b"\x1b[24;80f"));
        assert!(contains_cursor_positioning(b"text\x1b[5;10Hmore text"));
    }

    #[test]
    fn test_alternate_screen_unsafe() {
        assert!(contains_cursor_positioning(b"\x1b[?1049h"));
        assert!(contains_cursor_positioning(b"\x1b[?1049l"));
        assert!(contains_cursor_positioning(b"\x1b[?1047h"));
        assert!(contains_cursor_positioning(b"\x1b[?47h"));
    }

    #[test]
    fn test_save_restore_cursor_unsafe() {
        assert!(contains_cursor_positioning(b"\x1b7"));
        assert!(contains_cursor_positioning(b"\x1b8"));
    }

    #[test]
    fn test_mixed_content() {
        // Content with colors AND cursor positioning is unsafe
        assert!(contains_cursor_positioning(b"\x1b[31m\x1b[10;20Hred at position\x1b[0m"));
    }

    #[test]
    fn test_empty_and_edge_cases() {
        assert!(!contains_cursor_positioning(b""));
        assert!(!contains_cursor_positioning(b"\x1b")); // Incomplete ESC
        assert!(!contains_cursor_positioning(b"\x1b[")); // Incomplete CSI
        assert!(!contains_cursor_positioning(b"\x1b[31")); // Incomplete color
    }
}
