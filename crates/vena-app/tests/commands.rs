//! Local command surface that needs no model backend: progress/reseal, serial
//! mode, wiki consent gating, leak reports, OPDS browse against a loopback feed,
//! catalog store_download, and typographic auto_paint. All real AppApi calls on a
//! fresh temp profile — no mocks.

use vena_app::api::AppApi;
use vena_app::keystore::MemoryKeyStore;

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
fn serial_mode_persists_into_book_meta() {
    let (api, _d, book_id) = api_with_book();
    api.set_serial_mode(book_id, true, 12).unwrap();
    let meta = api.store_for_tests().book_meta_value(book_id).unwrap();
    assert_eq!(meta["serial"]["enabled"], true);
    assert_eq!(meta["serial"]["minutesPerEpisode"], 12);
}

#[test]
fn rewind_reseals_and_reopens_theories() {
    let (api, _d, book_id) = api_with_book();
    // advance, log a theory, then rewind behind it
    api.set_progress(book_id, 8, 0).unwrap();
    let t = api
        .add_theory(book_id, "Lucy's illness has a hidden cause")
        .unwrap();
    assert!(t.logged_at_chapter >= 1);
    api.set_progress(book_id, 2, 0).unwrap();
    // still listed after a re-seal rewind (reopened, not destroyed)
    let list = api.list_theories(book_id).unwrap();
    assert!(list.iter().any(|x| x.text.contains("hidden cause")));
    // progress actually moved back
    assert_eq!(api.store_for_tests().get_progress(book_id).unwrap().0, 2);
}

#[test]
fn forget_conversations_is_safe_with_no_history() {
    let (api, _d, book_id) = api_with_book();
    // no conversations yet — must not error
    api.forget_conversations(book_id).unwrap();
    let convo = api.get_conversation(book_id, None).unwrap();
    assert_eq!(convo["count"].as_i64().unwrap(), 0);
}

#[test]
fn wiki_consent_unseals_full_mode() {
    let (api, _d, book_id) = api_with_book();
    api.set_progress(book_id, 3, 0).unwrap();
    // synced mode always safe
    let synced = api.get_wiki_index(book_id, "synced").unwrap();
    assert!(!synced.entries.is_empty());
    // full mode needs consent
    api.set_spoiler_consent(book_id, true).unwrap();
    let full = api.get_wiki_index(book_id, "full").unwrap();
    // a real page for the first entry resolves in full mode
    if let Some(first) = full.entries.first() {
        let page = api.get_wiki_page(book_id, &first.id, "full").unwrap();
        assert!(!page.title.is_empty());
    }
    api.set_spoiler_consent(book_id, false).unwrap();
}

#[test]
fn report_leak_appends_a_local_jsonl_line() {
    let (api, dir, book_id) = api_with_book();
    api.set_progress(book_id, 4, 0).unwrap();
    api.report_leak(
        book_id,
        "future_event",
        "the offending line",
        "felt spoiled",
    )
    .unwrap();
    let log = dir.path().join("leak-reports.jsonl");
    let body = std::fs::read_to_string(&log).unwrap();
    assert!(body.contains("future_event"));
    assert!(body.contains("the offending line"));
    // a second report appends rather than truncating
    api.report_leak(book_id, "other", "second", "").unwrap();
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 2);
}

#[test]
fn image_status_reports_desktop_tier_when_paint_weights_present() {
    let (api, dir, _b) = api_with_book();
    // no config, no weights → none
    assert_eq!(api.get_image_status().unwrap().tier, "none");
    // drop a plausible paint weight on disk → desktop tier
    let paint = dir.path().join("models/paint");
    std::fs::create_dir_all(&paint).unwrap();
    std::fs::write(paint.join("stable-diffusion-v1-5.gguf"), b"weights").unwrap();
    assert_eq!(api.get_image_status().unwrap().tier, "desktop");
}

#[test]
fn set_chat_mode_local_without_engine_is_refused() {
    let (api, _d, _b) = api_with_book();
    // no weights, nothing on localhost → local mode is refused with guidance
    let err = api.set_chat_mode("local").unwrap_err();
    assert!(err.to_string().contains("local engine") || err.to_string().contains("download"));
    // switching to cloud always sticks
    api.set_chat_mode("cloud").unwrap();
    assert_eq!(api.get_settings().unwrap()["default_chat_mode"], "cloud");
}

/// A loopback OPDS feed so store_browse's OPDS path (and net::opds_fetch) run for real.
fn serve_opds() -> (u16, std::thread::JoinHandle<()>) {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let feed = r#"<?xml version="1.0"?>
      <feed xmlns="http://www.w3.org/2005/Atom">
        <entry><title>A Loopback Book</title><id>urn:lb:1</id>
          <author><name>Nobody</name></author>
          <link rel="http://opds-spec.org/acquisition" href="http://x.test/lb.epub"/>
        </entry>
      </feed>"#;
    let handle = std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let _ = req.respond(tiny_http::Response::from_string(feed));
        }
    });
    (port, handle)
}

#[test]
fn opds_catalog_add_browse_and_remove() {
    let (api, _d, _b) = api_with_book();
    let (port, _h) = serve_opds();
    let url = format!("http://127.0.0.1:{port}/opds");
    let id = api.add_opds_catalog(&url, "Loopback").unwrap();
    // browse the freshly added catalog by id → real HTTP fetch + parse
    let items = api.store_browse(&id, None).unwrap();
    assert!(items.iter().any(|i| i.title == "A Loopback Book"));
    assert_eq!(items[0].source, "opds");
    api.remove_opds_catalog(&id).unwrap();
    let ids: Vec<String> = api
        .list_opds_catalogs()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert!(!ids.contains(&id));
}

#[test]
fn store_download_imports_a_bundled_catalog_package() {
    // a fresh profile downloads the bundled Dracula catalog package end-to-end
    let dir = tempfile::tempdir().unwrap();
    let api = AppApi::new(
        dir.path().to_path_buf(),
        Box::new(MemoryKeyStore::default()),
    )
    .unwrap();
    let item = api
        .store_search("dracula")
        .unwrap()
        .into_iter()
        .find(|i| i.source == "vena-catalog")
        .expect("bundled catalog package");
    let mut phases = Vec::new();
    let meta = api
        .store_download(&item, |_p, phase| phases.push(phase.to_string()))
        .unwrap();
    assert!(meta.title.to_lowercase().contains("dracula"));
    assert!(phases.iter().any(|p| p == "forge"));
}

#[test]
fn export_whole_shelf_sync_bundle_carries_progress() {
    let (api, _d, book_id) = api_with_book();
    api.set_progress(book_id, 5, 0).unwrap();
    api.add_theory(book_id, "a portable theory").unwrap();
    // book_id = None exports the whole shelf; "sync" scope includes progress
    let bundle = api.export_bundle(None, "sync").unwrap();
    assert_eq!(bundle["scope"], "sync");
    let books = bundle["books"].as_array().unwrap();
    assert!(!books.is_empty());
    let mine = books
        .iter()
        .find(|b| b["slug"].as_str().unwrap_or("").starts_with("dracula"))
        .unwrap();
    assert_eq!(mine["progress"]["episode"], 5);
    assert!(mine["theories"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["text"] == "a portable theory"));
}

#[test]
fn import_ao3_link_rejects_a_non_ao3_url() {
    let (api, _d, _b) = api_with_book();
    // ao3_epub_url refuses anything that isn't an archiveofourown.org work URL
    assert!(api
        .import_ao3_link("https://example.com/not/ao3", |_, _| {})
        .is_err());
}

#[test]
fn set_api_config_reflects_in_settings() {
    let (api, _d, _b) = api_with_book();
    api.set_api_config("https://relay.test/v1", "sekret", "some-model")
        .unwrap();
    let s = api.get_settings().unwrap();
    assert_eq!(s["cloud_base_url"], "https://relay.test/v1");
    assert_eq!(s["cloud_model"], "some-model");
    assert_eq!(s["default_chat_mode"], "cloud");
    // the key went to the keystore, never into the settings blob
    assert!(!serde_json::to_string(&s).unwrap().contains("sekret"));
}

#[test]
fn delete_paint_model_removes_installed_weights() {
    let (api, dir, _b) = api_with_book();
    let paint = dir.path().join("models/paint");
    std::fs::create_dir_all(&paint).unwrap();
    let file = paint.join("stable-diffusion-v1-5-pruned-emaonly-Q8_0.gguf");
    std::fs::write(&file, b"weights").unwrap();
    let r = api.delete_paint_model("sketch").unwrap();
    assert_eq!(r["deleted"], true);
    assert!(!file.exists());
    // deleting again is a no-op, unknown tier errors
    assert_eq!(api.delete_paint_model("sketch").unwrap()["deleted"], false);
    assert!(api.delete_paint_model("bogus").is_err());
}

#[test]
fn import_book_data_imports_a_base64_text_upload() {
    let (api, _d, _b) = api_with_book();
    let para = vec!["word"; 200].join(" ");
    let text = format!("Uploaded Novel\nby Someone\n\nCHAPTER I\n\n{para}\n");
    let b64 = {
        // reuse the app's own encoder path via a round-trip through get_asset? no —
        // build standard base64 here.
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = text.as_bytes();
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            out.push(T[(b[0] >> 2) as usize] as char);
            out.push(T[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 {
                T[(((b[1] & 15) << 2) | (b[2] >> 6)) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                T[(b[2] & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    };
    let meta = api
        .import_book_data("uploaded.txt", &b64, |_, _| {})
        .unwrap();
    assert_eq!(meta.profile, "prose");
    assert!(meta.title.to_lowercase().contains("uploaded"));
}

#[test]
fn get_book_and_episode_error_on_unknown_id() {
    let (api, _d, _b) = api_with_book();
    assert!(api.get_book(999_999).is_err());
    assert!(api.get_episode(999_999, 1).is_err());
}

#[test]
fn auto_paint_generates_typographic_covers_without_a_paint_engine() {
    let (api, dir, book_id) = api_with_book();
    api.set_progress(book_id, 2, 0).unwrap();
    let res = api.auto_paint().unwrap();
    // at least the seeded prose book gets a cover; none are portraits (no paint engine)
    assert!(res["covers"].as_i64().unwrap() >= 1, "auto_paint: {res}");
    assert_eq!(res["portraits"].as_i64().unwrap(), 0);
    // the cover is a real typographic SVG on disk
    let svg = dir
        .path()
        .join("assets")
        .join(format!("cover-{book_id}.svg"));
    assert!(svg.exists(), "typographic cover written");
}
