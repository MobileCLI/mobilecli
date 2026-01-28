//! QR code generation for terminal display
//!
//! Generates QR codes that can be scanned by the mobile app.

use crate::protocol::ConnectionInfo;
use colored::Colorize;
use qrcode::QrCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum QrError {
    #[error("Failed to generate QR code: {0}")]
    Generation(String),
    #[error("Failed to get local IP: {0}")]
    LocalIp(String),
}

/// Default WebSocket port
pub const DEFAULT_WS_PORT: u16 = 9847;

/// Get the local IP address for LAN connections
pub fn get_local_ip() -> Result<String, QrError> {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .map_err(|e| QrError::LocalIp(e.to_string()))
}

/// Generate and display a QR code in the terminal
pub fn display_qr(data: &str) -> Result<(), QrError> {
    let code = QrCode::new(data.as_bytes()).map_err(|e| QrError::Generation(e.to_string()))?;

    // Use compact 1x1 modules for smaller QR display
    let string = code
        .render::<char>()
        .quiet_zone(false)
        .module_dimensions(1, 1)
        .build();

    // Print safely line by line
    for line in string.lines() {
        if std::io::Write::write_all(&mut std::io::stdout(), line.as_bytes()).is_err() {
            break;
        }
        let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\n");
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
    Ok(())
}

/// Generate connection info for a session
#[allow(dead_code)]
pub fn generate_connection_info(
    session_id: &str,
    ws_port: u16,
    encryption_key: Option<String>,
) -> Result<ConnectionInfo, QrError> {
    let local_ip = get_local_ip()?;

    // Load device info from config
    let config = crate::setup::load_config();
    let (device_id, device_name) = config
        .map(|c| (Some(c.device_id), Some(c.device_name)))
        .unwrap_or((None, None));

    Ok(ConnectionInfo {
        ws_url: format!("ws://{}:{}", local_ip, ws_port),
        session_id: session_id.to_string(),
        session_name: None,
        encryption_key,
        version: env!("CARGO_PKG_VERSION").to_string(),
        device_id,
        device_name,
    })
}

/// Show pairing QR code for mobile app
pub async fn show_pairing_qr() -> Result<(), QrError> {
    let local_ip = get_local_ip()?;
    let session_id = uuid::Uuid::new_v4().to_string();

    // Load device info from config
    let config = crate::setup::load_config();
    let (device_id, device_name) = config
        .map(|c| (Some(c.device_id), Some(c.device_name)))
        .unwrap_or((None, None));

    let info = ConnectionInfo {
        ws_url: format!("ws://{}:{}", local_ip, DEFAULT_WS_PORT),
        session_id,
        session_name: None,
        encryption_key: None, // TODO: Add encryption
        version: env!("CARGO_PKG_VERSION").to_string(),
        device_id,
        device_name,
    };

    println!();
    println!(
        "{}",
        "╔══════════════════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║           📱 Scan with MobileCLI app to connect              ║".cyan()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════════════════╝".cyan()
    );
    println!();

    display_qr(&info.to_qr_data())?;

    println!();
    println!("  {} {}", "WebSocket:".dimmed(), info.ws_url.green());
    println!("  {} {}", "Local IP:".dimmed(), local_ip.yellow());
    println!();
    println!(
        "{}",
        "Make sure your phone is on the same WiFi network.".dimmed()
    );
    println!();

    Ok(())
}

/// Display inline QR code for a session (smaller, for embedding in terminal)
pub fn display_session_qr(info: &ConnectionInfo) {
    println!();
    println!("  📱 {}", "Scan to connect from mobile:".cyan().bold());

    // Use compact QR format for much smaller QR code
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
