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
    pub mode: String,  // local | cloud | none
    pub model: String, // brand name (INK·3B / QUILL·7B / … or the relay model)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Crate-visible store access for sibling modules (images.rs).
    pub(crate) fn store_guard(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store()
    }

    pub(crate) fn assets_dir(&self) -> Result<PathBuf> {
        let dir = self.data_dir.join("assets");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// (base_url, key, model) of the image endpoint, if configured (§11.4: one key
    /// covers both by default — falls back to the chat relay key).
    pub(crate) fn image_config(&self) -> Result<Option<(String, String, String)>> {
        let store = self.store();
        let base = self
            .setting_opt(&store, K_IMAGE_BASE)
            .or_else(|| self.setting_opt(&store, K_CLOUD_BASE));
        let model = self.setting_or(&store, K_IMAGE_MODEL, "");
        drop(store);
        let key = self
            .keystore
            .get(KC_IMAGE_KEY)?
            .or(self.keystore.get(KC_CLOUD_KEY)?);
        match (base, key) {
            (Some(b), Some(k)) if !model.is_empty() => Ok(Some((b, k, model))),
            _ => Ok(None),
        }
    }

    pub(crate) fn set_cover_asset(&self, book_id: i64, path: &Path) -> Result<()> {
        let store = self.store();
        store.conn_execute_set_cover(book_id, &path.to_string_lossy())
    }

    /// Seed the bundled reference books. Each seed is checked INDEPENDENTLY — an
    /// early return keyed on Dracula alone meant a profile that already had Dracula
    /// never picked up newly-added bundled comics (e.g. a fetch-nemo.sh CBZ) on
    /// later launches.
    fn seed_first_run(&self) -> Result<()> {
        let has_dracula = self
            .store()
            .list_books()?
            .iter()
            .any(|b| b.slug.starts_with("dracula"));
        if !has_dracula {
            for candidate in bundled_packages() {
                if candidate.exists() {
                    let _ = vena_core::pkg::import_vena(&self.store(), &candidate)?;
                    break;
                }
            }
        }
        // Also seed any bundled comics (.cbz) — e.g. the sample comic, or a
        // user-fetched Little Nemo (scripts/fetch-nemo.sh drops it here).
        for dir in packages_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) == Some("cbz") {
                    let stem_slug = vena_core::util::slugify(
                        path.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
                    );
                    let on_shelf = self
                        .store()
                        .list_books()?
                        .iter()
                        .any(|b| b.slug.starts_with(&stem_slug));
                    if !on_shelf {
                        let _ = self.import_book(&path.to_string_lossy(), |_, _| {});
                    }
                }
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
            vena_forge::ledger::extract_with_model(
                backend.as_ref(),
                &book.chapters,
                |seq, total| {
                    on_progress(40 + (seq as u32 * 50 / total.max(1) as u32), "extract");
                },
            )
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
        // "Deleting a book burns its ledger with it" — a true hard delete must also
        // remove the on-disk artifacts, not just DB rows: the {slug}.vena archive
        // written at import, and every cached cover/portrait asset. Otherwise the
        // full canon + ledger + art stay recoverable on disk after a "burn".
        let slug = self.store().get_book(id).ok().map(|b| b.slug);
        self.store().burn_book(id)?;
        if let Some(slug) = slug {
            let _ = std::fs::remove_file(self.data_dir.join(format!("{slug}.vena")));
        }
        if let Ok(dir) = self.assets_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let name = e.file_name();
                    let name = name.to_string_lossy();
                    // Assets are keyed by book id: cover-{id}.* and portrait-{id}-*.
                    if name.starts_with(&format!("cover-{id}."))
                        || name.starts_with(&format!("portrait-{id}-"))
                    {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
        Ok(())
    }

    // ============================ Reading ============================

    pub fn get_episode(&self, book_id: i64, seq: i64) -> Result<EpisodeHtml> {
        self.store().get_episode(book_id, seq)
    }

    /// Set progress. Advancing resolves theories; rewinding re-seals (§11.4a):
    /// conversations/theories stamped after the new position are archived/reopened.
    pub fn set_progress(&self, book_id: i64, episode_seq: i64, scene_seq: i64) -> Result<()> {
        let store = self.store();
        // Scene-granular rewind detection: earlier episode OR earlier scene in the
        // same episode both count as a rewind.
        let (old_ep, old_scene) = store.get_progress(book_id)?;
        store.set_progress(book_id, episode_seq, scene_seq)?;
        let rewound = episode_seq < old_ep || (episode_seq == old_ep && scene_seq < old_scene);
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
        // Conversation memory, chatbot-shaped: a rolling condensed note over
        // the older exchanges + the recent verbatim window, both spoiler-gated
        // by pinned progress and loaded BEFORE this message lands.
        let memory = store.latest_chat_memory(convo, progress)?;
        let history: Vec<(String, String)> = store
            .recent_messages(convo, 8, progress)?
            .into_iter()
            .map(|(role, text, _)| (role, text))
            .collect();
        // Compaction: every ~6 exchanges, re-condense everything older than
        // the window into a fresh note. Best-effort — a failed condense (no
        // engine, offline) costs nothing but staler memory.
        let n = store.count_messages(convo, progress)?;
        if n >= 20 && n % 12 == 0 {
            let older: Vec<(String, String)> = store
                .recent_messages(convo, 48, progress)?
                .into_iter()
                .rev()
                .skip(8)
                .rev()
                .map(|(role, text, _)| (role, text))
                .collect();
            if !older.is_empty() {
                if let Ok(note) = engine.condense(&older) {
                    let _ = store.add_chat_memory(convo, note.trim(), progress);
                }
            }
        }
        store.add_message(convo, "user", message, progress, "{}")?;
        let report = engine.companion_turn_with_history(
            &store,
            book_id,
            character_id,
            message,
            memory.as_deref(),
            &history,
            on_stage,
        )?;
        let verify_json = serde_json::to_string(&report.claims).unwrap_or_else(|_| "[]".into());
        store.add_message(convo, "assistant", &report.reply, progress, &verify_json)?;
        Ok(report)
    }

    /// The stored chat thread for a (book, character), oldest-first and
    /// spoiler-gated: only turns pinned at or before the CURRENT bookmark
    /// replay (a re-seal rewind hides later chat), and a re-sealed (archived)
    /// conversation never returns at all — fresh eyes stay fresh.
    pub fn get_conversation(
        &self,
        book_id: i64,
        character_id: Option<i64>,
    ) -> Result<serde_json::Value> {
        let store = self.store();
        let progress = store.get_progress(book_id)?.0;
        let turns = match store.find_active_conversation(book_id, character_id)? {
            Some(convo) => store.recent_messages(convo, 200, progress)?,
            None => Vec::new(),
        };
        let last_ch = turns.iter().map(|t| t.2).max().unwrap_or(0);
        Ok(serde_json::json!({
            "turns": turns.iter().map(|(role, text, ch)| serde_json::json!({
                "role": role, "text": text, "chapter": ch,
            })).collect::<Vec<_>>(),
            "count": turns.len(),
            "last_chapter": last_ch,
        }))
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
                    return Err(VenaError::NotFound(format!(
                        "{name} — keep reading to meet them"
                    )));
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

    /// "Test the Gate — RUN 12 PROBES" (§11.4a): the shipped vena-eval loop. On a
    /// LOCAL backend, a fully-clean run (≥ n probes, 0 leaks) promotes the tier out of
    /// "experimental" for this device (the live, device-specific eval verdict, §11.6);
    /// any leak on a local run demotes it back. Relay runs never change the flag.
    pub fn run_probes(&self, book_id: i64, n: usize) -> Result<Vec<ProbeResult>> {
        let engine = self.build_engine()?;
        let results = engine.run_probes(&self.store(), book_id, n)?;
        let is_local = self.setting_or(&self.store(), K_CHAT_MODE, "cloud") == "local";
        if is_local && results.len() >= n && n > 0 {
            let clean = results.iter().all(|r| !r.leaked);
            let _ = self.set_local_validated(clean);
        }
        Ok(results)
    }

    /// "THAT SPOILED ME" (§6): one-tap leak report. Logs the transcript LOCALLY
    /// (leak-reports.jsonl in the app data dir) for eval regression — never sent
    /// anywhere. `reason` uses the leak taxonomy; `excerpt` is the offending line.
    pub fn report_leak(
        &self,
        book_id: i64,
        reason: &str,
        excerpt: &str,
        comment: &str,
    ) -> Result<()> {
        let progress = self.store().get_progress(book_id)?.0;
        let entry = serde_json::json!({
            "book_id": book_id,
            "pinned_progress": progress,
            "reason": reason,      // leak taxonomy: future_event | unmet_character | tone_implies_ending | other
            "excerpt": excerpt,
            "comment": comment,
            "reported_at": chrono_now(),
        });
        let path = self.data_dir.join("leak-reports.jsonl");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "{entry}")?;
        Ok(())
    }

    /// Forge (or re-forge) the ledger for an already-imported book — the path by
    /// which a `raw` import becomes `sealed` once a backend exists.
    ///
    /// STREAMING: facts are inserted chapter-by-chapter, not in one batch at the end.
    /// Because the store's gate is per-fact (`chapter_seq <= progress`), the companion
    /// becomes usable for the early chapters the instant they land — a reader can chat
    /// about chapter 1 while chapter 20 is still forging. `on_progress(pct, stage,
    /// forged_through)` reports the highest chapter whose facts are now committed.
    pub fn forge_ledger(
        &self,
        book_id: i64,
        on_progress: impl FnMut(u32, &str, i64),
    ) -> Result<BookMeta> {
        // Any error past the "forging" mark rolls the state back to raw — a book must
        // never be stranded in "forging" (companion disabled) with no retry path.
        match self.forge_ledger_inner(book_id, on_progress) {
            Ok(meta) => Ok(meta),
            Err(e) => {
                let _ = self.store().set_forge_state(book_id, "raw");
                Err(e)
            }
        }
    }

    fn forge_ledger_inner(
        &self,
        book_id: i64,
        mut on_progress: impl FnMut(u32, &str, i64),
    ) -> Result<BookMeta> {
        let backend = self.backend_for_forge()?; // NoBackend when none — honest
                                                 // Idempotent: a re-forge REPLACES rather than doubles the ledger.
        {
            let store = self.store();
            store.clear_ledger(book_id)?;
            store.set_forge_state(book_id, "forging")?;
        }
        on_progress(5, "parse", 0);

        // Rebuild chapters from stored canon (episode rows are the source of truth).
        let chapters = {
            let store = self.store();
            let book = store.get_book(book_id)?;
            let mut out = Vec::new();
            for seq in 1..=book.episode_count {
                let ep = store.get_episode(book_id, seq)?;
                out.push(vena_forge::import::Chapter {
                    seq,
                    title: ep.title,
                    paragraphs: html_to_paragraphs(&ep.content_html),
                });
            }
            out
        };

        let total = chapters.len().max(1) as u32;
        let mut known: Vec<String> = Vec::new();
        for ch in &chapters {
            // Extract one chapter, then commit it immediately so the gate exposes it.
            let partial = vena_forge::ledger::extract_chapter(backend.as_ref(), ch, &mut known)
                .map_err(|e| VenaError::Inference(e.to_string()))?;
            {
                let store = self.store();
                insert_ledger_rows(&store, book_id, &partial)?;
            }
            let pct = 5 + (ch.seq as u32 * 90 / total);
            on_progress(pct.min(99), "extract", ch.seq);
        }

        self.store().set_forge_state(book_id, "sealed")?;
        on_progress(100, "done", chapters.len() as i64);
        self.store().get_book(book_id)
    }

    // ============================ Portability & sharing ============================
    //
    // Vena is zero-server, so sync and social both ride PORTABLE FILES the user moves
    // themselves (email, AirDrop, a shared Dropbox/iCloud/Syncthing folder). A bundle
    // carries ONLY the user's own layer — progress, theories, spoiler-consent — never
    // canon or the ledger (those stay on-device; keeps bundles tiny and copyright-safe)
    // and never chat text (private by default). Books are matched by slug on import, so
    // both devices must already have the book. Two scopes:
    //   "sync"     — progress + theories + consent (your own devices, last-writer-wins)
    //   "theories" — theories only (book-club sharing, leaks no reading position)

    /// Export a portable bundle. `book_id = None` exports the whole shelf.
    pub fn export_bundle(&self, book_id: Option<i64>, scope: &str) -> Result<serde_json::Value> {
        let store = self.store();
        let books: Vec<BookMeta> = match book_id {
            Some(id) => vec![store.get_book(id)?],
            None => store.list_books()?,
        };
        let include_progress = scope != "theories";
        let mut out = Vec::new();
        for b in books {
            let theories: Vec<serde_json::Value> = store
                .list_theories(b.id)?
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "text": t.text,
                        "logged_at_chapter": t.logged_at_chapter,
                    })
                })
                .collect();
            let mut entry = serde_json::json!({
                "slug": b.slug,
                "title": b.title,
                "package_sha": b.package_sha,
                "theories": theories,
            });
            if include_progress {
                let (ep, sc) = store.get_progress(b.id)?;
                entry["progress"] = serde_json::json!({
                    "episode": ep,
                    "scene": sc,
                    "updated_at": store.progress_updated_at(b.id)?.unwrap_or_default(),
                });
                entry["spoiler_consent"] =
                    serde_json::json!(vena_core::wiki::has_consent(&store, b.id)?);
            }
            out.push(entry);
        }
        Ok(serde_json::json!({
            "vena_bundle_version": 1,
            "scope": if include_progress { "sync" } else { "theories" },
            "exported_at": chrono_now(),
            "books": out,
        }))
    }

    /// Import a bundle produced by `export_bundle`. Books absent from the shelf are
    /// skipped (canon isn't in the bundle). Progress merges last-writer-wins by
    /// timestamp; theories merge as a union deduped on normalized text; a rewind that
    /// wins triggers the usual re-seal. Returns a human-readable report.
    pub fn import_bundle(&self, json: &str) -> Result<serde_json::Value> {
        let bundle: serde_json::Value =
            serde_json::from_str(json).map_err(|e| VenaError::InvalidPackage(e.to_string()))?;
        if bundle["vena_bundle_version"].as_i64() != Some(1) {
            return Err(VenaError::InvalidPackage(
                "not a Vena bundle (or unsupported version)".into(),
            ));
        }
        let mut matched = 0;
        let mut progress_updated = 0;
        let mut theories_added = 0;
        let mut skipped: Vec<String> = Vec::new();

        for entry in bundle["books"].as_array().cloned().unwrap_or_default() {
            let slug = entry["slug"].as_str().unwrap_or_default();
            // Match the book on the shelf by slug (canon must already be present).
            let local = {
                let store = self.store();
                store.list_books()?.into_iter().find(|b| b.slug == slug)
            };
            let Some(local) = local else {
                skipped.push(entry["title"].as_str().unwrap_or(slug).to_string());
                continue;
            };
            matched += 1;

            // ---- progress: last-writer-wins by updated_at ----
            if let Some(p) = entry.get("progress") {
                let incoming_at = p["updated_at"].as_str().unwrap_or_default();
                let local_at = self
                    .store()
                    .progress_updated_at(local.id)?
                    .unwrap_or_default();
                let (local_ep, _) = self.store().get_progress(local.id)?;
                // Accept the incoming position if this device hasn't read the book at
                // all (a fresh import stamps updated_at = now, which would otherwise
                // spuriously out-rank the sending device's older read timestamp), OR
                // if the incoming write is strictly newer (last-writer-wins).
                // Lexicographic compare works for SQLite datetime('now') strings.
                if !incoming_at.is_empty() && (local_ep == 0 || incoming_at > local_at.as_str()) {
                    let ep = p["episode"].as_i64().unwrap_or(0);
                    let sc = p["scene"].as_i64().unwrap_or(0);
                    self.store()
                        .set_progress_synced(local.id, ep, sc, incoming_at)?;
                    // A winning rewind re-seals, same as a manual rewind.
                    if ep < local_ep {
                        let store = self.store();
                        if self.setting_bool_locked(&store, K_RESEAL, true) {
                            store.reseal_after(local.id, ep)?;
                            store.reopen_theories_after(local.id, ep)?;
                        }
                    }
                    engine::resolve_theories(&self.store(), local.id)?;
                    progress_updated += 1;
                }
                if let Some(c) = entry.get("spoiler_consent").and_then(|v| v.as_bool()) {
                    vena_core::wiki::set_consent(&self.store(), local.id, c)?;
                }
            }

            // ---- theories: union, deduped on normalized text ----
            let existing: std::collections::HashSet<String> = self
                .store()
                .list_theories(local.id)?
                .into_iter()
                .map(|t| normalize_theory(&t.text))
                .collect();
            for t in entry["theories"].as_array().cloned().unwrap_or_default() {
                let text = t["text"].as_str().unwrap_or_default();
                if text.is_empty() || existing.contains(&normalize_theory(text)) {
                    continue;
                }
                let at = t["logged_at_chapter"].as_i64().unwrap_or(0);
                self.store().add_theory(local.id, text, at)?;
                theories_added += 1;
            }
            engine::resolve_theories(&self.store(), local.id)?;
        }

        Ok(serde_json::json!({
            "matched_books": matched,
            "progress_updated": progress_updated,
            "theories_added": theories_added,
            "skipped_not_on_shelf": skipped,
        }))
    }

    /// "Forget our conversations" (§6b): wipe chat + memory for a book, keeping the
    /// book, ledger, progress, and theories.
    pub fn forget_conversations(&self, book_id: i64) -> Result<()> {
        self.store().forget_conversations(book_id)
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
        // The eval steer (§11.5) is now DEVICE-CORRECT, not a global constant: local
        // chat is "experimental" until THIS device's tier passes the in-app gate probe
        // (Test the Gate → RUN 12 PROBES with 0 leaks), which promotes it via
        // set_local_validated. A tier that GO's on a 32 GB desktop but not a phone is
        // handled correctly because validation is keyed by (device, model).
        let local_experimental =
            !self.setting_bool_locked(&store, &local_validated_key(&model), false);
        Ok(AiStatus {
            mode: if ready { mode.clone() } else { "none".into() },
            model,
            ready,
            local_experimental,
        })
    }

    /// Promote (or demote) the local tier from "experimental" for THIS device, keyed
    /// by the current local model. Called after a clean in-app gate probe (0 leaks) —
    /// the live, device-specific version of the Phase-1 eval verdict (§11.6). Honest:
    /// only a real clean run flips it; the UI wires this to a passing RUN 12 PROBES.
    pub fn set_local_validated(&self, validated: bool) -> Result<()> {
        let store = self.store();
        let model = self.setting_or(&store, K_LOCAL_MODEL, "");
        store.set_setting(
            &local_validated_key(&model),
            if validated { "1" } else { "0" },
        )
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

    /// Curated Cloud Relay providers so onboarding is one tap: pick a provider, paste
    /// a key, done. The base URL + a sensible default model are pre-filled — the user
    /// only supplies the secret. (OpenAI-compatible endpoints; free tiers noted.)
    pub fn relay_presets(&self) -> serde_json::Value {
        serde_json::json!([
            {
                "id": "openrouter", "name": "OpenRouter",
                "base_url": "https://openrouter.ai/api/v1",
                "default_model": "meta-llama/llama-3.3-70b-instruct:free",
                "free_tier": true,
                "key_url": "https://openrouter.ai/keys",
                "note": "Free models available; one key, many providers."
            },
            {
                "id": "groq", "name": "Groq",
                "base_url": "https://api.groq.com/openai/v1",
                "default_model": "llama-3.3-70b-versatile",
                "free_tier": true,
                "key_url": "https://console.groq.com/keys",
                "note": "Very fast; generous free tier."
            },
            {
                "id": "together", "name": "Together AI",
                "base_url": "https://api.together.xyz/v1",
                "default_model": "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                "free_tier": false,
                "key_url": "https://api.together.ai/settings/api-keys",
                "note": "Broad open-model catalog."
            },
            {
                "id": "ollama", "name": "Ollama (this machine)",
                "base_url": "http://localhost:11434/v1",
                "default_model": "qwen3:8b",
                "free_tier": true,
                "key_url": "https://ollama.com/download",
                "note": "Fully local via Ollama — no key, nothing leaves the device."
            },
            {
                "id": "lmstudio", "name": "LM Studio (this machine)",
                "base_url": "http://localhost:1234/v1",
                "default_model": "local-model",
                "free_tier": true,
                "key_url": "https://lmstudio.ai",
                "note": "Fully local via LM Studio — no key needed."
            }
        ])
    }

    /// One-tap relay setup: given a preset id and (optional) key, fill in the base
    /// URL + default model, persist, and TEST it in a single round-trip. Returns the
    /// relay test so the UI can confirm success without a second call. Localhost
    /// presets (Ollama/LM Studio) need no key.
    pub fn configure_relay(&self, provider: &str, api_key: &str, model: &str) -> Result<RelayTest> {
        let presets = self.relay_presets();
        let preset = presets
            .as_array()
            .and_then(|a| a.iter().find(|p| p["id"] == provider))
            .ok_or_else(|| VenaError::Other(format!("unknown relay provider: {provider}")))?;
        let base = preset["base_url"].as_str().unwrap_or_default();
        let chosen_model = if model.trim().is_empty() {
            preset["default_model"].as_str().unwrap_or_default()
        } else {
            model.trim()
        };
        // Localhost providers may omit the key; remote ones require it.
        let is_local = base.contains("localhost") || base.contains("127.0.0.1");
        if api_key.trim().is_empty() && !is_local {
            return Err(VenaError::Other(
                "this provider needs an API key (localhost providers don't)".into(),
            ));
        }
        self.set_api_config(base, api_key.trim(), chosen_model)?;
        self.test_relay()
    }

    pub fn set_image_config(&self, base_url: &str, api_key: &str, model: &str) -> Result<()> {
        self.keystore.set(KC_IMAGE_KEY, api_key)?;
        let store = self.store();
        store.set_setting(K_IMAGE_BASE, base_url)?;
        store.set_setting(K_IMAGE_MODEL, model)?;
        Ok(())
    }

    pub fn set_chat_mode(&self, mode: &str) -> Result<()> {
        // Switching to local only sticks when a local server actually answers —
        // otherwise every turn after the switch would fail with a dead socket.
        if mode == "local" {
            let base = {
                let store = self.store();
                self.setting_or(&store, K_LOCAL_BASE, "http://localhost:11434")
            };
            if !crate::net::probe_openai_base(&base) {
                return Err(VenaError::Other(format!(
                    "no local engine at {base} — start Ollama, LM Studio or llama-server \
                     (any OpenAI-compatible server) first, then activate"
                )));
            }
        }
        self.store().set_setting(K_CHAT_MODE, mode)
    }

    /// TEST THE RELAY (§11.4): a real round-trip that also VERIFIES (not asserts)
    /// the gate runs locally: it executes stage 1–2 against a sealed book and
    /// checks that no future fact entered the assembled context before sending.
    pub fn test_relay(&self) -> Result<RelayTest> {
        let backend = self.cloud_backend()?;

        // Measured gate check — false when there is no sealed book to gate.
        let gate_verified = {
            let store = self.store();
            match store.list_books()?.into_iter().find(|b| b.fact_count > 0) {
                Some(b) => {
                    let progress = store.get_progress(b.id)?.0;
                    let gated =
                        vena_core::engine::gate_and_assemble(&store, b.id, None, "relay test")?;
                    gated.visible.iter().all(|f| f.chapter_seq <= progress)
                }
                None => false,
            }
        };

        let t0 = std::time::Instant::now();
        let res = backend.complete(
            "You are a connectivity probe. Reply with the single word: PONG.",
            "ping",
            &vena_core::inference::GenOptions {
                max_tokens: 8,
                temperature: 0.0,
                json: false,
            },
        );
        let latency_ms = t0.elapsed().as_millis();
        match res {
            Ok(reply) => Ok(RelayTest {
                ok: true,
                latency_ms,
                gate_verified,
                message: format!("relay ok — {}", reply.trim()),
            }),
            Err(e) => Ok(RelayTest {
                ok: false,
                latency_ms,
                gate_verified,
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
        // Normalize base to root (tolerate a trailing `/v1`, like OpenAiClient).
        let root = base
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/');
        let url = format!("{root}/v1/models");
        let resp = reqwest::blocking::Client::new()
            .get(&url)
            .bearer_auth(&key)
            .send()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
        let v: serde_json::Value = resp
            .json()
            .map_err(|e| VenaError::Inference(e.to_string()))?;
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
            // per-tier install state comes from the weights actually on disk,
            // so every downloaded tier (not just the last one) shows as such.
            // A file that is missing most of its bytes (killed download, manual
            // truncation) reports partial, NOT installed — resume or delete it.
            "tiers": MODEL_TIERS
                .iter()
                .map(|t| {
                    let path = self.tier_gguf_path(t);
                    let installed = weights_plausible(&path, t.size_gb);
                    serde_json::json!({
                        "id": t.id, "brand": t.brand, "chip": t.chip, "gguf": t.gguf,
                        "size_gb": t.size_gb, "min_ram_gb": t.min_ram_gb,
                        "installed": installed,
                        "partial": !installed
                            && (path.with_extension("part").exists() || path.exists()),
                    })
                })
                .collect::<Vec<_>>(),
        }))
    }

    fn tier_gguf_path(&self, t: &vena_core::model::ModelTier) -> PathBuf {
        self.data_dir
            .join("models")
            .join(format!("{}.gguf", t.gguf))
    }

    /// Stop an in-flight model download (chat or paint tier). The partial file
    /// stays on disk, so the tier shows PARTIAL and RESUME continues from it.
    pub fn cancel_model_download(&self, kind: &str, tier: &str) -> Result<serde_json::Value> {
        let path = if kind == "paint" {
            let (.., file, _) = PAINT_TIERS
                .iter()
                .find(|(id, brand, ..)| *id == tier || *brand == tier)
                .ok_or_else(|| VenaError::Other(format!("unknown paint tier {tier}")))?;
            self.data_dir.join("models/paint").join(file)
        } else {
            let t = MODEL_TIERS
                .iter()
                .find(|m| m.id == tier || m.chip == tier || m.brand == tier)
                .ok_or_else(|| VenaError::Other(format!("unknown tier {tier}")))?;
            self.tier_gguf_path(t)
        };
        crate::net::cancel_download(&path);
        Ok(serde_json::json!({ "cancelled": true }))
    }

    /// Delete a downloaded chat tier's weights (and any half-finished .part).
    /// If the deleted tier was the configured local model, local readiness and
    /// its device-validation stamp are cleared — the UI turns honest instantly.
    pub fn delete_local_model(&self, tier: &str) -> Result<serde_json::Value> {
        let t = MODEL_TIERS
            .iter()
            .find(|m| m.id == tier || m.chip == tier || m.brand == tier)
            .ok_or_else(|| VenaError::Other(format!("unknown tier {tier}")))?;
        let path = self.tier_gguf_path(t);
        let existed = path.exists();
        if existed {
            std::fs::remove_file(&path)?;
        }
        let _ = std::fs::remove_file(path.with_extension("part"));
        let store = self.store();
        if self.setting_or(&store, K_LOCAL_MODEL, "") == t.brand {
            store.set_setting(K_LOCAL_READY, "0")?;
            store.set_setting(K_LOCAL_MODEL, "")?;
            store.set_setting(&local_validated_key(t.brand), "0")?;
        }
        Ok(serde_json::json!({ "deleted": existed, "brand": t.brand }))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        // Never allow a secret to be routed into the settings table.
        let k = key.to_ascii_lowercase();
        if [
            "api_key",
            "apikey",
            "secret",
            "token",
            "password",
            "credential",
        ]
        .iter()
        .any(|w| k.contains(w))
            || k.ends_with("_key")
        {
            return Err(VenaError::Other(
                "secrets go to the keychain, not settings".into(),
            ));
        }
        self.store().set_setting(key, value)
    }

    // ============================ Images ============================

    pub fn get_image_status(&self) -> Result<ImageStatus> {
        let store = self.store();
        if self.setting_opt(&store, K_IMAGE_MODEL).is_some()
            || self.keystore.get(KC_IMAGE_KEY)?.is_some()
        {
            Ok(ImageStatus {
                tier: "api".into(),
                model: self.setting_or(&store, K_IMAGE_MODEL, "relay"),
            })
        } else if self.data_dir.join("models/paint").exists() {
            Ok(ImageStatus {
                tier: "desktop".into(),
                model: "EASEL·XL".into(),
            })
        } else {
            Ok(ImageStatus {
                tier: "none".into(),
                model: "".into(),
            })
        }
    }

    // ============================ Store (§F4) ============================

    /// Merged search across all sources, origin-tagged. Local vena-catalog (bundled
    /// prebuilt packages) + Project Gutenberg (Gutendex). OPDS/AO3 add in browse.
    pub fn store_search(&self, query: &str) -> Result<Vec<StoreItem>> {
        let on_shelf = self.on_shelf_check()?;
        let mut items = Vec::new();

        // vena-catalog: EVERY bundled pre-forged package features, described by
        // its own metadata (pkg::peek_vena) — drop a new .vena into a packages
        // dir and it appears here, nothing hardcoded.
        let mut seen = std::collections::HashSet::new();
        for dir in packages_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("vena") {
                    continue;
                }
                let Ok((slug, title, author, license)) = vena_core::pkg::peek_vena(&p) else {
                    continue;
                };
                if !seen.insert(slug.clone()) {
                    continue;
                }
                if !query.is_empty() && !title.to_lowercase().contains(&query.to_lowercase()) {
                    continue;
                }
                items.push(StoreItem {
                    source: "vena-catalog".into(),
                    on_shelf: on_shelf(&title) || on_shelf(&slug),
                    id: slug,
                    title,
                    author,
                    license: Some(license),
                    download_url: Some(p.to_string_lossy().into()),
                    cover: None,
                });
            }
        }
        items.sort_by(|a, b| a.title.cmp(&b.title));

        // Project Gutenberg (real Gutendex; may be offline-blocked — that's honest).
        if !query.is_empty() {
            if let Ok(results) = crate::net::gutendex_search(query, None, 1) {
                for (id, title, author, epub, cover) in results.into_iter().take(20) {
                    items.push(StoreItem {
                        source: "gutenberg".into(),
                        on_shelf: on_shelf(&title),
                        id,
                        title,
                        author,
                        license: Some("public-domain".into()),
                        download_url: epub,
                        cover,
                    });
                }
            }
        }
        Ok(items)
    }

    /// Whether a store title is already on the shelf (title or slug identity).
    fn on_shelf_check(&self) -> Result<impl Fn(&str) -> bool> {
        let books = self.store().list_books()?;
        let titles: std::collections::HashSet<String> =
            books.iter().map(|b| b.title.to_lowercase()).collect();
        let slugs: std::collections::HashSet<String> =
            books.iter().map(|b| b.slug.clone()).collect();
        Ok(move |title: &str| {
            titles.contains(&title.to_lowercase())
                || slugs.contains(&vena_core::util::slugify(title))
        })
    }

    pub fn store_browse(&self, source: &str, cursor: Option<&str>) -> Result<Vec<StoreItem>> {
        let on_shelf = self.on_shelf_check()?;
        match source {
            "gutenberg" => {
                // cursor forms: None (popular p.1), "2" (page), "mystery@1" (topic@page).
                let (topic, page) = match cursor {
                    Some(c) if c.contains('@') => {
                        let (t, p) = c.split_once('@').unwrap();
                        (Some(t.to_string()), p.parse().unwrap_or(1))
                    }
                    Some(c) => (None, c.parse().unwrap_or(1)),
                    None => (None, 1),
                };
                let results = crate::net::gutendex_search("", topic.as_deref(), page)?;
                Ok(results
                    .into_iter()
                    .map(|(id, title, author, epub, cover)| StoreItem {
                        source: "gutenberg".into(),
                        on_shelf: on_shelf(&title),
                        id,
                        title,
                        author,
                        license: Some("public-domain".into()),
                        download_url: epub,
                        cover,
                    })
                    .collect())
            }
            _ => {
                // OPDS catalog by id (Standard Ebooks or user-added).
                let url = self
                    .opds_url_for(source)
                    .unwrap_or_else(|| source.to_string());
                let entries = crate::net::opds_fetch(&url, &self.user_hosts())?;
                Ok(entries
                    .into_iter()
                    .map(|(id, title, author, acquire)| StoreItem {
                        source: "opds".into(),
                        on_shelf: on_shelf(&title),
                        id,
                        title,
                        author,
                        license: None,
                        download_url: acquire,
                        cover: None,
                    })
                    .collect())
            }
        }
    }

    /// Download a store item and forge it. `on_progress(pct, phase)` where phase is
    /// "download" then "forge". Returns the new BookMeta.
    pub fn store_download(
        &self,
        item: &StoreItem,
        mut on_progress: impl FnMut(u32, &str),
    ) -> Result<BookMeta> {
        let url = item
            .download_url
            .as_deref()
            .ok_or_else(|| VenaError::Other("no download url".into()))?;
        // A local vena-catalog package: import directly.
        if item.source == "vena-catalog" && Path::new(url).exists() {
            on_progress(50, "download");
            let sid = vena_core::pkg::import_vena(&self.store(), Path::new(url))?;
            on_progress(100, "forge");
            return self.store().get_book(sid);
        }
        // Otherwise download the EPUB then import+forge.
        let tmp = self
            .data_dir
            .join(format!("dl-{}.epub", slugify(&item.title)));
        crate::net::download_file(url, &tmp, &mut |p| on_progress(p * 60 / 100, "download"))?;
        let meta = self.import_book(&tmp.to_string_lossy(), |p, _| {
            on_progress(60 + p * 40 / 100, "forge")
        })?;
        let _ = std::fs::remove_file(&tmp);
        Ok(meta)
    }

    pub fn add_opds_catalog(&self, url: &str, name: &str) -> Result<String> {
        let store = self.store();
        let mut list = self.opds_catalogs(&store);
        let id = format!("opds-{}", list.len() + 1);
        list.push(serde_json::json!({ "id": id, "name": name, "url": url }));
        store.set_setting("opds_catalogs", &serde_json::Value::Array(list).to_string())?;
        Ok(id)
    }

    pub fn remove_opds_catalog(&self, id: &str) -> Result<()> {
        let store = self.store();
        let list: Vec<serde_json::Value> = self
            .opds_catalogs(&store)
            .into_iter()
            .filter(|c| c["id"].as_str() != Some(id))
            .collect();
        store.set_setting("opds_catalogs", &serde_json::Value::Array(list).to_string())
    }

    pub fn list_opds_catalogs(&self) -> Result<serde_json::Value> {
        Ok(serde_json::Value::Array(self.opds_catalogs(&self.store())))
    }

    /// import_ao3_link — fetch the EPUB AO3 officially serves, then import+forge.
    pub fn import_ao3_link(
        &self,
        url: &str,
        mut on_progress: impl FnMut(u32, &str),
    ) -> Result<BookMeta> {
        let epub = crate::net::ao3_epub_url(url)?;
        let tmp = self.data_dir.join("dl-ao3.epub");
        crate::net::download_file(&epub, &tmp, &mut |p| on_progress(p * 60 / 100, "download"))?;
        let meta = self.import_book(&tmp.to_string_lossy(), |p, _| {
            on_progress(60 + p * 40 / 100, "forge")
        })?;
        let _ = std::fs::remove_file(&tmp);
        Ok(meta)
    }

    fn opds_catalogs(&self, store: &Store) -> Vec<serde_json::Value> {
        store
            .get_setting("opds_catalogs")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
            .unwrap_or_else(|| {
                // Standard Ebooks OPDS ships as a known-good default source.
                vec![serde_json::json!({
                    "id": "standard-ebooks",
                    "name": "Standard Ebooks",
                    "url": "https://standardebooks.org/feeds/opds/all"
                })]
            })
    }

    /// Hosts of the user's registered catalogs + configured relay endpoints — the
    /// user-initiated additions to the fixed network allowlist (§11.2).
    pub(crate) fn user_hosts(&self) -> Vec<String> {
        let store = self.store();
        let mut hosts: Vec<String> = self
            .opds_catalogs(&store)
            .into_iter()
            .filter_map(|c| c["url"].as_str().map(crate::net::host_of))
            .collect();
        for key in [K_CLOUD_BASE, K_LOCAL_BASE, K_IMAGE_BASE] {
            if let Some(base) = self.setting_opt(&store, key) {
                hosts.push(crate::net::host_of(&base));
            }
        }
        hosts
    }

    fn opds_url_for(&self, id: &str) -> Option<String> {
        self.opds_catalogs(&self.store())
            .into_iter()
            .find(|c| c["id"].as_str() == Some(id))
            .and_then(|c| c["url"].as_str().map(str::to_string))
    }

    // ============================ helpers ============================

    fn ensure_conversation(
        &self,
        store: &Store,
        book_id: i64,
        character_id: Option<i64>,
    ) -> Result<i64> {
        // One active (non-archived) conversation per (book, character).
        match store.find_active_conversation(book_id, character_id)? {
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
                // Pre-flight instead of a raw connection error mid-turn. NO silent
                // cloud fallback: the reader chose local — content stays on-device.
                if !crate::net::probe_openai_base(&base) {
                    return Err(VenaError::Other(format!(
                        "local engine offline — nothing is answering at {base}. Start your \
                         local model server (Ollama, LM Studio, or llama-server with the \
                         downloaded GGUF), or switch chat to Cloud Relay in Settings"
                    )));
                }
                let model = self.setting_or(store, K_LOCAL_MODEL, "qwen3");
                Ok(Box::new(OpenAiClient::new(&base, "", &model)))
            }
            _ => self.cloud_backend_with(store),
        }
    }

    /// Cloud Relay backend. Takes the ALREADY-HELD store guard — never re-locks
    /// (the profile Mutex is not reentrant; re-locking here would deadlock).
    fn cloud_backend_with(
        &self,
        store: &Store,
    ) -> Result<Box<dyn vena_core::inference::Inference>> {
        if let Ok(base) = std::env::var("VENA_BASE_URL") {
            let key = std::env::var("VENA_API_KEY").unwrap_or_default();
            let model = std::env::var("VENA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
            return Ok(Box::new(OpenAiClient::new(&base, &key, &model)));
        }
        let base = self
            .setting_opt(store, K_CLOUD_BASE)
            .ok_or(VenaError::NoBackend)?;
        let model = self.setting_or(store, K_CLOUD_MODEL, "gpt-4o-mini");
        let key = self
            .keystore
            .get(KC_CLOUD_KEY)?
            .ok_or(VenaError::NoBackend)?;
        Ok(Box::new(OpenAiClient::new(&base, &key, &model)))
    }

    fn cloud_backend(&self) -> Result<Box<dyn vena_core::inference::Inference>> {
        let store = self.store();
        self.cloud_backend_with(&store)
    }

    /// A forge backend (full-tier): whatever chat backend is ready, else error.
    fn backend_for_forge(&self) -> Result<Box<dyn vena_core::inference::Inference>> {
        let store = self.store();
        self.runtime_backend(&store)
    }

    fn setting_opt(&self, store: &Store, key: &str) -> Option<String> {
        store
            .get_setting(key)
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
    }
    fn setting_or(&self, store: &Store, key: &str, default: &str) -> String {
        self.setting_opt(store, key)
            .unwrap_or_else(|| default.to_string())
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

/// Normalize theory text for dedup on merge (lowercase, collapse whitespace, drop
/// trailing punctuation) so "The Count is a vampire." == "the count is a vampire".
fn normalize_theory(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '!', '?', ' '])
        .to_string()
}

/// Settings key for "this device's local tier passed the gate", keyed by model so
/// each tier is validated independently.
/// A weights file counts as installed only when it holds most of its expected
/// bytes — a killed download or truncated file must not masquerade as a model.
fn weights_plausible(path: &Path, size_gb: f32) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() as f64 >= f64::from(size_gb) * 0.5 * 1_000_000_000.0)
        .unwrap_or(false)
}

fn local_validated_key(model: &str) -> String {
    format!("local_validated::{}", model.to_lowercase())
}

fn chrono_now() -> String {
    // ISO-ish UTC timestamp without a chrono dependency.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

fn html_to_paragraphs(html: &str) -> Vec<String> {
    vena_forge::import::html_to_paragraphs(html)
}

/// Insert a freshly-extracted ledger into an EXISTING story (forge_ledger path).
/// Mirrors forge_to_db's name→id resolution, scoped to the story's ids.
fn insert_ledger_rows(
    store: &Store,
    story_id: i64,
    ledger: &vena_forge::ledger::Ledger,
) -> Result<()> {
    use std::collections::HashMap;
    let mut char_id_by_name: HashMap<String, i64> = HashMap::new();
    for c in store.list_characters(story_id)? {
        char_id_by_name.insert(c.name.to_lowercase(), c.id);
        for a in &c.aliases {
            char_id_by_name.entry(a.to_lowercase()).or_insert(c.id);
        }
    }
    for c in &ledger.characters {
        if !char_id_by_name.contains_key(&c.name.to_lowercase()) {
            let id = store.insert_character(
                story_id,
                &c.name,
                &c.aliases,
                &c.voice,
                c.first_appearance_chapter,
            )?;
            char_id_by_name.insert(c.name.to_lowercase(), id);
            for a in &c.aliases {
                char_id_by_name.entry(a.to_lowercase()).or_insert(id);
            }
        }
    }
    for f in &ledger.facts {
        let subject_char_id = f
            .subject
            .as_deref()
            .and_then(|s| char_id_by_name.get(&s.to_lowercase()).copied());
        let known_by: Vec<vena_core::model::KnownBy> = f
            .known_by
            .iter()
            .filter_map(|(name, learned)| {
                char_id_by_name
                    .get(&name.to_lowercase())
                    .map(|&cid| vena_core::model::KnownBy {
                        character_id: cid,
                        learned_at_chapter: *learned,
                    })
            })
            .collect();
        let fact_id = store.insert_fact(&vena_core::model::Fact {
            id: 0,
            story_id,
            chapter_seq: f.chapter,
            subject_char_id,
            kind: f.kind,
            text: f.text.clone(),
            known_by: known_by.clone(),
            spoiler_weight: f.spoiler_weight.clamp(0, 3),
        })?;
        // Derive chapter-stamped edges from relationship facts, citing the source.
        if matches!(f.kind, vena_core::model::FactKind::Relationship) {
            if let Some(subject_id) = subject_char_id {
                for kb in &known_by {
                    if kb.character_id != subject_id {
                        store.add_edge(
                            story_id,
                            &format!("char:{subject_id}"),
                            &format!("char:{}", kb.character_id),
                            "knows",
                            f.chapter,
                            None,
                            Some(fact_id),
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Every directory that can hold bundled packages (deduped).
fn packages_dirs() -> std::collections::BTreeSet<PathBuf> {
    bundled_packages()
        .iter()
        .filter_map(|p| p.parent().map(std::path::Path::to_path_buf))
        .collect()
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

use vena_core::util::{slugify, unique_slug};

// ============================ F5c: dictionary & translate ============================

impl AppApi {
    /// lookup_word(term, lang) — §F5c. Sources in priority order: user-imported
    /// StarDict-style JSON packs in <data>/dict/*.json ({word: definition}),
    /// then the AI fallback (stamped "ai") when a backend is ready.
    pub fn lookup_word(&self, term: &str, _lang: &str) -> Result<serde_json::Value> {
        let t = term.trim().to_lowercase();
        if t.is_empty() {
            return Err(VenaError::Other("empty term".into()));
        }
        // 1) user dictionary packs (bring-your-own, imported as JSON maps)
        let dict_dir = self.data_dir.join("dict");
        if let Ok(entries) = std::fs::read_dir(&dict_dir) {
            for e in entries.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Ok(map) = serde_json::from_str::<serde_json::Value>(
                        &std::fs::read_to_string(e.path()).unwrap_or_default(),
                    ) {
                        if let Some(def) = map.get(&t).and_then(|v| v.as_str()) {
                            return Ok(serde_json::json!({"source":"stardict","entry":def}));
                        }
                    }
                }
            }
        }
        // 2) AI fallback — stamped "ai" (the ✦ AI badge in the design).
        let store = self.store();
        let backend = self.runtime_backend(&store)?;
        drop(store);
        let entry = backend.complete(
            "You are a concise offline dictionary. Define the word in <=25 words. No preamble.",
            &t,
            &vena_core::inference::GenOptions {
                max_tokens: 60,
                temperature: 0.2,
                json: false,
            },
        )?;
        Ok(serde_json::json!({"source":"ai","entry":entry.trim()}))
    }

    /// translate_selection(bookId, text, targetLang) — §F5c. INVARIANTS: the
    /// translation is an overlay (canon untouched by construction), and only text
    /// at or before the reader's bookmark may be translated — enforced by checking
    /// the selection actually occurs in an episode ≤ progress.
    pub fn translate_selection(
        &self,
        book_id: i64,
        text: &str,
        target_lang: &str,
    ) -> Result<String> {
        let needle = text.trim();
        if needle.len() < 2 {
            return Err(VenaError::Other("selection too short".into()));
        }
        {
            let store = self.store();
            // No .max(1): at progress 0 nothing has been read, so nothing may be
            // translated — the ≤-bookmark invariant holds at the boundary too.
            let progress = store.get_progress(book_id)?.0;
            let book = store.get_book(book_id)?;
            let mut found = false;
            let probe: String = needle.chars().take(80).collect();
            for seq in 1..=progress.min(book.episode_count) {
                if store
                    .get_episode(book_id, seq)?
                    .content_html
                    .contains(&probe)
                {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(VenaError::Other(
                    "only text at or before your bookmark can be translated".into(),
                ));
            }
        }
        let store = self.store();
        let backend = self.runtime_backend(&store)?;
        drop(store);
        backend.complete(
            &format!(
                "Translate the passage into {target_lang}. Output ONLY the translation, \
                 faithful in register and era."
            ),
            needle,
            &vena_core::inference::GenOptions {
                max_tokens: 800,
                temperature: 0.3,
                json: false,
            },
        )
    }
}

// ============================ Paint Engine (local image models) ============================

/// Local paint tiers (§11.4 images): stable-diffusion.cpp GGUF weights, downloaded
/// like the voice tiers — no API key needed. Rendering uses the sd.cpp `sd` CLI when
/// present; with weights but no engine the status says so honestly.
pub const PAINT_TIERS: &[(&str, &str, &str, &str, f32)] = &[
    // (id, brand, hf_repo, hf_file, size_gb)
    (
        "sketch",
        "SKETCH·1.5",
        "second-state/stable-diffusion-v1-5-GGUF",
        "stable-diffusion-v1-5-pruned-emaonly-Q8_0.gguf",
        2.0,
    ),
    (
        "easel",
        "EASEL·XL",
        "gpustack/stable-diffusion-xl-base-1.0-GGUF",
        "stable-diffusion-xl-base-1.0-Q8_0.gguf",
        4.3,
    ),
];

impl AppApi {
    /// The paint tier catalog for the Settings panel.
    pub fn paint_tiers(&self) -> serde_json::Value {
        let dir = self.data_dir.join("models/paint");
        serde_json::json!(PAINT_TIERS
            .iter()
            .map(|(id, brand, _repo, file, size)| {
                let path = dir.join(file);
                let installed = weights_plausible(&path, *size);
                serde_json::json!({
                    "id": id, "brand": brand, "size_gb": size,
                    "installed": installed,
                    "partial": !installed
                        && (path.with_extension("part").exists() || path.exists()),
                })
            })
            .collect::<Vec<_>>())
    }

    /// Download a paint tier's GGUF (resumable, SHA-verified from the HF LFS pointer,
    /// same plumbing as the voice tiers) into models/paint/. The in-repo filename is
    /// resolved from the live repo listing — hardcoded names 401 when a repo renames
    /// its quantizations — while the on-disk name stays the tier's fixed one so
    /// installed/delete detection keeps working.
    pub fn download_paint_model(
        &self,
        tier: &str,
        mut on_progress: impl FnMut(u32),
    ) -> Result<serde_json::Value> {
        let (_, brand, repo, file, _) = PAINT_TIERS
            .iter()
            .find(|(id, brand, ..)| *id == tier || *brand == tier)
            .ok_or_else(|| VenaError::Other(format!("unknown paint tier {tier}")))?;
        let dir = self.data_dir.join("models/paint");
        std::fs::create_dir_all(&dir)?;
        let remote = crate::net::hf_pick_gguf(repo, file);
        crate::net::hf_download(repo, &remote, &dir.join(file), &mut on_progress)?;
        Ok(serde_json::json!({ "brand": brand, "engine_present": sd_cli_present() }))
    }

    /// Delete a paint tier's weights (and any half-finished .part). The paint
    /// dir is dropped when empty so get_image_status stops reporting 'desktop'.
    pub fn delete_paint_model(&self, tier: &str) -> Result<serde_json::Value> {
        let (_, brand, _, file, _) = PAINT_TIERS
            .iter()
            .find(|(id, brand, ..)| *id == tier || *brand == tier)
            .ok_or_else(|| VenaError::Other(format!("unknown paint tier {tier}")))?;
        let dir = self.data_dir.join("models/paint");
        let path = dir.join(file);
        let existed = path.exists();
        if existed {
            std::fs::remove_file(&path)?;
        }
        let _ = std::fs::remove_file(path.with_extension("part"));
        let _ = std::fs::remove_dir(&dir); // only succeeds when empty
        Ok(serde_json::json!({ "deleted": existed, "brand": brand }))
    }

    fn installed_paint_model(&self) -> Option<String> {
        let dir = self.data_dir.join("models/paint");
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.ends_with(".gguf").then_some(n)
        })
    }

    /// The installed paint weights + whether the sd.cpp engine binary is available —
    /// used by images.rs tier 2 and reported by get_image_status.
    pub(crate) fn local_paint(&self) -> Option<(PathBuf, bool)> {
        let file = self.installed_paint_model()?;
        Some((
            self.data_dir.join("models/paint").join(file),
            sd_cli_present(),
        ))
    }

    // ============================ Comics (F5c reading) ============================

    /// Number of real page images available for a comic book (0 for prose).
    pub fn get_manga_pages(&self, book_id: i64) -> Result<serde_json::Value> {
        let book = self.store().get_book(book_id)?;
        let dir = self.assets_dir()?.join("manga").join(&book.slug);
        let count = std::fs::read_dir(&dir)
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        Ok(serde_json::json!({ "count": count, "profile": book.profile }))
    }

    /// One page image, base64 (lazy — the UI builds a data: URI per page on demand).
    pub fn get_manga_page(&self, book_id: i64, page: i64) -> Result<serde_json::Value> {
        let book = self.store().get_book(book_id)?;
        let dir = self.assets_dir()?.join("manga").join(&book.slug);
        let entry = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .nth((page.max(1) - 1) as usize)
            .ok_or_else(|| VenaError::NotFound(format!("page {page}")))?;
        let bytes = std::fs::read(&entry)?;
        let mime = match entry.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            _ => "image/jpeg",
        };
        Ok(serde_json::json!({ "mime": mime, "data": base64_encode(&bytes) }))
    }

    /// Browser-upload import: the webview can't hand us a filesystem path, so the UI
    /// posts the file's bytes; we persist them under imports/ and run the SAME import
    /// pipeline (Tauri uses the native dialog + import_book instead).
    pub fn import_book_data(
        &self,
        name: &str,
        data_b64: &str,
        on_progress: impl FnMut(u32, &str),
    ) -> Result<BookMeta> {
        let safe = Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.contains(".."))
            .ok_or_else(|| VenaError::Other("bad file name".into()))?;
        let dir = self.data_dir.join("imports");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(safe);
        std::fs::write(&path, crate::images::base64_decode(data_b64)?)?;
        self.import_book(&path.to_string_lossy(), on_progress)
    }
}

fn sd_cli_present() -> bool {
    // stable-diffusion.cpp ships the `sd` CLI; PATH probe keeps this dependency-free.
    std::process::Command::new("sd")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success() || s.code().is_some())
        .unwrap_or(false)
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
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
}
