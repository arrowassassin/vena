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
