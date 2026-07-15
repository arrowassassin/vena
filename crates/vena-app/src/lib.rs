//! `vena-app`. The command LOGIC lives in `api` (no Tauri dependency → fully
//! unit-testable and identical whether driven by Tauri, a test, or a headless
//! runner). The Tauri shell (`#[tauri::command]` wrappers + `Builder`) lives in
//! `tauri_shell` behind the `tauri` feature and is exposed as `run()`: the
//! desktop `vena` bin calls it from `main`, and on mobile the
//! `tauri::mobile_entry_point` macro exports it as the entry symbol the
//! Android/iOS host loads from `libvena_app.so`.

pub mod api;
pub mod gguf;
pub mod images;
pub mod keystore;
#[cfg(feature = "embedded-llm")]
pub mod local_llm;
#[cfg(feature = "embedded-paint")]
pub mod local_paint;
pub mod net;

// The Tauri shell + mobile entry point. Only built with the `tauri` feature, so
// the lib stays Tauri-free (and testable) for desktop/CI unit builds.
#[cfg(feature = "tauri")]
mod tauri_shell;
#[cfg(feature = "tauri")]
pub use tauri_shell::run;

pub use api::AppApi;
pub use keystore::{KeyStore, MemoryKeyStore};

#[cfg(test)]
mod tests;
