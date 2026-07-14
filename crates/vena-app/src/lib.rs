//! `vena-app` — the Tauri shell. The command LOGIC lives here in `api` (no Tauri
//! dependency → fully unit-testable and identical whether driven by Tauri, a test,
//! or a headless runner). The `vena` binary (src/bin/vena.rs, `--features tauri`) is
//! a thin set of `#[tauri::command]` wrappers over `AppApi`.

pub mod api;
pub mod gguf;
pub mod images;
pub mod keystore;
#[cfg(feature = "embedded-llm")]
pub mod local_llm;
#[cfg(feature = "embedded-paint")]
pub mod local_paint;
pub mod net;

pub use api::AppApi;
pub use keystore::{KeyStore, MemoryKeyStore};

#[cfg(test)]
mod tests;
