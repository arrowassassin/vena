//! In-process local painting (feature `embedded-paint`): downloaded SD GGUF
//! tiers render through stable-diffusion.cpp compiled into the app — no `sd`
//! CLI to install. One render at a time (a Mutex serializes; diffusion is
//! memory-heavy), and every failure falls through to the next honest tier.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use diffusion_rs::api::{gen_img, ConfigBuilder, ModelConfigBuilder};
use vena_core::{Result, VenaError};

fn lock() -> &'static Mutex<()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
}

/// Render `prompt` with the GGUF at `model` into `out` (PNG).
pub fn render(model: &Path, prompt: &str, w: u32, h: u32, out: &Path) -> Result<()> {
    crate::gguf::assert_gguf(model)?;
    let _g = lock()
        .lock()
        .map_err(|_| VenaError::Other("paint lock poisoned".into()))?;
    let mut model_config = ModelConfigBuilder::default()
        .model(model.to_path_buf())
        .build()
        .map_err(|e| VenaError::Other(format!("paint model config: {e}")))?;
    let config = ConfigBuilder::default()
        .prompt(prompt.to_string())
        .width(w as i32)
        .height(h as i32)
        .steps(18)
        .cfg_scale(7.0)
        .output(out.to_path_buf())
        .build()
        .map_err(|e| VenaError::Other(format!("paint config: {e}")))?;
    gen_img(&config, &mut model_config)
        .map_err(|e| VenaError::Other(format!("local paint failed: {e}")))?;
    if !out.exists() {
        return Err(VenaError::Other("local paint produced no image".into()));
    }
    Ok(())
}
