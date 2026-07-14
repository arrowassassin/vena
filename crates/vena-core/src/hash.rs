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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_vector() {
        // SHA-256("abc") is a published test vector.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex(b"").len(), 64);
    }

    #[test]
    fn reader_and_slice_agree() {
        let data = b"the quick brown fox";
        let from_slice = sha256_hex(data);
        let from_reader = sha256_hex_reader(std::io::Cursor::new(data)).unwrap();
        assert_eq!(from_slice, from_reader);
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
