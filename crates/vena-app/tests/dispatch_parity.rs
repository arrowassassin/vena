//! Guard: the Tauri command handlers (bin/vena.rs) and the HTTP devserver dispatch
//! (bin/devserver.rs) are two hand-maintained mirrors of the same §11.2 surface.
//! If one gains a command the other lacks, the UI works on one transport and 404s
//! on the other. This test parses both binaries' command sets and asserts they match.

use std::collections::BTreeSet;

fn read(p: &str) -> String {
    std::fs::read_to_string(format!("{}/{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Tauri commands = the identifiers listed in `tauri::generate_handler![ ... ]`.
fn tauri_commands() -> BTreeSet<String> {
    let src = read("src/bin/vena.rs");
    let start = src.find("generate_handler![").expect("handler macro");
    let rest = &src[start + "generate_handler![".len()..];
    let end = rest.find(']').expect("handler close");
    rest[..end]
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Devserver commands = the string arms of the dispatch match (`"cmd" =>`).
fn devserver_commands() -> BTreeSet<String> {
    let src = read("src/bin/devserver.rs");
    let re = regex::Regex::new(r#""([a-z0-9_]+)"\s*=>"#).unwrap();
    re.captures_iter(&src)
        .map(|c| c[1].to_string())
        .filter(|c| c != "events") // not a command, the SSE endpoint
        .collect()
}

#[test]
fn tauri_and_devserver_expose_the_same_commands() {
    let tauri = tauri_commands();
    let dev = devserver_commands();
    let only_tauri: Vec<_> = tauri.difference(&dev).collect();
    let only_dev: Vec<_> = dev.difference(&tauri).collect();
    assert!(
        only_tauri.is_empty() && only_dev.is_empty(),
        "command surfaces drifted — only in Tauri: {only_tauri:?}; only in devserver: {only_dev:?}"
    );
    assert!(
        tauri.len() > 30,
        "sanity: expected the full command surface"
    );
}
