//! QR code rendering for terminal display.
//!
//! The daemon uses device-level pairing. We render a compact `mobilecli://...`
//! URL that the mobile app can scan and store for future connections.

use crate::protocol::ConnectionInfo;
use colored::Colorize;
use qrcode::QrCode;

/// Display inline QR code for a device pairing URL (compact, for embedding in terminal).
pub fn display_session_qr(info: &ConnectionInfo) {
    println!();
    println!("  {}", "Scan to connect from mobile:".cyan().bold());

    // Use compact QR format for much smaller QR code.
    let qr_data = info.to_compact_qr();

    if let Ok(code) = QrCode::new(qr_data.as_bytes()) {
        // Get the QR code as a 2D grid of bools
        let width = code.width();
        let mut modules: Vec<Vec<bool>> = vec![vec![false; width]; width];

        for y in 0..width {
            for x in 0..width {
                use qrcode::Color;
                modules[y][x] = code[(x, y)] == Color::Dark;
            }
        }

        // Render using Unicode half-block characters (2 rows per line)
        // ▀ = top half, ▄ = bottom half, █ = full block, ' ' = empty
        let mut stdout = std::io::stdout();
        for y in (0..width).step_by(2) {
            print!("  ");
            #[allow(clippy::needless_range_loop)]
            for x in 0..width {
                let top = modules[y][x];
                let bottom = if y + 1 < width {
                    modules[y + 1][x]
                } else {
                    false
                };

                let ch = match (top, bottom) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                };
                let _ = std::io::Write::write_all(&mut stdout, ch.to_string().as_bytes());
            }
            let _ = std::io::Write::write_all(&mut stdout, b"\n");
        }
        let _ = std::io::Write::flush(&mut stdout);
    } else {
        println!("  (QR generation failed - use URL below)");
    }

    println!();
    println!("  {} {}", "Or connect to:".dimmed(), info.ws_url.green());
    println!();
}
