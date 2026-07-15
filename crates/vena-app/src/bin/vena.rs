//! The Vena desktop binary — a thin entry point. The whole Tauri shell (every
//! `#[tauri::command]` + the `Builder`) lives in the lib (`vena_app::run`) so the
//! SAME shell powers mobile, where Android/iOS load `libvena_app.so` and call its
//! `tauri::mobile_entry_point`. Desktop `main` just invokes it.
//!
//! Build on a dev machine: `cargo build -p vena-app --features tauri --bin vena`

fn main() {
    vena_app::run();
}
