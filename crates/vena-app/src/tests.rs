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

#[test]
fn portable_bundle_syncs_progress_and_theories_across_devices() {
    // Device A: read to ch8, pin two theories.
    let a = AppApi::in_memory().unwrap();
    let book_a = a.store_download(&dracula_item(), |_, _| {}).unwrap();
    a.set_progress(book_a.id, 8, 0).unwrap();
    a.add_theory(book_a.id, "The Count controls the weather")
        .unwrap();
    a.add_theory(book_a.id, "Renfield serves a hidden master")
        .unwrap();

    let bundle = a.export_bundle(Some(book_a.id), "sync").unwrap();
    let bundle_str = bundle.to_string();

    // Device B: same book, fresh (progress 0, no theories). Import the bundle.
    let b = AppApi::in_memory().unwrap();
    let book_b = b.store_download(&dracula_item(), |_, _| {}).unwrap();
    let report = b.import_bundle(&bundle_str).unwrap();
    assert_eq!(report["matched_books"], 1);
    assert_eq!(report["progress_updated"], 1);
    assert_eq!(report["theories_added"], 2);

    // B now mirrors A's progress + theories.
    assert_eq!(b.get_book(book_b.id).unwrap().progress_episode, 8);
    assert_eq!(b.list_theories(book_b.id).unwrap().len(), 2);

    // Re-importing the SAME bundle is idempotent (dedup on text, LWW on progress).
    let again = b.import_bundle(&bundle_str).unwrap();
    assert_eq!(again["theories_added"], 0);
    assert_eq!(again["progress_updated"], 0);

    // "theories" scope shares no reading position (book-club safe).
    let theories_only = a.export_bundle(Some(book_a.id), "theories").unwrap();
    assert!(theories_only["books"][0].get("progress").is_none());

    // A book not on the importer's shelf is skipped, not errored (in_memory starts empty).
    let c = AppApi::in_memory().unwrap();
    assert!(c.list_books().unwrap().is_empty());
    let skipped = c.import_bundle(&bundle_str).unwrap();
    assert_eq!(skipped["matched_books"], 0);
    assert_eq!(skipped["skipped_not_on_shelf"].as_array().unwrap().len(), 1);
}

#[test]
fn forget_conversations_keeps_book_and_theories() {
    let api = AppApi::in_memory().unwrap();
    let book = api.store_download(&dracula_item(), |_, _| {}).unwrap();
    api.add_theory(book.id, "a theory").unwrap();
    api.forget_conversations(book.id).unwrap(); // no chats yet — must not error
    assert!(api.get_book(book.id).is_ok());
    assert_eq!(api.list_theories(book.id).unwrap().len(), 1);
}

#[test]
fn local_steer_is_device_correct_via_validation() {
    let api = AppApi::in_memory().unwrap();
    // Configure a local tier (no real model, but the steer is a settings signal).
    api.set_setting("default_chat_mode", "local").unwrap();
    api.set_setting("local_model", "QUILL·7B").unwrap();
    api.set_setting("local_model_ready", "1").unwrap();

    // Before validation: experimental.
    assert!(api.get_ai_status().unwrap().local_experimental);

    // A clean in-app probe run promotes THIS tier; simulate via the explicit setter.
    api.set_local_validated(true).unwrap();
    assert!(!api.get_ai_status().unwrap().local_experimental);

    // Validation is per-tier: switching to an unvalidated tier is experimental again.
    api.set_setting("local_model", "INK·3B").unwrap();
    assert!(api.get_ai_status().unwrap().local_experimental);

    // A leak demotes.
    api.set_setting("local_model", "QUILL·7B").unwrap();
    api.set_local_validated(false).unwrap();
    assert!(api.get_ai_status().unwrap().local_experimental);
}

#[test]
fn relay_presets_and_one_tap_setup() {
    let api = AppApi::in_memory().unwrap();
    let presets = api.relay_presets();
    let arr = presets.as_array().unwrap();
    assert!(arr.iter().any(|p| p["id"] == "openrouter"));
    assert!(arr.iter().any(|p| p["id"] == "ollama"));

    // Remote provider without a key is refused (honest, no silent misconfig).
    assert!(api.configure_relay("openrouter", "", "").is_err());
    // Unknown provider is refused.
    assert!(api.configure_relay("nope", "sk-x", "").is_err());
    // A localhost provider needs no key: it fills base+default model and persists,
    // then attempts a (here unreachable) test — config is written regardless.
    let _ = api.configure_relay("ollama", "", "");
    let s = api.get_settings().unwrap();
    assert_eq!(s["cloud_base_url"], "http://localhost:11434/v1");
    assert_eq!(s["cloud_model"], "qwen3:8b");
    assert_eq!(s["default_chat_mode"], "cloud");
}
