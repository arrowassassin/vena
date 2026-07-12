//! vena-devserver — the §11.2 IPC surface over localhost HTTP + SSE, backed by the
//! REAL AppApi/engine. Lets the React UI run in a plain browser against real data
//! (dev + CI). The shipped app uses the Tauri binary; this bridge is never bundled.
//!
//!   POST /api/<command>       body: JSON args → JSON result (VenaError → {code,message})
//!   GET  /api/events          SSE stream: forge:progress, companion:stage, …
//!   GET  /<path>              static files from ui/dist (when built)

use std::io::Read;
use std::sync::{Arc, Mutex};
use vena_app::api::{AppApi, StoreItem};
use vena_app::MemoryKeyStore;

type Events = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

fn main() {
    let port: u16 = std::env::var("VENA_DEV_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5714);
    let data_dir = std::env::var("VENA_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("vena-dev"));
    let api =
        Arc::new(AppApi::new(data_dir, Box::new(MemoryKeyStore::default())).expect("open profile"));
    let events: Events = Arc::new(Mutex::new(Vec::new()));

    let server = tiny_http::Server::http(("127.0.0.1", port)).expect("bind");
    eprintln!("vena-devserver listening on http://127.0.0.1:{port} (real engine, no mocks)");

    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        let method = req.method().clone();
        let api = api.clone();
        let events = events.clone();

        // SSE poll endpoint: return-and-clear queued events (simple long-poll SSE).
        if url.starts_with("/api/events") {
            let drained: Vec<_> = std::mem::take(&mut *events.lock().unwrap());
            let body: String = drained
                .into_iter()
                .map(|(name, payload)| format!("event: {name}\ndata: {payload}\n\n"))
                .collect();
            let _ = req.respond(with_cors(
                tiny_http::Response::from_string(body).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/event-stream"[..])
                        .unwrap(),
                ),
            ));
            continue;
        }

        if url.starts_with("/api/") {
            if method == tiny_http::Method::Options {
                let _ = req.respond(with_cors(tiny_http::Response::from_string("")));
                continue;
            }
            let mut body = String::new();
            let _ = req.as_reader().read_to_string(&mut body);
            let args: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let cmd = url
                .trim_start_matches("/api/")
                .split('?')
                .next()
                .unwrap_or("");
            let result = dispatch(&api, &events, cmd, &args);
            let (status, payload) = match result {
                Ok(v) => (200, v),
                Err(e) => (
                    400,
                    serde_json::json!({ "code": e.code(), "message": e.to_string() }),
                ),
            };
            let resp = tiny_http::Response::from_string(payload.to_string())
                .with_status_code(status)
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap(),
                );
            let _ = req.respond(with_cors(resp));
            continue;
        }

        // Static UI (ui/dist) with SPA fallback.
        let ui_root = ui_dist();
        let rel = url.trim_start_matches('/').split('?').next().unwrap_or("");
        let candidate = if rel.is_empty() { "index.html" } else { rel };
        let path = ui_root.join(candidate);
        let path = if path.is_file() {
            path
        } else {
            ui_root.join("index.html")
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let mime = match path.extension().and_then(|e| e.to_str()) {
                    Some("html") => "text/html",
                    Some("js") => "application/javascript",
                    Some("css") => "text/css",
                    Some("svg") => "image/svg+xml",
                    Some("png") => "image/png",
                    Some("woff2") => "font/woff2",
                    _ => "application/octet-stream",
                };
                let resp = tiny_http::Response::from_data(bytes).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap(),
                );
                let _ = req.respond(resp);
            }
            Err(_) => {
                let _ = req.respond(
                    tiny_http::Response::from_string("ui not built — run npm run build in ui/")
                        .with_status_code(404),
                );
            }
        }
    }
}

fn with_cors(
    r: tiny_http::Response<std::io::Cursor<Vec<u8>>>,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    r.with_header(
        tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
    )
    .with_header(
        tiny_http::Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"content-type"[..])
            .unwrap(),
    )
}

fn ui_dist() -> std::path::PathBuf {
    std::env::var("VENA_UI_DIST")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist")
        })
}

fn push(events: &Events, name: &str, payload: serde_json::Value) {
    events.lock().unwrap().push((name.to_string(), payload));
}

fn jv<T: serde::Serialize>(v: T) -> vena_core::Result<serde_json::Value> {
    serde_json::to_value(v).map_err(Into::into)
}

/// One dispatcher, same command names as the Tauri handler — the UI's api.ts calls
/// these identically through either transport.
fn dispatch(
    api: &Arc<AppApi>,
    events: &Events,
    cmd: &str,
    a: &serde_json::Value,
) -> vena_core::Result<serde_json::Value> {
    use vena_core::VenaError;
    let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
    let i = |k: &str| a[k].as_i64().unwrap_or(0);

    match cmd {
        "import_book" => {
            let ev = events.clone();
            let meta = api.import_book(&s("path"), |pct, stage| {
                push(
                    &ev,
                    "forge:progress",
                    serde_json::json!({ "pct": pct, "stage": stage }),
                );
            })?;
            push(
                events,
                "forge:done",
                serde_json::json!({ "bookId": meta.id, "ledgerCoverage": meta.ledger_coverage }),
            );
            jv(meta)
        }
        "list_books" => jv(api.list_books()?),
        "delete_book" => jv(api.delete_book(i("id"))?),
        "get_episode" => jv(api.get_episode(i("bookId"), i("seq"))?),
        "set_progress" => jv(api.set_progress(i("bookId"), i("episodeSeq"), i("sceneSeq"))?),
        "set_serial_mode" => jv(api.set_serial_mode(
            i("bookId"),
            a["enabled"].as_bool().unwrap_or(false),
            a["minutesPerEpisode"].as_i64().unwrap_or(20),
        )?),
        "companion_turn" => {
            let ev = events.clone();
            let turn_id = i("turnId");
            let character_id = a["characterId"].as_i64();
            let report =
                api.companion_turn(i("bookId"), character_id, &s("message"), &mut |st| {
                    push(
                        &ev,
                        "companion:stage",
                        serde_json::json!({ "turnId": turn_id, "stage": st }),
                    );
                })?;
            push(
                events,
                "companion:done",
                serde_json::json!({ "turnId": turn_id, "report": report }),
            );
            jv(report)
        }
        "list_characters" => jv(api.list_characters(i("bookId"))?),
        "who_is" => jv(api.who_is(i("bookId"), &s("name"))?),
        "get_recap" => jv(api.get_recap(i("bookId"))?),
        "run_probes" => jv(api.run_probes(i("bookId"), a["n"].as_u64().unwrap_or(12) as usize)?),
        "add_theory" => jv(api.add_theory(i("bookId"), &s("text"))?),
        "list_theories" => jv(api.list_theories(i("bookId"))?),
        "get_wiki_index" => jv(api.get_wiki_index(i("bookId"), &s("mode"))?),
        "get_wiki_page" => jv(api.get_wiki_page(i("bookId"), &s("entityId"), &s("mode"))?),
        "set_spoiler_consent" => {
            jv(api.set_spoiler_consent(i("bookId"), a["granted"].as_bool().unwrap_or(false))?)
        }
        "store_search" => jv(api.store_search(&s("query"))?),
        "store_browse" => jv(api.store_browse(&s("source"), a["cursor"].as_str())?),
        "store_download" => {
            let item: StoreItem = serde_json::from_value(a["item"].clone())?;
            let ev = events.clone();
            let id = item.id.clone();
            let meta = api.store_download(&item, |pct, phase| {
                push(
                    &ev,
                    "store:progress",
                    serde_json::json!({ "jobId": id, "pct": pct, "phase": phase }),
                );
            })?;
            jv(meta)
        }
        "add_opds_catalog" => jv(api.add_opds_catalog(&s("url"), &s("name"))?),
        "remove_opds_catalog" => jv(api.remove_opds_catalog(&s("id"))?),
        "list_opds_catalogs" => jv(api.list_opds_catalogs()?),
        "import_ao3_link" => {
            let ev = events.clone();
            let meta = api.import_ao3_link(&s("url"), |pct, phase| {
                push(
                    &ev,
                    "store:progress",
                    serde_json::json!({ "jobId": "ao3", "pct": pct, "phase": phase }),
                );
            })?;
            jv(meta)
        }
        "get_ai_status" => jv(api.get_ai_status()?),
        "set_api_config" => jv(api.set_api_config(&s("baseUrl"), &s("apiKey"), &s("model"))?),
        "set_image_config" => jv(api.set_image_config(&s("baseUrl"), &s("apiKey"), &s("model"))?),
        "set_chat_mode" => jv(api.set_chat_mode(&s("mode"))?),
        "test_relay" => jv(api.test_relay()?),
        "list_relay_models" => jv(api.list_relay_models()?),
        "download_local_model" => {
            let ev = events.clone();
            let r = api.download_local_model(&s("tier"), |pct| {
                push(&ev, "model:progress", serde_json::json!({ "pct": pct }));
            })?;
            jv(r)
        }
        "get_settings" => api.get_settings(),
        "set_setting" => jv(api.set_setting(&s("key"), &s("value"))?),
        "get_image_status" => jv(api.get_image_status()?),
        other => Err(VenaError::NotFound(format!("command {other}"))),
    }
}
