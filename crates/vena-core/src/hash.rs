//! Shared digest + hex helpers. One canonical implementation so package digests
//! (forge `content_sha`, `pkg::write_vena`) and download verification (`net`) can
//! never drift in format and silently fail an integrity gate.

use crate::Result;
use sha2::{Digest, Sha256};

/// Lowercase hex of a byte slice.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lowercase-hex SHA-256 of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finalize())
}

/// Streaming lowercase-hex SHA-256 of anything readable (files without loading
/// the whole blob into memory — model GGUFs are multi-GB).
pub fn sha256_hex_reader(mut reader: impl std::io::Read) -> Result<String> {
    let mut h = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}
