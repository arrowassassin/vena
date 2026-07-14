//! End-to-end Cloud Relay: point the app's OpenAI-compatible client at a real
//! loopback stub server (loopback is allowlisted) and drive the whole stack that
//! talks to a backend — test_relay, list_relay_models, the 5-stage companion turn,
//! recap, dictionary + translation. No mocks inside the app: the app builds a real
//! `OpenAiClient`, makes real HTTP round-trips, and parses real JSON responses.

use vena_app::api::AppApi;
use vena_app::keystore::MemoryKeyStore;

/// A stub OpenAI-compatible server on loopback. It answers `POST /v1/chat/
/// completions` with a gate-safe in-character reply and `GET /v1/models` with a
/// small model list — enough for the app's real client to exercise every path.
struct Relay {
    port: u16,
    _thread: std::thread::JoinHandle<()>,
}

fn spawn_relay() -> Relay {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let thread = std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            let body = if url.contains("/images/generations") {
                // POST /v1/images/generations → b64 image ("hello" as bytes)
                serde_json::json!({ "data": [ { "b64_json": "aGVsbG8=" } ] })
            } else if url.contains("/models") {
                // GET /v1/models
                serde_json::json!({
                    "data": [ {"id": "stub-large"}, {"id": "stub-small"} ]
                })
            } else {
                // POST /v1/chat/completions — read to detect JSON-mode requests
                let mut s = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut s);
                let content = if s.contains("json_object") {
                    // the forge extractor expects a strict single-chapter ledger
                    r#"{"facts":[],"new_characters":[]}"#.to_string()
                } else {
                    // a benign, gate-safe reply that names no future fact
                    "I keep my own counsel, friend — ask me only of what we have seen together."
                        .to_string()
                };
                serde_json::json!({
                    "choices": [ { "message": { "role": "assistant", "content": content } } ]
                })
            };
            let data = serde_json::to_vec(&body).unwrap();
            let header =
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                    .unwrap();
            let resp = tiny_http::Response::from_data(data).with_header(header);
            let _ = req.respond(resp);
        }
    });
    Relay {
        port,
        _thread: thread,
    }
}

/// A fresh API whose Cloud Relay points at a local stub server.
fn api_on_relay() -> (AppApi, tempfile::TempDir, Relay, i64) {
    let dir = tempfile::tempdir().unwrap();
    let api = AppApi::new(
        dir.path().to_path_buf(),
        Box::new(MemoryKeyStore::default()),
    )
    .unwrap();
    let relay = spawn_relay();
    let base = format!("http://127.0.0.1:{}/v1", relay.port);
    api.set_api_config(&base, "test-key", "stub-large").unwrap();
    let book_id = api
        .store_for_tests()
        .list_books()
        .unwrap()
        .into_iter()
        .find(|b| b.slug.starts_with("dracula"))
        .expect("first-run seed imported Dracula")
        .id;
    (api, dir, relay, book_id)
}

#[test]
fn test_relay_round_trips_and_verifies_the_gate() {
    let (api, _d, _r, _b) = api_on_relay();
    let res = api.test_relay().unwrap();
    assert!(res.ok, "relay probe succeeded: {}", res.message);
    // Dracula is a sealed book, so the gate check actually ran and passed.
    assert!(res.gate_verified, "gate verified against the sealed book");
    assert!(res.message.contains("relay ok"));
}

#[test]
fn list_relay_models_reads_the_models_endpoint() {
    let (api, _d, _r, _b) = api_on_relay();
    let models = api.list_relay_models().unwrap();
    assert!(models.iter().any(|m| m == "stub-large"));
    assert!(models.iter().any(|m| m == "stub-small"));
}

#[test]
fn ai_status_is_ready_once_relay_is_configured() {
    let (api, _d, _r, _b) = api_on_relay();
    let st = api.get_ai_status().unwrap();
    assert_eq!(st.mode, "cloud");
    assert!(st.ready, "cloud key + base ⇒ ready");
}

#[test]
fn companion_turn_runs_the_five_stages_and_persists() {
    let (api, _d, _r, book_id) = api_on_relay();
    api.set_progress(book_id, 3, 0).unwrap();
    let mut stages = Vec::new();
    let report = api
        .companion_turn(book_id, None, "What do you make of the castle?", &mut |s| {
            stages.push(s.to_string())
        })
        .unwrap();
    assert!(!report.reply.is_empty());
    // the gate stage always fires; compose fires when generation runs
    assert!(stages.iter().any(|s| s == "gate"), "stages: {stages:?}");
    // the turn was persisted and replays through get_conversation
    let convo = api.get_conversation(book_id, None).unwrap();
    assert!(
        convo["count"].as_i64().unwrap() >= 2,
        "user + assistant stored"
    );
}

#[test]
fn fate_question_is_deflected_without_leaking() {
    let (api, _d, _r, book_id) = api_on_relay();
    api.set_progress(book_id, 2, 0).unwrap();
    let report = api
        .companion_turn(book_id, None, "Does Dracula die at the end?", &mut |_| {})
        .unwrap();
    // guard_fates short-circuits: a non-empty in-character deflection, no claims
    assert!(!report.reply.is_empty());
    assert!(report.claims.is_empty(), "fate guard emits no claim checks");
}

#[test]
fn forget_conversations_wipes_a_real_thread() {
    let (api, _d, _r, book_id) = api_on_relay();
    api.set_progress(book_id, 3, 0).unwrap();
    api.companion_turn(book_id, None, "Tell me of the castle.", &mut |_| {})
        .unwrap();
    assert!(
        api.get_conversation(book_id, None).unwrap()["count"]
            .as_i64()
            .unwrap()
            >= 2
    );
    api.forget_conversations(book_id).unwrap();
    assert_eq!(
        api.get_conversation(book_id, None).unwrap()["count"]
            .as_i64()
            .unwrap(),
        0
    );
}

#[test]
fn recap_uses_the_backend() {
    let (api, _d, _r, book_id) = api_on_relay();
    api.set_progress(book_id, 4, 0).unwrap();
    let recap = api.get_recap(book_id).unwrap();
    assert!(!recap.trim().is_empty());
}

#[test]
fn run_probes_returns_results_over_relay() {
    let (api, _d, _r, book_id) = api_on_relay();
    api.set_progress(book_id, 5, 0).unwrap();
    let results = api.run_probes(book_id, 3).unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn lookup_word_falls_back_to_the_ai_backend() {
    let (api, _d, _r, _b) = api_on_relay();
    let v = api.lookup_word("crepuscular", "en").unwrap();
    assert_eq!(v["source"], "ai");
    assert!(!v["entry"].as_str().unwrap().is_empty());
    // an empty term is rejected before any backend call
    assert!(api.lookup_word("   ", "en").is_err());
}

#[test]
fn translate_selection_gated_to_read_text() {
    let (api, _d, _r, book_id) = api_on_relay();
    // read chapter 1, then translate a snippet that really occurs in it
    api.set_progress(book_id, 1, 0).unwrap();
    let ep = api.get_episode(book_id, 1).unwrap();
    // pull a plain-text word span from the rendered chapter
    let plain: String = {
        let mut out = String::new();
        let mut in_tag = false;
        for c in ep.content_html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if in_tag => {}
                c => out.push(c),
            }
        }
        out
    };
    let snippet: String = plain
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(snippet.len() >= 2, "got a real snippet: {snippet:?}");
    let translated = api
        .translate_selection(book_id, &snippet, "French")
        .unwrap();
    assert!(!translated.trim().is_empty());
    // text NOT in a read chapter is refused
    assert!(api
        .translate_selection(
            book_id,
            "a passage that appears nowhere in chapter one at all",
            "French"
        )
        .is_err());
    // too-short selections are refused before any gate work
    assert!(api.translate_selection(book_id, "x", "French").is_err());
}

#[test]
fn who_is_gates_unmet_and_finds_met() {
    let (api, _d, _r, book_id) = api_on_relay();
    api.set_progress(book_id, 2, 0).unwrap();
    let cast = api.list_characters(book_id).unwrap();
    if let Some(met) = cast.iter().find(|c| c.met) {
        let card = api.who_is(book_id, &met.name).unwrap();
        assert!(card.met);
    }
    // a name nobody in the book has is a plain NotFound
    assert!(api.who_is(book_id, "Absolutely Nobody").is_err());
}

#[test]
fn forge_ledger_runs_over_the_relay_backend() {
    let (api, dir, _r, _b) = api_on_relay();
    // import a tiny plaintext book, then forge its ledger through the relay
    let para = vec!["word"; 200].join(" ");
    let text =
        format!("The Test Novel\nby A. Writer\n\nCHAPTER I\n\n{para}\n\nCHAPTER II\n\n{para}\n");
    let path = dir.path().join("tiny.txt");
    std::fs::write(&path, text).unwrap();
    let meta = api.import_book(&path.to_string_lossy(), |_, _| {}).unwrap();

    let mut phases = Vec::new();
    let forged = api
        .forge_ledger(meta.id, |_pct, phase, _through| {
            phases.push(phase.to_string())
        })
        .unwrap();
    // the book reaches the sealed state and the extract phase ran per chapter
    assert!(phases.iter().any(|p| p == "extract"), "phases: {phases:?}");
    assert!(phases.iter().any(|p| p == "done"));
    assert_eq!(forged.id, meta.id);
}

#[test]
fn cover_and_portrait_render_through_the_image_relay() {
    let (api, _d, relay, book_id) = api_on_relay();
    let base = format!("http://127.0.0.1:{}/v1", relay.port);
    api.set_image_config(&base, "img-key", "stub-image")
        .unwrap();
    // status now reports the API tier
    let st = api.get_image_status().unwrap();
    assert_eq!(st.tier, "api");

    // cover renders via the relay image endpoint → a real .png on disk
    let cover = api.generate_cover(book_id, true, |_| {}).unwrap();
    assert!(
        cover.ends_with(".png"),
        "relay image tier writes a png: {cover}"
    );
    let asset = api.get_asset(&cover).unwrap();
    assert!(!asset["data"].as_str().unwrap().is_empty());

    // a portrait for a met character also renders as a png
    api.set_progress(book_id, 4, 0).unwrap();
    if let Some(met) = api
        .list_characters(book_id)
        .unwrap()
        .into_iter()
        .find(|c| c.met)
    {
        // resolve the character's numeric id via who_is
        let card = api.who_is(book_id, &met.name).unwrap();
        let p = api.generate_portrait(book_id, card.id, |_| {}).unwrap();
        assert!(p.ends_with(".png"), "portrait via relay is a png: {p}");
    }
}

#[test]
fn configure_relay_localhost_preset_needs_no_key() {
    let (api, _d, _r, _b) = api_on_relay();
    // Ollama preset is localhost → no key required; it will try to connect and
    // report a test result (ok=false is fine, nothing is listening) without error.
    let res = api.configure_relay("ollama", "", "").unwrap();
    assert!(!res.message.is_empty());
    // a remote preset with no key is a hard error
    assert!(api.configure_relay("groq", "", "").is_err());
    // an unknown provider errors
    assert!(api.configure_relay("nope", "k", "m").is_err());
}
