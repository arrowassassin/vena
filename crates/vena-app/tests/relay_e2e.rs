//! Cloud Relay END-TO-END over real HTTP: a local OpenAI-compatible stub plays
//! the provider, and the full path is exercised — config (key → keystore, never
//! the db), TEST THE RELAY's measured gate check, and a companion turn whose
//! first draft leaks a future fact and gets repaired. Every request body the
//! "provider" ever sees is captured and checked against the forbidden ledger:
//! nothing past the reader's bookmark may leave the device.

use std::io::Read;
use std::sync::{Arc, Mutex};

use vena_app::api::AppApi;
use vena_app::keystore::MemoryKeyStore;

/// Scripted OpenAI-compatible provider. Returns queued chat replies and
/// records every request body it receives.
struct StubProvider {
    port: u16,
    bodies: Arc<Mutex<Vec<String>>>,
    queue: Arc<Mutex<std::collections::VecDeque<String>>>,
    _thread: std::thread::JoinHandle<()>,
}

fn start_stub(replies: Vec<String>) -> StubProvider {
    let server = tiny_http::Server::http(("127.0.0.1", 0)).expect("bind stub");
    let port = server.server_addr().to_ip().unwrap().port();
    let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let bodies2 = bodies.clone();
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(replies)));
    let queue_srv = queue.clone();
    let thread = std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            if !body.is_empty() {
                bodies2.lock().unwrap().push(body);
            }
            let url = req.url().to_string();
            let json = if url.contains("/models") {
                r#"{"data":[{"id":"stub-model"}]}"#.to_string()
            } else {
                let reply = queue_srv
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| "I cannot say more than the pages have shown.".into());
                serde_json::json!({
                    "choices": [{ "message": { "role": "assistant", "content": reply } }]
                })
                .to_string()
            };
            let hdr = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                .unwrap();
            let _ = req.respond(tiny_http::Response::from_string(json).with_header(hdr));
        }
    });
    StubProvider {
        port,
        bodies,
        queue,
        _thread: thread,
    }
}

fn dracula_vena() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/packages/dracula.vena")
}

#[test]
fn relay_configures_tests_and_repairs_over_real_http() {
    let stub = start_stub(vec!["The relay hears you.".into()]);
    let dir = tempfile::tempdir().unwrap();
    std::env::remove_var("VENA_BASE_URL"); // the env override must not mask the config
    let api = AppApi::new(
        dir.path().to_path_buf(),
        Box::new(MemoryKeyStore::default()),
    )
    .unwrap();

    // A real sealed book: the bundled pre-forged Dracula package.
    let book_id = vena_core::pkg::import_vena(&api.store_for_tests(), &dracula_vena()).unwrap();
    api.set_progress(book_id, 6, 0).unwrap();

    // ---- configure: key goes to the KEYSTORE, never the settings table ----
    let base = format!("http://127.0.0.1:{}", stub.port);
    api.set_api_config(&base, "sk-secret-key", "stub-model")
        .unwrap();
    api.set_chat_mode("cloud").unwrap();
    let settings = api.get_settings().unwrap();
    assert_eq!(settings["cloud_base_url"], base.as_str());
    assert!(
        !serde_json::to_string(&settings)
            .unwrap()
            .contains("sk-secret-key"),
        "the API key must never appear in settings"
    );

    // ---- TEST THE RELAY: real round trip + measured local gate ----
    let t = api.test_relay().unwrap();
    assert!(t.ok, "relay test should pass: {t:?}");
    assert!(t.gate_verified, "the gate must be measured, not asserted");

    // ---- companion turn through the wire: leak → verify → repair ----
    // The "leak" quotes a REAL forbidden fact from this ledger, so the verify
    // stage must catch it; the queued regeneration is clean.
    let leak_fact = {
        let store = api.store_for_tests();
        store.forbidden_facts(book_id, 6, None).unwrap()[0]
            .text
            .clone()
    };
    {
        let mut q = stub.queue.lock().unwrap();
        q.push_back(format!("Mark this well, for it is coming: {leak_fact}."));
        q.push_back("It troubles me greatly; I cannot say how it will end.".into());
    }
    let report = api
        .companion_turn(
            book_id,
            None,
            "What do you make of Lucy's illness?",
            &mut |_| {},
        )
        .unwrap();
    assert!(
        report.repaired,
        "the leaky draft must be repaired: {report:?}"
    );
    assert!(
        !report.reply.contains(&leak_fact),
        "final reply leaked: {}",
        report.reply
    );

    // ---- the invariant: nothing FORBIDDEN ever left the device ----
    let forbidden: Vec<String> = {
        let store = api.store_for_tests();
        store
            .forbidden_facts(book_id, 6, None)
            .unwrap()
            .into_iter()
            .map(|f| f.text)
            .collect()
    };
    assert!(!forbidden.is_empty(), "fixture sanity: future facts exist");
    let bodies = stub.bodies.lock().unwrap();
    assert!(bodies.len() >= 3, "expected real HTTP traffic");
    for (i, body) in bodies.iter().enumerate() {
        // A loopback endpoint is ON-DEVICE by address, so the repair stage is
        // allowed to disclose forbidden topics to it ("you do not know X yet")
        // — that text never leaves the machine. The REMOTE branch (no
        // disclosure at all) is covered by engine::remote_repair_discloses_
        // nothing in vena-core. Every other request must be spotless.
        let is_local_repair = body.contains("You do NOT yet know");
        if is_local_repair {
            continue;
        }
        for fact in &forbidden {
            assert!(
                !body.contains(fact.as_str()),
                "forbidden fact was sent to the relay (request #{i}): {fact}"
            );
        }
    }
    // And the COMPOSE request itself (pre-draft) must be clean of ALL of them,
    // including the leak fact — the gate runs before anything is sent.
    let compose = bodies
        .iter()
        .find(|b| b.contains("Lucy's illness"))
        .expect("compose request captured");
    for fact in &forbidden {
        assert!(
            !compose.contains(fact.as_str()),
            "forbidden fact entered the COMPOSE prompt: {fact}"
        );
    }
}
