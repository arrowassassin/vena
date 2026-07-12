//! Forge orchestrator: imported book + ledger → a populated single-story `.vena`
//! database. Runs scene segmentation, resolves ledger names to ids, derives the
//! story graph, computes a ledger-coverage self-audit score and a content SHA.

use crate::import::{Chapter, ImportedBook};
use crate::ledger::Ledger;
use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use vena_core::model::{Fact, KnownBy};
use vena_core::store::Store;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ForgeStats {
    pub story_id: i64,
    pub chapters: i64,
    pub scenes: i64,
    pub characters: i64,
    pub entities: i64,
    pub facts: i64,
    pub edges: i64,
    pub ledger_coverage: f32,
    pub content_sha: String,
    pub profile: String,
}

/// Segment a chapter into ~scene units (paragraph groups) with short summaries.
fn segment_scenes(ch: &Chapter) -> Vec<String> {
    const PARAS_PER_SCENE: usize = 5;
    ch.paragraphs
        .chunks(PARAS_PER_SCENE)
        .map(|chunk| {
            let first = chunk.first().map(String::as_str).unwrap_or("");
            let mut s: String = first.chars().take(160).collect();
            if first.chars().count() > 160 {
                s.push('…');
            }
            s
        })
        .collect()
}

/// Write the full package into a fresh SQLite db at `db_path`.
#[allow(clippy::too_many_arguments)]
pub fn forge_to_db(
    book: &ImportedBook,
    ledger: &Ledger,
    slug: &str,
    license: &str,
    source: Option<&str>,
    cover_asset: Option<&str>,
    db_path: &Path,
) -> Result<ForgeStats> {
    if db_path.exists() {
        std::fs::remove_file(db_path)?;
    }
    let store = Store::open(db_path)?;

    let meta = serde_json::json!({
        "format_version": 1,
        "profile": book.profile,
        "profile_evidence": book.profile_evidence,
        "forge_state": "sealed",
    });
    let sid = store.insert_story(
        slug,
        &book.title,
        book.author.as_deref(),
        license,
        source,
        cover_asset,
        &meta.to_string(),
    )?;

    // Episodes + scenes (canon, immutable).
    let mut scene_total = 0;
    for ch in &book.chapters {
        let ep = store.insert_episode(
            sid,
            ch.seq,
            ch.title.as_deref(),
            Some(ch.est_minutes()),
            &ch.content_html(),
        )?;
        for (i, summary) in segment_scenes(ch).into_iter().enumerate() {
            store.insert_scene(ep, i as i64 + 1, &summary)?;
            scene_total += 1;
        }
    }
    let n_chapters = book.chapters.len() as i64;

    // Characters (name/alias -> "char:id").
    let mut key_of: HashMap<String, String> = HashMap::new();
    let mut char_count = 0;
    for c in &ledger.characters {
        let id = store.insert_character(
            sid,
            &c.name,
            &c.aliases,
            &c.voice,
            c.first_appearance_chapter.clamp(1, n_chapters.max(1)),
        )?;
        let key = format!("char:{id}");
        key_of.insert(norm(&c.name), key.clone());
        for a in &c.aliases {
            key_of.entry(norm(a)).or_insert_with(|| key.clone());
        }
        char_count += 1;
    }

    // Entities (name/alias -> "entity:id").
    let mut entity_count = 0;
    for e in &ledger.entities {
        let id = store.add_entity(
            sid,
            &e.kind,
            &e.name,
            &e.aliases,
            e.first_appearance_chapter.clamp(1, n_chapters.max(1)),
        )?;
        let key = format!("entity:{id}");
        key_of.entry(norm(&e.name)).or_insert_with(|| key.clone());
        for a in &e.aliases {
            key_of.entry(norm(a)).or_insert_with(|| key.clone());
        }
        entity_count += 1;
    }

    // Facts (resolve subject + known_by names to ids). Facts beyond the last
    // chapter are dropped (a curated ledger must not out-run the canon).
    let char_id_of = |name: &str| -> Option<i64> {
        key_of
            .get(&norm(name))
            .and_then(|k| k.strip_prefix("char:"))
            .and_then(|s| s.parse::<i64>().ok())
    };
    let mut fact_count = 0;
    // Collect derived-edge candidates from relationship facts so each cites its
    // source fact id (v2.0 §6b: "every edge cites its source fact").
    let mut derived: Vec<(String, String, i64, i64)> = Vec::new(); // from_key,to_key,since,source_fact_id
    for f in &ledger.facts {
        if f.chapter < 1 || f.chapter > n_chapters {
            continue;
        }
        let subject_char_id = f.subject.as_deref().and_then(char_id_of);
        let known_by: Vec<KnownBy> = f
            .known_by
            .iter()
            .filter_map(|(name, learned)| {
                char_id_of(name).map(|cid| KnownBy {
                    character_id: cid,
                    learned_at_chapter: *learned,
                })
            })
            .collect();
        let fact_id = store.insert_fact(&Fact {
            id: 0,
            story_id: sid,
            chapter_seq: f.chapter,
            subject_char_id,
            kind: f.kind,
            text: f.text.clone(),
            known_by,
            spoiler_weight: f.spoiler_weight.clamp(0, 3),
        })?;
        fact_count += 1;

        if matches!(f.kind, vena_core::model::FactKind::Relationship) {
            if let Some(subject) = f.subject.as_deref().and_then(|s| key_of.get(&norm(s))) {
                for (participant, _) in &f.known_by {
                    if let Some(pkey) = key_of.get(&norm(participant)) {
                        if pkey != subject {
                            derived.push((subject.clone(), pkey.clone(), f.chapter, fact_id));
                        }
                    }
                }
            }
        }
    }

    // Edges: explicit (authored, no single source fact) + derived (cite source).
    // Dedup by (from,to,rel_type,since).
    let mut seen: std::collections::HashSet<(String, String, String, i64)> = Default::default();
    let mut edge_count = 0;
    for e in &ledger.edges {
        let (Some(from), Some(to)) = (key_of.get(&norm(&e.from)), key_of.get(&norm(&e.to))) else {
            continue;
        };
        if seen.insert((
            from.clone(),
            to.clone(),
            e.rel_type.clone(),
            e.since_chapter,
        )) {
            store.add_edge(
                sid,
                from,
                to,
                &e.rel_type,
                e.since_chapter,
                e.until_chapter,
                None,
            )?;
            edge_count += 1;
        }
    }
    for (from, to, since, source_fact_id) in derived {
        if seen.insert((from.clone(), to.clone(), "knows".into(), since)) {
            store.add_edge(sid, &from, &to, "knows", since, None, Some(source_fact_id))?;
            edge_count += 1;
        }
    }

    // §11.3: a package ships with user tables EMPTY. insert_story seeds a default
    // progress row; clear it so the .vena carries no user state.
    store
        .conn()
        .execute("DELETE FROM progress WHERE story_id=?1", [sid])?;

    // Self-audit: ledger-coverage score (twist coverage across chapters).
    let coverage = coverage_score(&store, sid, n_chapters)?;
    let content_sha = content_sha(&store, sid)?;

    let mut meta = store.book_meta_value(sid)?;
    meta["ledger_coverage"] = serde_json::json!(coverage);
    meta["package_sha"] = serde_json::json!(content_sha);
    store.set_book_meta(sid, &meta.to_string())?;

    Ok(ForgeStats {
        story_id: sid,
        chapters: n_chapters,
        scenes: scene_total,
        characters: char_count,
        entities: entity_count,
        facts: fact_count,
        edges: edge_count,
        ledger_coverage: coverage,
        content_sha,
        profile: book.profile.clone(),
    })
}

/// Coverage = half "every chapter has some fact" + half "every chapter with a
/// major beat has a weight≥2 fact". Honest, in [0,1]; shown as "COVERAGE NN%".
fn coverage_score(store: &Store, sid: i64, n_chapters: i64) -> Result<f32> {
    if n_chapters == 0 {
        return Ok(0.0);
    }
    let facts = store.facts_at_or_before(sid, i64::MAX)?;
    let mut has_fact = std::collections::HashSet::new();
    let mut has_major = std::collections::HashSet::new();
    for f in &facts {
        has_fact.insert(f.chapter_seq);
        if f.spoiler_weight >= 2 {
            has_major.insert(f.chapter_seq);
        }
    }
    let a = has_fact.len() as f32 / n_chapters as f32;
    let b = has_major.len() as f32 / n_chapters as f32;
    Ok(((a * 0.5) + (b * 0.5)).clamp(0.0, 1.0))
}

/// Stable content identity: SHA-256 over title + canon HTML + sorted fact texts.
/// Independent of meta so it can be stored back into meta without self-reference.
fn content_sha(store: &Store, sid: i64) -> Result<String> {
    let book = store.get_book(sid)?;
    let mut hasher = Sha256::new();
    hasher.update(book.title.as_bytes());
    for seq in 1..=book.episode_count {
        if let Ok(ep) = store.get_episode(sid, seq) {
            hasher.update(ep.content_html.as_bytes());
        }
    }
    let mut fact_texts: Vec<String> = store
        .facts_at_or_before(sid, i64::MAX)?
        .into_iter()
        .map(|f| format!("{}:{}", f.chapter_seq, f.text))
        .collect();
    fact_texts.sort();
    for t in fact_texts {
        hasher.update(t.as_bytes());
    }
    let full = hasher.finalize();
    Ok(full.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

pub fn require_nonempty(book: &ImportedBook) -> Result<()> {
    if book.chapters.is_empty() {
        return Err(anyhow!("import produced no chapters"));
    }
    Ok(())
}
