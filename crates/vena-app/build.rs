fn main() {
    // Only run the Tauri build steps when the tauri feature is enabled.
    #[cfg(feature = "tauri")]
    tauri_build::build();
}
