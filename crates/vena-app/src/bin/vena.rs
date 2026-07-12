//! The Vena desktop/mobile binary — Tauri 2 shell. Every command is a thin wrapper
//! over `vena_app::AppApi` (where the real logic + tests live). Events emitted:
//! forge:progress, forge:done, companion:stage, companion:token, companion:done,
//! model:progress, store:progress, image:progress, image:done (§11.2).
//!
//! Build on a dev machine: `cargo build -p vena-app --features tauri --bin vena`

use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use vena_app::api::{AiStatus, AppApi, ImageStatus, RelayTest, StoreItem};
use vena_core::engine::ProbeResult;
use vena_core::model::{BookMeta, Character, EpisodeHtml, Theory, TurnReport};
use vena_core::wiki::{WikiIndex, WikiPage};
use vena_core::VenaError;

type Api = Arc<AppApi>;

/// OS-keychain keystore (macOS Keychain / Windows Credential Manager / libsecret;
/// Keychain/Keystore on mobile). Keys NEVER touch SQLite or logs (§11.4).
struct KeychainStore;
impl vena_app::KeyStore for KeychainStore {
    fn set(&self, key: &str, secret: &str) -> vena_core::Result<()> {
        keyring::Entry::new("vena", key)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| VenaError::Other(format!("keychain: {e}")))
    }
    fn get(&self, key: &str) -> vena_core::Result<Option<String>> {
        match keyring::Entry::new("vena", key).and_then(|e| e.get_password()) {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(VenaError::Other(format!("keychain: {e}"))),
        }
    }
    fn delete(&self, key: &str) -> vena_core::Result<()> {
        let _ = keyring::Entry::new("vena", key).and_then(|e| e.delete_credential());
        Ok(())
    }
}

// ============================ Library ============================

#[tauri::command]
async fn import_book(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    path: String,
) -> Result<BookMeta, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        api.import_book(&path, |pct, stage| {
            let _ = handle.emit(
                "forge:progress",
                serde_json::json!({ "pct": pct, "stage": stage }),
            );
        })
        .inspect(|meta| {
            let _ = handle.emit(
                "forge:done",
                serde_json::json!({ "bookId": meta.id, "ledgerCoverage": meta.ledger_coverage }),
            );
        })
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
fn list_books(api: State<'_, Api>) -> Result<Vec<BookMeta>, VenaError> {
    api.list_books()
}

#[tauri::command]
fn delete_book(api: State<'_, Api>, id: i64) -> Result<(), VenaError> {
    api.delete_book(id)
}

// ============================ Reading ============================

#[tauri::command]
fn get_episode(api: State<'_, Api>, book_id: i64, seq: i64) -> Result<EpisodeHtml, VenaError> {
    api.get_episode(book_id, seq)
}

#[tauri::command]
fn set_progress(
    api: State<'_, Api>,
    book_id: i64,
    episode_seq: i64,
    scene_seq: i64,
) -> Result<(), VenaError> {
    api.set_progress(book_id, episode_seq, scene_seq)
}

#[tauri::command]
fn set_serial_mode(
    api: State<'_, Api>,
    book_id: i64,
    enabled: bool,
    minutes_per_episode: i64,
) -> Result<(), VenaError> {
    api.set_serial_mode(book_id, enabled, minutes_per_episode)
}

// ============================ Companion ============================

#[tauri::command]
async fn companion_turn(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    book_id: i64,
    character_id: Option<i64>,
    message: String,
    turn_id: Option<i64>,
) -> Result<TurnReport, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Echo the UI-supplied turnId so companion:stage events correlate with the
        // caller's turn (the UI drops events whose turnId != its own counter).
        let turn_id = turn_id.unwrap_or(0);
        let report = api.companion_turn(book_id, character_id, &message, &mut |stage| {
            let _ = handle.emit(
                "companion:stage",
                serde_json::json!({ "turnId": turn_id, "stage": stage }),
            );
        })?;
        let _ = handle.emit(
            "companion:token",
            serde_json::json!({ "turnId": turn_id, "text": report.reply }),
        );
        let _ = handle.emit(
            "companion:done",
            serde_json::json!({ "turnId": turn_id, "report": report }),
        );
        Ok(report)
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
fn list_characters(api: State<'_, Api>, book_id: i64) -> Result<Vec<Character>, VenaError> {
    api.list_characters(book_id)
}

#[tauri::command]
fn who_is(api: State<'_, Api>, book_id: i64, name: String) -> Result<Character, VenaError> {
    api.who_is(book_id, &name)
}

#[tauri::command]
async fn get_recap(api: State<'_, Api>, book_id: i64) -> Result<String, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api.get_recap(book_id))
        .await
        .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn run_probes(
    api: State<'_, Api>,
    book_id: i64,
    n: usize,
) -> Result<Vec<ProbeResult>, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api.run_probes(book_id, n))
        .await
        .map_err(|e| VenaError::Other(e.to_string()))?
}

// ============================ Theories ============================

#[tauri::command]
fn add_theory(api: State<'_, Api>, book_id: i64, text: String) -> Result<Theory, VenaError> {
    api.add_theory(book_id, &text)
}

#[tauri::command]
fn list_theories(api: State<'_, Api>, book_id: i64) -> Result<Vec<Theory>, VenaError> {
    api.list_theories(book_id)
}

// ============================ Archive ============================

#[tauri::command]
fn get_wiki_index(api: State<'_, Api>, book_id: i64, mode: String) -> Result<WikiIndex, VenaError> {
    api.get_wiki_index(book_id, &mode)
}

#[tauri::command]
fn get_wiki_page(
    api: State<'_, Api>,
    book_id: i64,
    entity_id: String,
    mode: String,
) -> Result<WikiPage, VenaError> {
    api.get_wiki_page(book_id, &entity_id, &mode)
}

#[tauri::command]
fn set_spoiler_consent(api: State<'_, Api>, book_id: i64, granted: bool) -> Result<(), VenaError> {
    api.set_spoiler_consent(book_id, granted)
}

// ============================ Store ============================

#[tauri::command]
async fn store_search(api: State<'_, Api>, query: String) -> Result<Vec<StoreItem>, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api.store_search(&query))
        .await
        .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn store_browse(
    api: State<'_, Api>,
    source: String,
    cursor: Option<String>,
) -> Result<Vec<StoreItem>, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api.store_browse(&source, cursor.as_deref()))
        .await
        .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn store_download(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    item: StoreItem,
) -> Result<BookMeta, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        api.store_download(&item, |pct, phase| {
            let _ = handle.emit(
                "store:progress",
                serde_json::json!({ "jobId": item.id, "pct": pct, "phase": phase }),
            );
        })
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
fn add_opds_catalog(api: State<'_, Api>, url: String, name: String) -> Result<String, VenaError> {
    api.add_opds_catalog(&url, &name)
}

#[tauri::command]
fn remove_opds_catalog(api: State<'_, Api>, id: String) -> Result<(), VenaError> {
    api.remove_opds_catalog(&id)
}

#[tauri::command]
fn list_opds_catalogs(api: State<'_, Api>) -> Result<serde_json::Value, VenaError> {
    api.list_opds_catalogs()
}

#[tauri::command]
async fn import_ao3_link(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    url: String,
) -> Result<BookMeta, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        api.import_ao3_link(&url, |pct, phase| {
            let _ = handle.emit(
                "store:progress",
                serde_json::json!({ "jobId": "ao3", "pct": pct, "phase": phase }),
            );
        })
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

// ============================ Leak reports / forge / images ============================

#[tauri::command]
fn report_leak(
    api: State<'_, Api>,
    book_id: i64,
    reason: String,
    excerpt: String,
    comment: String,
) -> Result<(), VenaError> {
    api.report_leak(book_id, &reason, &excerpt, &comment)
}

#[tauri::command]
async fn forge_ledger(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    book_id: i64,
) -> Result<BookMeta, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let meta = api.forge_ledger(book_id, |pct, stage| {
            let _ = handle.emit(
                "forge:progress",
                serde_json::json!({ "bookId": book_id, "pct": pct, "stage": stage }),
            );
        })?;
        let _ = handle.emit(
            "forge:done",
            serde_json::json!({ "bookId": meta.id, "ledgerCoverage": meta.ledger_coverage }),
        );
        Ok(meta)
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn generate_portrait(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    book_id: i64,
    character_id: i64,
) -> Result<String, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = api.generate_portrait(book_id, character_id, |pct| {
            let _ = handle.emit(
                "image:progress",
                serde_json::json!({ "jobId": format!("portrait-{character_id}"), "pct": pct }),
            );
        })?;
        let _ = handle.emit(
            "image:done",
            serde_json::json!({ "jobId": format!("portrait-{character_id}"), "assetPath": path }),
        );
        Ok(path)
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn generate_cover(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    book_id: i64,
    regenerate: Option<bool>,
) -> Result<String, VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = api.generate_cover(book_id, regenerate.unwrap_or(false), |pct| {
            let _ = handle.emit(
                "image:progress",
                serde_json::json!({ "jobId": format!("cover-{book_id}"), "pct": pct }),
            );
        })?;
        let _ = handle.emit(
            "image:done",
            serde_json::json!({ "jobId": format!("cover-{book_id}"), "assetPath": path }),
        );
        Ok(path)
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
fn lookup_word(
    api: State<'_, Api>,
    term: String,
    lang: String,
) -> Result<serde_json::Value, VenaError> {
    api.lookup_word(&term, &lang)
}

#[tauri::command]
async fn translate_selection(
    api: State<'_, Api>,
    book_id: i64,
    text: String,
    target_lang: String,
) -> Result<String, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        api.translate_selection(book_id, &text, &target_lang)
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

// ============================ Models & settings ============================

#[tauri::command]
fn get_ai_status(api: State<'_, Api>) -> Result<AiStatus, VenaError> {
    api.get_ai_status()
}

#[tauri::command]
fn set_api_config(
    api: State<'_, Api>,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<(), VenaError> {
    api.set_api_config(&base_url, &api_key, &model)
}

#[tauri::command]
fn set_image_config(
    api: State<'_, Api>,
    base_url: String,
    api_key: String,
    model: String,
) -> Result<(), VenaError> {
    api.set_image_config(&base_url, &api_key, &model)
}

#[tauri::command]
fn set_chat_mode(api: State<'_, Api>, mode: String) -> Result<(), VenaError> {
    api.set_chat_mode(&mode)
}

#[tauri::command]
async fn test_relay(api: State<'_, Api>) -> Result<RelayTest, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api.test_relay())
        .await
        .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn list_relay_models(api: State<'_, Api>) -> Result<Vec<String>, VenaError> {
    let api = api.inner().clone();
    tauri::async_runtime::spawn_blocking(move || api.list_relay_models())
        .await
        .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
async fn download_local_model(
    app: tauri::AppHandle,
    api: State<'_, Api>,
    tier: String,
) -> Result<(), VenaError> {
    let api = api.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let tier_name = tier.clone();
        api.download_local_model(&tier, |pct| {
            let _ = handle.emit(
                "model:progress",
                serde_json::json!({ "tier": tier_name, "pct": pct }),
            );
        })
    })
    .await
    .map_err(|e| VenaError::Other(e.to_string()))?
}

#[tauri::command]
fn get_settings(api: State<'_, Api>) -> Result<serde_json::Value, VenaError> {
    api.get_settings()
}

#[tauri::command]
fn set_setting(api: State<'_, Api>, key: String, value: String) -> Result<(), VenaError> {
    api.set_setting(&key, &value)
}

#[tauri::command]
fn get_image_status(api: State<'_, Api>) -> Result<ImageStatus, VenaError> {
    api.get_image_status()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("app data dir");
            let api = AppApi::new(data_dir, Box::new(KeychainStore)).expect("open profile");
            app.manage(Arc::new(api));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            import_book,
            list_books,
            delete_book,
            get_episode,
            set_progress,
            set_serial_mode,
            companion_turn,
            list_characters,
            who_is,
            get_recap,
            run_probes,
            add_theory,
            list_theories,
            get_wiki_index,
            get_wiki_page,
            set_spoiler_consent,
            store_search,
            store_browse,
            store_download,
            add_opds_catalog,
            remove_opds_catalog,
            list_opds_catalogs,
            import_ao3_link,
            report_leak,
            forge_ledger,
            generate_portrait,
            generate_cover,
            lookup_word,
            translate_selection,
            get_ai_status,
            set_api_config,
            set_image_config,
            set_chat_mode,
            test_relay,
            list_relay_models,
            download_local_model,
            get_settings,
            set_setting,
            get_image_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vena");
}
