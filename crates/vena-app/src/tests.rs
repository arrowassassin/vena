//! IPC-surface tests. Drive the real `AppApi` (no Tauri) against the real Dracula
//! package to prove the commands work end-to-end and uphold the invariants.

use crate::api::{AppApi, StoreItem};

fn dracula_item() -> StoreItem {
    let pkg = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/packages/dracula.vena");
    StoreItem {
        source: "vena-catalog".into(),
        id: "dracula".into(),
        title: "Dracula".into(),
        author: Some("Bram Stoker".into()),
        license: Some("public-domain".into()),
        download_url: Some(pkg.to_string_lossy().into()),
        cover: None,
        on_shelf: false,
    }
}

#[test]
fn library_reading_and_gate_via_ipc() {
    let api = AppApi::in_memory().unwrap();
    let book = api
        .store_download(&dracula_item(), |_, _| {})
        .expect("import bundled package");

    assert!(api
        .list_books()
        .unwrap()
        .iter()
        .any(|b| b.title == "Dracula"));

    // Reading: episode 1 is the real canon.
    let ep = api.get_episode(book.id, 1).unwrap();
    assert!(ep.content_html.contains("Bistritz") || ep.content_html.contains("Munich"));

    // Progress + gate: at ch6, Van Helsing (ch9) is unmet.
    api.set_progress(book.id, 6, 0).unwrap();
    let chars = api.list_characters(book.id).unwrap();
    let vh = chars
        .iter()
        .find(|c| c.name.contains("Van Helsing"))
        .unwrap();
    assert!(!vh.met, "Van Helsing unmet at ch6");
    // who_is gates unmet characters.
    assert_eq!(
        api.who_is(book.id, "Van Helsing").unwrap_err().code(),
        "NotFound"
    );

    // Companion with no backend configured → NoBackend (honest, not a mock reply).
    let err = api
        .companion_turn(book.id, None, "Hello?", &mut |_| {})
        .unwrap_err();
    assert_eq!(err.code(), "NoBackend");

    // Theories resolve only after the reveal.
    let t = api
        .add_theory(book.id, "Lucy dies from her illness")
        .unwrap();
    assert!(t.resolved_status.is_none());
    api.set_progress(book.id, 12, 0).unwrap();
    let resolved = api.list_theories(book.id).unwrap();
    assert!(resolved[0].resolved_status.is_some());

    // Re-seal on rewind: back to ch6 reopens the resolution.
    api.set_progress(book.id, 6, 0).unwrap();
    assert!(api.list_theories(book.id).unwrap()[0]
        .resolved_status
        .is_none());

    // Archive: full wiki requires consent.
    assert_eq!(
        api.get_wiki_index(book.id, "full").unwrap_err().code(),
        "SpoilerConsentRequired"
    );
    api.set_spoiler_consent(book.id, true).unwrap();
    assert!(api.get_wiki_index(book.id, "full").is_ok());

    // Burn removes it.
    api.delete_book(book.id).unwrap();
    assert!(api.list_books().unwrap().is_empty());
}

#[test]
fn api_key_never_touches_the_database() {
    let api = AppApi::in_memory().unwrap();
    api.set_api_config("https://openrouter.ai/api", "sk-secret-123", "some-model")
        .unwrap();
    let settings = api.get_settings().unwrap();
    assert!(
        !settings.to_string().contains("sk-secret-123"),
        "secret leaked into settings"
    );
    // Direct set_setting of a secret is refused.
    assert!(api.set_setting("cloud_api_key", "sk-nope").is_err());
    // Cloud mode ready once base + key configured.
    let status = api.get_ai_status().unwrap();
    assert_eq!(status.mode, "cloud");
    assert!(status.ready);
    assert!(status.local_experimental);
}

#[test]
fn ai_status_defaults_to_cloud_steer() {
    // Per the §11.5 eval steer: default chat mode is cloud, local is experimental.
    let api = AppApi::in_memory().unwrap();
    let status = api.get_ai_status().unwrap();
    assert!(status.local_experimental);
    assert!(!status.ready); // nothing configured → honest "not ready", no mock
    assert_eq!(status.mode, "none");
}
