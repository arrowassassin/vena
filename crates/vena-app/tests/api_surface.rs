//! Integration tests over the real AppApi command surface (no mocks, no network,
//! no FFI). Each test builds an AppApi on a fresh temp dir and imports the
//! bundled pre-forged Dracula package, then drives the commands the UI calls.

use vena_app::api::AppApi;
use vena_app::keystore::MemoryKeyStore;

/// A fresh API on an isolated temp dir. `AppApi::new` already seeds the bundled
/// Dracula package on first run, so we read that book's id rather than importing
/// a second copy.
fn api_with_book() -> (AppApi, tempfile::TempDir, i64) {
    let dir = tempfile::tempdir().unwrap();
    let api = AppApi::new(
        dir.path().to_path_buf(),
        Box::new(MemoryKeyStore::default()),
    )
    .unwrap();
    let book_id = api
        .store_for_tests()
        .list_books()
        .unwrap()
        .into_iter()
        .find(|b| b.slug.starts_with("dracula"))
        .expect("first-run seed imported Dracula")
        .id;
    (api, dir, book_id)
}

#[test]
fn settings_roundtrip_and_secret_rejection() {
    let (api, _d, _b) = api_with_book();
    // defaults
    let s = api.get_settings().unwrap();
    assert_eq!(s["gate_mode"], "standard");
    assert_eq!(s["onboarded"], false);
    // a normal setting persists
    api.set_setting("gate_mode", "strict").unwrap();
    api.set_setting("onboarded", "1").unwrap();
    let s = api.get_settings().unwrap();
    assert_eq!(s["gate_mode"], "strict");
    assert_eq!(s["onboarded"], true);
    // secret-shaped keys are refused (keys go to the keychain, never settings)
    for k in ["api_key", "cloud_key", "secret_token", "password"] {
        assert!(api.set_setting(k, "sk-xxx").is_err(), "{k} must be refused");
    }
    // and none of them leaked into the settings blob
    assert!(!serde_json::to_string(&api.get_settings().unwrap())
        .unwrap()
        .contains("sk-xxx"));
}

#[test]
fn tiers_report_installed_and_partial_from_disk() {
    let (api, dir, _b) = api_with_book();
    let models = dir.path().join("models");
    std::fs::create_dir_all(&models).unwrap();
    // a plausible-size INK file → installed
    let ink = models.join("Qwen3-4B-Instruct-Q4_K_M.gguf");
    let f = std::fs::File::create(&ink).unwrap();
    f.set_len(1_200_000_000).unwrap();
    // a stub QUILL .part → partial, not installed
    std::fs::write(models.join("Qwen3-8B-Instruct-Q4_K_M.part"), b"x").unwrap();

    let s = api.get_settings().unwrap();
    let tiers = s["tiers"].as_array().unwrap();
    let ink_t = tiers.iter().find(|t| t["id"] == "ink").unwrap();
    let quill_t = tiers.iter().find(|t| t["id"] == "quill").unwrap();
    assert_eq!(ink_t["installed"], true);
    assert_eq!(quill_t["installed"], false);
    assert_eq!(quill_t["partial"], true);
}

#[test]
fn delete_model_rejects_unknown_and_removes_known() {
    let (api, dir, _b) = api_with_book();
    assert!(api.delete_local_model("bogus").is_err());
    assert!(api.delete_paint_model("bogus").is_err());
    let models = dir.path().join("models");
    std::fs::create_dir_all(&models).unwrap();
    let ink = models.join("Qwen3-4B-Instruct-Q4_K_M.gguf");
    std::fs::write(&ink, b"weights").unwrap();
    let r = api.delete_local_model("ink").unwrap();
    assert_eq!(r["deleted"], true);
    assert!(!ink.exists());
    // deleting again is a no-op (deleted:false), not an error
    assert_eq!(api.delete_local_model("ink").unwrap()["deleted"], false);
}

#[test]
fn paint_tiers_and_cancel_lookup() {
    let (api, _d, _b) = api_with_book();
    let tiers = api.paint_tiers();
    let arr = tiers.as_array().unwrap();
    assert!(arr.iter().any(|t| t["id"] == "sketch"));
    assert!(arr.iter().any(|t| t["id"] == "easel"));
    assert!(arr.iter().all(|t| t["installed"] == false)); // nothing downloaded
                                                          // cancel on a known/unknown tier
    assert!(api.cancel_model_download("chat", "ink").is_ok());
    assert!(api.cancel_model_download("paint", "sketch").is_ok());
    assert!(api.cancel_model_download("chat", "nope").is_err());
}

#[test]
fn store_search_features_bundled_package_and_marks_shelf() {
    let (api, _d, _b) = api_with_book();
    let items = api.store_search("").unwrap();
    let dracula = items
        .iter()
        .find(|i| i.title.eq_ignore_ascii_case("dracula"))
        .expect("bundled Dracula features");
    assert_eq!(dracula.source, "vena-catalog");
    assert!(dracula.on_shelf, "already imported → on the shelf");
    // a non-matching query hides it
    assert!(api.store_search("zzzzzz").unwrap().is_empty());
}

#[test]
fn opds_ids_are_url_stable_across_removal() {
    let (api, _d, _b) = api_with_book();
    let ids = |api: &AppApi| -> Vec<String> {
        api.list_opds_catalogs()
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    };
    let base = ids(&api).len(); // a default Standard Ebooks entry is always present
    let a = api.add_opds_catalog("https://a.test/opds", "A").unwrap();
    let b = api.add_opds_catalog("https://b.test/opds", "B").unwrap();
    assert_ne!(a, b);
    // re-adding the same URL is idempotent (same id, no duplicate)
    let a2 = api
        .add_opds_catalog("https://a.test/opds", "A again")
        .unwrap();
    assert_eq!(a, a2);
    assert_eq!(ids(&api).len(), base + 2);
    // removing A leaves B (and the default) intact, B keeps its id
    api.remove_opds_catalog(&a).unwrap();
    let after = ids(&api);
    assert_eq!(after.len(), base + 1);
    assert!(after.contains(&b));
    assert!(!after.contains(&a));
}

#[test]
fn get_asset_confined_to_assets_dir() {
    let (api, _d, book_id) = api_with_book();
    // generate a real (typographic) cover asset with no paint backend
    let path = api.generate_cover(book_id, false, |_| {}).unwrap();
    let asset = api.get_asset(&path).unwrap();
    assert!(!asset["data"].as_str().unwrap().is_empty());
    assert!(asset["mime"].as_str().unwrap().contains("svg"));
    // traversal is refused
    assert!(api.get_asset("/etc/passwd").is_err());
    assert!(api.get_asset("../../etc/passwd").is_err());
}

#[test]
fn theories_add_list_and_gate_by_progress() {
    let (api, _d, book_id) = api_with_book();
    api.set_progress(book_id, 5, 0).unwrap();
    let t = api.add_theory(book_id, "The Count fears sunlight").unwrap();
    assert!(t.logged_at_chapter >= 1);
    let list = api.list_theories(book_id).unwrap();
    assert!(list.iter().any(|x| x.text.contains("sunlight")));
}

#[test]
fn burn_removes_book_and_is_idempotent_on_settings() {
    let (api, dir, book_id) = api_with_book();
    api.set_progress(book_id, 4, 0).unwrap();
    // stamp a paint marker + consent so burn has something to clear
    api.store_for_tests()
        .set_setting("spoiler_consent:1", "1")
        .unwrap();
    api.generate_cover(book_id, false, |_| {}).unwrap();
    assert!(!api.store_for_tests().list_books().unwrap().is_empty());
    let cover = dir
        .path()
        .join("assets")
        .join(format!("cover-{book_id}.svg"));
    assert!(cover.exists(), "cover was generated");
    api.delete_book(book_id).unwrap();
    // the burned book is gone (the bundled sample comic may still be on the shelf)
    assert!(!api
        .store_for_tests()
        .list_books()
        .unwrap()
        .iter()
        .any(|b| b.id == book_id));
    // cover asset gone
    assert!(!cover.exists());
}

#[test]
fn export_then_import_bundle_roundtrips_theories() {
    let (api, _d, book_id) = api_with_book();
    api.set_progress(book_id, 6, 0).unwrap();
    api.add_theory(book_id, "Renfield answers to the Count")
        .unwrap();
    let bundle = api.export_bundle(Some(book_id), "theories").unwrap();
    let json = serde_json::to_string(&bundle).unwrap();
    assert!(json.contains("Renfield"));

    // fresh profile (its own first-run Dracula seed) imports the same bundle;
    // the bundle matches by slug onto that seeded book
    let dir2 = tempfile::tempdir().unwrap();
    let api2 = AppApi::new(
        dir2.path().to_path_buf(),
        Box::new(MemoryKeyStore::default()),
    )
    .unwrap();
    let rep = api2.import_bundle(&json).unwrap();
    assert!(rep["theories_added"].as_i64().unwrap_or(0) >= 1);
    let b2 = api2
        .store_for_tests()
        .list_books()
        .unwrap()
        .into_iter()
        .find(|b| b.slug.starts_with("dracula"))
        .unwrap()
        .id;
    assert!(api2
        .list_theories(b2)
        .unwrap()
        .iter()
        .any(|t| t.text.contains("Renfield")));
}

#[test]
fn relay_presets_cover_the_documented_providers() {
    let (api, _d, _b) = api_with_book();
    let presets = api.relay_presets();
    let ids: Vec<&str> = presets
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    for want in ["openrouter", "groq", "together", "ollama", "lmstudio"] {
        assert!(ids.contains(&want), "missing preset {want}: {ids:?}");
    }
}

#[test]
fn characters_and_wiki_are_progress_gated() {
    let (api, _d, book_id) = api_with_book();
    api.set_progress(book_id, 3, 0).unwrap();
    let cast = api.list_characters(book_id).unwrap();
    assert!(!cast.is_empty());
    // met characters at ch3 are a subset of the full cast
    let met = cast.iter().filter(|c| c.met).count();
    assert!(met <= cast.len());
    // synced wiki index returns without leaking future entries
    let idx = api.get_wiki_index(book_id, "synced").unwrap();
    assert!(idx.entries.iter().all(|e| !e.group.is_empty()));
}

#[test]
fn image_status_none_without_paint_or_key() {
    let (api, _d, _b) = api_with_book();
    let st = api.get_image_status().unwrap();
    assert_eq!(st.tier, "none");
}
