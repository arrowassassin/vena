//! Shared GGUF validation. llama.cpp / stable-diffusion.cpp SEGFAULT on
//! malformed weights instead of erroring — nothing crosses the FFI line
//! without the magic check.

use vena_core::{Result, VenaError};

pub fn assert_gguf(path: &std::path::Path) -> Result<()> {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|e| VenaError::Other(format!("unreadable weights: {e}")))?;
    if &magic != b"GGUF" {
        return Err(VenaError::Other(
            "weights file is not a valid GGUF (corrupt download?) — delete and re-download it"
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_gguf_magic_rejects_other() {
        let dir = std::env::temp_dir().join(format!("vena-gguf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("ok.gguf");
        std::fs::write(&good, b"GGUF\x00\x00\x00\x03rest").unwrap();
        assert!(assert_gguf(&good).is_ok());

        let bad = dir.join("bad.gguf");
        std::fs::write(&bad, b"<htmlerror>").unwrap();
        assert!(assert_gguf(&bad).is_err());

        assert!(assert_gguf(&dir.join("missing.gguf")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
