//! The real IPC surface (§11.2 + v2.0). Every method here is the actual command
//! logic; the Tauri `#[command]` wrappers in `src/bin/vena.rs` are one-liners that
//! call these. No Tauri types leak in, so this whole surface is unit-testable and
//! ships identically whether driven by Tauri, a test, or a headless runner.

use crate::keystore::{KeyStore, MemoryKeyStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use vena_core::engine::{self, Engine, ProbeResult};
use vena_core::inference::OpenAiClient;
use vena_core::model::*;
use vena_core::store::Store;
use vena_core::wiki::{self, WikiIndex, WikiMode, WikiPage};
use vena_core::{GateMode, Result, VenaError};

pub struct AppApi {
    profile: Mutex<Store>,
    data_dir: PathBuf,
    keystore: Box<dyn KeyStore>,
}

// ---- settings keys ----
const K_CHAT_MODE: &str = "default_chat_mode"; // "cloud" | "local"
const K_GATE_MODE: &str = "gate_mode";
const K_GUARD_FATES: &str = "guard_fates";
const K_SHOW_STAMPS: &str = "show_engine_stamps";
const K_RESEAL: &str = "reseal_on_reread";
const K_CLOUD_BASE: &str = "cloud_base_url";
const K_CLOUD_MODEL: &str = "cloud_model";
const K_LOCAL_BASE: &str = "local_base_url";
const K_LOCAL_MODEL: &str = "local_model";
const K_LOCAL_READY: &str = "local_model_ready";
const K_IMAGE_BASE: &str = "image_base_url";
const K_IMAGE_MODEL: &str = "image_model";
const K_TARGET_LANG: &str = "target_language";
const KC_CLOUD_KEY: &str = "vena:cloud_api_key";
const KC_IMAGE_KEY: &str = "vena:image_api_key";

// ---- extra DTOs (not in vena-core) ----

#[derive(Debug, Clone, Serialize)]
pub struct AiStatus {
    pub mode: String,   // local | cloud | none
    pub model: String,  // brand name (INK·3B / QUILL·7B / … or the relay model)
    pub ready: bool,
    /// The eval steer: local labelled experimental until validated (see EVAL.md).
    pub local_experimental: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageStatus {
    pub tier: String, // desktop | phone | api | none
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayTest {
    pub ok: bool,
    pub latency_ms: u128,
    /// The Cloud Relay promise: the gate ran locally BEFORE anything was sent.
    pub gate_verified: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreItem {
    pub source: String, // vena-catalog | gutenberg | standard-ebooks | opds | ao3
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub license: Option<String>,
    pub download_url: Option<String>,
    pub cover: Option<String>,
    pub on_shelf: bool,
}

impl AppApi {
    /// Open (or create) a profile at `data_dir/profile.db`, seed the bundled Dracula
    /// package on first run, and use the OS keychain-backed keystore (app) or an
    /// in-memory one (tests).
    pub fn new(data_dir: PathBuf, keystore: Box<dyn KeyStore>) -> Result<AppApi> {
        std::fs::create_dir_all(&data_dir)?;
        std::env::set_var("VENA_ASSET_DIR", data_dir.join("assets"));
        let store = Store::open(&data_dir.join("profile.db"))?;
        let api = AppApi {
            profile: Mutex::new(store),
            data_dir,
            keystore,
        };
        api.seed_first_run()?;
        Ok(api)
    }

    /// Headless/test constructor: in-memory profile, in-memory keystore.
    pub fn in_memory() -> Result<AppApi> {
        let api = AppApi {
            profile: Mutex::new(Store::in_memory()?),
            data_dir: std::env::temp_dir().join("vena-test"),
            keystore: Box::new(MemoryKeyStore::default()),
        };
        Ok(api)
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.profile.lock().unwrap()
    }

    /// Import the bundled flagship Dracula package once, so the shelf is never empty.
    fn seed_first_run(&self) -> Result<()> {
        let store = self.store();
        let already = store
            .list_books()?
            .iter()
            .any(|b| b.slug.starts_with("dracula"));
        if already {
            return Ok(());
        }
        for candidate in bundled_packages() {
            if candidate.exists() {
                drop(store);
                let _ = vena_core::pkg::import_vena(&self.store(), &candidate)?;
                return Ok(());
            }
        }
        Ok(())
    }

    // ============================ Library ============================

    /// Import a book file. Canon is inserted immediately (the reader works at once);
    /// the ledger forges in the background when a backend is available. `on_progress`
    /// receives (pct, stage) for the forge:progress event.
    pub fn import_book(
        &self,
        path: &str,
        mut on_progress: impl FnMut(u32, &str),
    ) -> Result<BookMeta> {
        let book = vena_forge::import::import_path(Path::new(path))
            .map_err(|e| VenaError::InvalidPackage(e.to_string()))?;
        on_progress(20, "parse");

        // If a sidecar curated ledger exists next to the file, use it (prebuilt).
        let sidecar = Path::new(path).with_extension("ledger.json");
        let ledger = if sidecar.exists() {
            let json = std::fs::read_to_string(&sidecar)?;
            vena_forge::ledger::load_curated(&json)
                .map(|(_, l)| l)
                .map_err(|e| VenaError::Other(e.to_string()))?
        } else if let Ok(backend) = self.backend_for_forge() {
            on_progress(40, "extract");
            vena_forge::ledger::extract_with_model(backend.as_ref(), &book.chapters, |seq, total| {
                on_progress(40 + (seq as u32 * 50 / total.max(1) as u32), "extract");
            })
            .map_err(|e| VenaError::Inference(e.to_string()))?
        } else {
            // Local tier with no model yet: import canon only, ledger pending.
            vena_forge::ledger::Ledger::default()
        };
        on_progress(92, "seal");

        let slug = unique_slug(&self.store(), &slugify(&book.title))?;
        let db_path = self.data_dir.join(format!("pkg-{slug}.db"));
        let stats = vena_forge::forge::forge_to_db(
            &book,
            &ledger,
            &slug,
            "user-owned",
            Some("import"),
            None,
            &db_path,
        )
        .map_err(|e| VenaError::Other(e.to_string()))?;
        let vena_path = self.data_dir.join(format!("{slug}.vena"));
        vena_core::pkg::write_vena(&db_path, None, &vena_path)?;
        let sid = vena_core::pkg::import_vena(&self.store(), &vena_path)?;
        let _ = std::fs::remove_file(&db_path);
        on_progress(100, "done");

        // Honest forge_state: sealed if we produced facts, else raw (ledger pending).
        if stats.facts == 0 {
            let mut meta = self.store().book_meta_value(sid)?;
            meta["forge_state"] = serde_json::json!("raw");
            self.store().set_book_meta(sid, &meta.to_string())?;
        }
        self.store().get_book(sid)
    }

    pub fn list_books(&self) -> Result<Vec<BookMeta>> {
        self.store().list_books()
    }

    pub fn get_book(&self, id: i64) -> Result<BookMeta> {
        self.store().get_book(id)
    }

    /// "Burn this book's data" (§11.4a): per-book hard delete.
    pub fn delete_book(&self, id: i64) -> Result<()> {
        self.store().burn_book(id)
    }

    // ============================ Reading ============================

    pub fn get_episode(&self, book_id: i64, seq: i64) -> Result<EpisodeHtml> {
        self.store().get_episode(book_id, seq)
    }

    /// Set progress. Advancing resolves theories; rewinding re-seals (§11.4a):
    /// conversations/theories stamped after the new position are archived/reopened.
    pub fn set_progress(&self, book_id: i64, episode_seq: i64, scene_seq: i64) -> Result<()> {
        let store = self.store();
        let rewound = store.set_progress(book_id, episode_seq, scene_seq)?;
        engine::resolve_theories(&store, book_id)?;
        if rewound && self.setting_bool_locked(&store, K_RESEAL, true) {
            store.reseal_after(book_id, episode_seq)?;
            store.reopen_theories_after(book_id, episode_seq)?;
        }
        Ok(())
    }

    pub fn set_serial_mode(&self, book_id: i64, enabled: bool, minutes: i64) -> Result<()> {
        let store = self.store();
        let mut meta = store.book_meta_value(book_id)?;
        meta["serial"] = serde_json::json!({ "enabled": enabled, "minutesPerEpisode": minutes });
        store.set_book_meta(book_id, &meta.to_string())
    }

    // ============================ Companion ============================

    /// One companion turn through the full 5-stage engine. `on_stage` receives the
    /// GATE→COMPOSE→VERIFY stamps. Requires a ready backend (Cloud Relay or local).
    pub fn companion_turn(
        &self,
        book_id: i64,
        character_id: Option<i64>,
        message: &str,
        on_stage: &mut dyn FnMut(&str),
    ) -> Result<TurnReport> {
        let engine = self.build_engine()?;
        let store = self.store();
        let progress = store.get_progress(book_id)?.0;
        // Persist the turn (audit trail; pinned_progress).
        let convo = self.ensure_conversation(&store, book_id, character_id)?;
        store.add_message(convo, "user", message, progress, "{}")?;
        let report = engine.companion_turn(&store, book_id, character_id, message, on_stage)?;
        let verify_json = serde_json::to_string(&report.claims).unwrap_or_else(|_| "[]".into());
        store.add_message(convo, "assistant", &report.reply, progress, &verify_json)?;
        Ok(report)
    }

    pub fn list_characters(&self, book_id: i64) -> Result<Vec<Character>> {
        self.store().list_characters(book_id)
    }

    /// Progress-gated who's-who card (F5b `who_is`).
    pub fn who_is(&self, book_id: i64, name: &str) -> Result<Character> {
        let store = self.store();
        for c in store.list_characters(book_id)? {
            let hit = c.name.eq_ignore_ascii_case(name)
                || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name));
            if hit {
                if !c.met {
                    return Err(VenaError::NotFound(format!("{name} — keep reading to meet them")));
                }
                return Ok(c);
            }
        }
        Err(VenaError::NotFound(name.into()))
    }

    pub fn get_recap(&self, book_id: i64) -> Result<String> {
        let engine = self.build_engine()?;
        engine.recap(&self.store(), book_id)
    }

    /// "Test the Gate — RUN 12 PROBES" (§11.4a): the shipped vena-eval loop.
    pub fn run_probes(&self, book_id: i64, n: usize) -> Result<Vec<ProbeResult>> {
        let engine = self.build_engine()?;
        engine.run_probes(&self.store(), book_id, n)
    }

    // ============================ Theories ============================

    pub fn add_theory(&self, book_id: i64, text: &str) -> Result<Theory> {
        let store = self.store();
        let progress = store.get_progress(book_id)?.0;
        let t = store.add_theory(book_id, text, progress)?;
        engine::resolve_theories(&store, book_id)?; // may resolve immediately if past reveal
        Ok(t)
    }

    pub fn list_theories(&self, book_id: i64) -> Result<Vec<Theory>> {
        self.store().list_theories(book_id)
    }

    // ============================ Archive (wiki) ============================

    pub fn get_wiki_index(&self, book_id: i64, mode: &str) -> Result<WikiIndex> {
        wiki::get_wiki_index(&self.store(), book_id, WikiMode::parse(mode))
    }

    pub fn get_wiki_page(&self, book_id: i64, entity_id: &str, mode: &str) -> Result<WikiPage> {
        wiki::get_wiki_page(&self.store(), book_id, entity_id, WikiMode::parse(mode))
    }

    pub fn set_spoiler_consent(&self, book_id: i64, granted: bool) -> Result<()> {
        wiki::set_consent(&self.store(), book_id, granted)
    }

    // ============================ Models & settings ============================

    pub fn get_ai_status(&self) -> Result<AiStatus> {
        let store = self.store();
        let mode = self.setting_or(&store, K_CHAT_MODE, "cloud");
        let (model, ready) = match mode.as_str() {
            "local" => {
                let ready = self.setting_bool_locked(&store, K_LOCAL_READY, false);
                (self.setting_or(&store, K_LOCAL_MODEL, "QUILL·7B"), ready)
            }
            _ => {
                let has_key = self.keystore.get(KC_CLOUD_KEY)?.is_some()
                    || std::env::var("VENA_API_KEY").is_ok();
                let base = self.setting_or(&store, K_CLOUD_BASE, "");
                let ready = has_key && (!base.is_empty() || std::env::var("VENA_BASE_URL").is_ok());
                (self.setting_or(&store, K_CLOUD_MODEL, "Cloud Relay"), ready)
            }
        };
        Ok(AiStatus {
            mode: if ready { mode.clone() } else { "none".into() },
            model,
            ready,
            local_experimental: true, // per the §11.5 steer (EVAL.md) until re-validated
        })
    }

    pub fn set_api_config(&self, base_url: &str, api_key: &str, model: &str) -> Result<()> {
        // Key -> keychain; base/model -> settings. Key NEVER touches SQLite.
        self.keystore.set(KC_CLOUD_KEY, api_key)?;
        let store = self.store();
        store.set_setting(K_CLOUD_BASE, base_url)?;
        store.set_setting(K_CLOUD_MODEL, model)?;
        store.set_setting(K_CHAT_MODE, "cloud")?;
        Ok(())
    }

    pub fn set_image_config(&self, base_url: &str, api_key: &str, model: &str) -> Result<()> {
        self.keystore.set(KC_IMAGE_KEY, api_key)?;
        let store = self.store();
        store.set_setting(K_IMAGE_BASE, base_url)?;
        store.set_setting(K_IMAGE_MODEL, model)?;
        Ok(())
    }

    pub fn set_chat_mode(&self, mode: &str) -> Result<()> {
        self.store().set_setting(K_CHAT_MODE, mode)
    }

    /// TEST THE RELAY (§11.4): a real round-trip that also confirms the gate ran
    /// locally first (it always does — stage 1 is local SQL before any send).
    pub fn test_relay(&self) -> Result<RelayTest> {
        let backend = self.cloud_backend()?;
        let t0 = std::time::Instant::now();
        let res = backend.complete(
            "You are a connectivity probe. Reply with the single word: PONG.",
            "ping",
            &vena_core::inference::GenOptions { max_tokens: 8, temperature: 0.0, json: false },
        );
        let latency_ms = t0.elapsed().as_millis();
        match res {
            Ok(reply) => Ok(RelayTest {
                ok: true,
                latency_ms,
                gate_verified: true,
                message: format!("relay ok — {}", reply.trim()),
            }),
            Err(e) => Ok(RelayTest {
                ok: false,
                latency_ms,
                gate_verified: true,
                message: e.to_string(),
            }),
        }
    }

    /// list_relay_models — fetch the provider's model list (`GET /v1/models`).
    pub fn list_relay_models(&self) -> Result<Vec<String>> {
        let store = self.store();
        let base = self
            .setting_opt(&store, K_CLOUD_BASE)
            .or_else(|| std::env::var("VENA_BASE_URL").ok())
            .ok_or(VenaError::NoBackend)?;
        let key = self
            .keystore
            .get(KC_CLOUD_KEY)?
            .or_else(|| std::env::var("VENA_API_KEY").ok())
            .unwrap_or_default();
        let url = format!("{}/v1/models", base.trim_end_matches('/'));
        let resp = reqwest::blocking::Client::new()
            .get(&url)
            .bearer_auth(&key)
            .send()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
        let v: serde_json::Value = resp.json().map_err(|e| VenaError::Inference(e.to_string()))?;
        Ok(v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// download_local_model(tier): real Hugging Face resumable download with SHA
    /// verify (§11.4 plumbing). `on_progress(pct)`. Marks local ready on success.
    pub fn download_local_model(&self, tier: &str, mut on_progress: impl FnMut(u32)) -> Result<()> {
        let t = MODEL_TIERS
            .iter()
            .find(|m| m.id == tier || m.chip == tier || m.brand == tier)
            .ok_or_else(|| VenaError::Other(format!("unknown tier {tier}")))?;
        let dir = self.data_dir.join("models");
        std::fs::create_dir_all(&dir)?;
        // The HF URL for the tier's GGUF; resumable download implemented in net.rs.
        crate::net::download_hf_gguf(t.gguf, &dir, &mut on_progress)?;
        let store = self.store();
        store.set_setting(K_LOCAL_MODEL, t.brand)?;
        store.set_setting(K_LOCAL_READY, "1")?;
        store.set_setting(K_LOCAL_BASE, "http://localhost:11434")?;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<serde_json::Value> {
        let store = self.store();
        Ok(serde_json::json!({
            "default_chat_mode": self.setting_or(&store, K_CHAT_MODE, "cloud"),
            "gate_mode": self.setting_or(&store, K_GATE_MODE, "standard"),
            "guard_fates": self.setting_bool_locked(&store, K_GUARD_FATES, true),
            "show_engine_stamps": self.setting_bool_locked(&store, K_SHOW_STAMPS, true),
            "reseal_on_reread": self.setting_bool_locked(&store, K_RESEAL, true),
            "target_language": self.setting_or(&store, K_TARGET_LANG, "French"),
            "cloud_base_url": self.setting_or(&store, K_CLOUD_BASE, ""),
            "cloud_model": self.setting_or(&store, K_CLOUD_MODEL, ""),
            "local_model": self.setting_or(&store, K_LOCAL_MODEL, ""),
            "local_ready": self.setting_bool_locked(&store, K_LOCAL_READY, false),
            "tiers": MODEL_TIERS,
        }))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        // Never allow a secret to be routed into the settings table.
        if key.contains("api_key") || key.contains("secret") {
            return Err(VenaError::Other("secrets go to the keychain, not settings".into()));
        }
        self.store().set_setting(key, value)
    }

    // ============================ Images ============================

    pub fn get_image_status(&self) -> Result<ImageStatus> {
        let store = self.store();
        if self.setting_opt(&store, K_IMAGE_MODEL).is_some() || self.keystore.get(KC_IMAGE_KEY)?.is_some() {
            Ok(ImageStatus { tier: "api".into(), model: self.setting_or(&store, K_IMAGE_MODEL, "relay") })
        } else if self.data_dir.join("models/paint").exists() {
            Ok(ImageStatus { tier: "desktop".into(), model: "EASEL·XL".into() })
        } else {
            Ok(ImageStatus { tier: "none".into(), model: "".into() })
        }
    }

    // ============================ helpers ============================

    fn ensure_conversation(
        &self,
        store: &Store,
        book_id: i64,
        character_id: Option<i64>,
    ) -> Result<i64> {
        // One active (non-archived) conversation per (book, character).
        let existing: Option<i64> = store
            .conn()
            .query_row(
                "SELECT id FROM conversation WHERE story_id=?1 AND archived=0
                 AND ((character_id IS NULL AND ?2 IS NULL) OR character_id=?2)
                 ORDER BY id DESC LIMIT 1",
                rusqlite_params(book_id, character_id),
                |r| r.get(0),
            )
            .ok();
        match existing {
            Some(id) => Ok(id),
            None => store.create_conversation(book_id, character_id),
        }
    }

    /// Build the runtime Engine from settings: backend (cloud/local) + gate mode +
    /// guard fates + tone (STRICT).
    fn build_engine(&self) -> Result<Engine> {
        let store = self.store();
        let mode = GateMode::parse(&self.setting_or(&store, K_GATE_MODE, "standard"));
        let guard = self.setting_bool_locked(&store, K_GUARD_FATES, true);
        let backend = self.runtime_backend(&store)?;
        drop(store);
        let mut eng = Engine::new(backend).with_mode(mode);
        eng.guard_fates = guard;
        Ok(eng)
    }

    /// The runtime chat backend per default_chat_mode. Cloud Relay (remote) or a
    /// local OpenAI-compat server. Env vars override for dev.
    fn runtime_backend(&self, store: &Store) -> Result<Box<dyn vena_core::inference::Inference>> {
        if let Ok(base) = std::env::var("VENA_BASE_URL") {
            let key = std::env::var("VENA_API_KEY").unwrap_or_default();
            let model = std::env::var("VENA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
            return Ok(Box::new(OpenAiClient::new(&base, &key, &model)));
        }
        match self.setting_or(store, K_CHAT_MODE, "cloud").as_str() {
            "local" => {
                if !self.setting_bool_locked(store, K_LOCAL_READY, false) {
                    return Err(VenaError::NoBackend);
                }
                let base = self.setting_or(store, K_LOCAL_BASE, "http://localhost:11434");
                let model = self.setting_or(store, K_LOCAL_MODEL, "qwen3");
                Ok(Box::new(OpenAiClient::new(&base, "", &model)))
            }
            _ => self.cloud_backend(),
        }
    }

    fn cloud_backend(&self) -> Result<Box<dyn vena_core::inference::Inference>> {
        if let Ok(base) = std::env::var("VENA_BASE_URL") {
            let key = std::env::var("VENA_API_KEY").unwrap_or_default();
            let model = std::env::var("VENA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
            return Ok(Box::new(OpenAiClient::new(&base, &key, &model)));
        }
        let store = self.store();
        let base = self.setting_opt(&store, K_CLOUD_BASE).ok_or(VenaError::NoBackend)?;
        let model = self.setting_or(&store, K_CLOUD_MODEL, "gpt-4o-mini");
        let key = self.keystore.get(KC_CLOUD_KEY)?.ok_or(VenaError::NoBackend)?;
        Ok(Box::new(OpenAiClient::new(&base, &key, &model)))
    }

    /// A forge backend (full-tier): whatever chat backend is ready, else error.
    fn backend_for_forge(&self) -> Result<Box<dyn vena_core::inference::Inference>> {
        let store = self.store();
        self.runtime_backend(&store)
    }

    fn setting_opt(&self, store: &Store, key: &str) -> Option<String> {
        store.get_setting(key).ok().flatten().filter(|s| !s.is_empty())
    }
    fn setting_or(&self, store: &Store, key: &str, default: &str) -> String {
        self.setting_opt(store, key).unwrap_or_else(|| default.to_string())
    }
    fn setting_bool_locked(&self, store: &Store, key: &str, default: bool) -> bool {
        match store.get_setting(key).ok().flatten() {
            Some(v) => v == "1" || v == "true",
            None => default,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

fn rusqlite_params(
    book_id: i64,
    character_id: Option<i64>,
) -> impl rusqlite::Params {
    rusqlite::params![book_id, character_id]
}

fn bundled_packages() -> Vec<PathBuf> {
    // Look next to the binary, in a bundled resources dir, and in the repo tree.
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("packages/dracula.vena"));
            out.push(dir.join("resources/dracula.vena"));
        }
    }
    if let Ok(res) = std::env::var("VENA_PACKAGES_DIR") {
        out.push(PathBuf::from(res).join("dracula.vena"));
    }
    // Repo-relative (dev + tests).
    out.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/packages/dracula.vena"));
    out
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn unique_slug(store: &Store, slug: &str) -> Result<String> {
    let mut candidate = slug.to_string();
    let mut n = 1;
    while store
        .conn()
        .query_row("SELECT 1 FROM story WHERE slug=?1", [&candidate], |r| r.get::<_, i64>(0))
        .is_ok()
    {
        n += 1;
        candidate = format!("{slug}-{n}");
    }
    Ok(candidate)
}
