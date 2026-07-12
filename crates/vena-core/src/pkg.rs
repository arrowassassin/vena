//! The `.vena` package format (§11.3): a SQLite database (story/episode/scene/
//! character/fact/entity/edge populated; user tables empty) zipped with any cover
//! asset. Import = schema-validate → copy rows into the profile db under a fresh
//! story_id, remapping every foreign key. `format_version` = 1.

use crate::error::{Result, VenaError};
use crate::model::*;
use crate::store::Store;
use rusqlite::{params, Connection, OptionalExtension};
use std::io::{Read, Write};
use std::path::Path;

pub const FORMAT_VERSION: i64 = 1;

/// Zip a populated single-story package db (+ optional cover) into a `.vena` file.
/// Returns the SHA-256 of the resulting archive (shown as "SHA …" in status rows).
pub fn write_vena(
    package_db_path: &Path,
    cover: Option<(&str, &[u8])>,
    out_vena_path: &Path,
) -> Result<String> {
    let file = std::fs::File::create(out_vena_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let db_bytes = std::fs::read(package_db_path)?;
    zip.start_file("package.db", opts)?;
    zip.write_all(&db_bytes)?;

    if let Some((name, bytes)) = cover {
        zip.start_file(format!("cover_{name}"), opts)?;
        zip.write_all(bytes)?;
    }

    let manifest = serde_json::json!({ "format_version": FORMAT_VERSION });
    zip.start_file("manifest.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    zip.finish()?;

    Ok(crate::hash::sha256_hex(&std::fs::read(out_vena_path)?))
}

/// Import a `.vena` into a profile store under a fresh story_id. Every FK is
/// remapped (characters, entities, episodes, scenes, facts.known_by, edges).
/// Returns the new story_id. Schema-validates the package before copying.
pub fn import_vena(profile: &Store, vena_path: &Path) -> Result<i64> {
    let tmp = tempfile::tempdir()?;
    let db_path = tmp.path().join("package.db");
    let mut cover_asset: Option<String> = None;

    // Unzip package.db + cover.
    {
        let f = std::fs::File::open(vena_path)?;
        let mut archive =
            zip::ZipArchive::new(f).map_err(|e| VenaError::InvalidPackage(e.to_string()))?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if name == "package.db" {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                std::fs::write(&db_path, &buf)?;
            } else if name.starts_with("cover_") {
                // Zip-slip guard: a malicious package could name an entry
                // "cover_../../etc" and escape the assets dir. Use ONLY the final
                // path component, and reject anything with separators / traversal.
                let leaf = std::path::Path::new(&name)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if leaf != name || leaf.is_empty() || leaf.contains("..") {
                    return Err(VenaError::InvalidPackage(format!(
                        "unsafe asset path in package: {name}"
                    )));
                }
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                let assets = profile_asset_dir()?;
                let out = assets.join(leaf);
                std::fs::write(&out, &buf)?;
                cover_asset = Some(out.to_string_lossy().to_string());
            }
        }
    }
    if !db_path.exists() {
        return Err(VenaError::InvalidPackage("missing package.db".into()));
    }

    let pkg = Connection::open(&db_path)?;
    validate_schema(&pkg)?;

    // ---- story ----
    let (slug, title, author, license, source, cover, meta_json): (
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
    ) = pkg.query_row(
        "SELECT slug,title,author,license,source,cover,meta_json FROM story LIMIT 1",
        [],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        },
    )?;
    // Guarantee a unique slug in the profile.
    let unique_slug = crate::util::unique_slug(profile, &slug)?;
    let cover_final = cover_asset.or(cover);
    let new_sid = profile.insert_story(
        &unique_slug,
        &title,
        author.as_deref(),
        &license,
        source.as_deref(),
        cover_final.as_deref(),
        &meta_json,
    )?;

    // ---- characters (build old->new map) ----
    let mut char_map = std::collections::HashMap::new();
    {
        let mut stmt = pkg.prepare(
            "SELECT id,name,aliases_json,voice_card_json,first_appearance_chapter FROM character",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            let (old_id, name, aliases_json, voice_json, first) = row?;
            let aliases: Vec<String> = serde_json::from_str(&aliases_json).unwrap_or_default();
            let voice: VoiceCard = serde_json::from_str(&voice_json).unwrap_or_default();
            let new_id = profile.insert_character(new_sid, &name, &aliases, &voice, first)?;
            char_map.insert(old_id, new_id);
        }
    }

    // ---- entities (old->new map for edge keys) ----
    let mut entity_map = std::collections::HashMap::new();
    {
        let mut stmt =
            pkg.prepare("SELECT id,kind,name,aliases_json,first_appearance_chapter FROM entity")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            let (old_id, kind, name, aliases_json, first) = row?;
            let aliases: Vec<String> = serde_json::from_str(&aliases_json).unwrap_or_default();
            let new_id = profile.add_entity(new_sid, &kind, &name, &aliases, first)?;
            entity_map.insert(old_id, new_id);
        }
    }

    // ---- episodes + scenes ----
    {
        let mut stmt =
            pkg.prepare("SELECT id,seq,title,est_minutes,content_html FROM episode ORDER BY seq")?;
        let eps = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (old_ep, seq, title, est, html) in eps {
            let new_ep = profile.insert_episode(new_sid, seq, title.as_deref(), est, &html)?;
            let mut sstmt =
                pkg.prepare("SELECT seq,summary FROM scene WHERE episode_id=?1 ORDER BY seq")?;
            let scenes = sstmt
                .query_map(params![old_ep], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (sseq, summary) in scenes {
                profile.insert_scene(new_ep, sseq, &summary)?;
            }
        }
    }

    // ---- facts (remap subject + known_by chars; build old->new fact map for edges) ----
    let mut fact_map = std::collections::HashMap::new();
    {
        let mut stmt = pkg.prepare(
            "SELECT id,chapter_seq,subject_char_id,kind,text,known_by_json,spoiler_weight FROM fact ORDER BY id",
        )?;
        let facts = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (old_id, chapter, subj, kind, text, known_json, weight) in facts {
            let mut known: Vec<KnownBy> = serde_json::from_str(&known_json).unwrap_or_default();
            for kb in known.iter_mut() {
                if let Some(&new_c) = char_map.get(&kb.character_id) {
                    kb.character_id = new_c;
                }
            }
            let new_fact = Fact {
                id: 0,
                story_id: new_sid,
                chapter_seq: chapter,
                subject_char_id: subj.and_then(|c| char_map.get(&c).copied()),
                kind: FactKind::parse(&kind),
                text,
                known_by: known,
                spoiler_weight: weight,
            };
            let new_id = profile.insert_fact(&new_fact)?;
            fact_map.insert(old_id, new_id);
        }
    }

    // ---- edges (remap entity keys + source fact) ----
    {
        let mut stmt = pkg.prepare(
            "SELECT from_entity,to_entity,rel_type,since_chapter,until_chapter,source_fact_id FROM edge",
        )?;
        let edges = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (from, to, rel, since, until, src) in edges {
            let from2 = remap_entity_key(&from, &char_map, &entity_map);
            let to2 = remap_entity_key(&to, &char_map, &entity_map);
            let src2 = src.and_then(|s| fact_map.get(&s).copied());
            profile.add_edge(new_sid, &from2, &to2, &rel, since, until, src2)?;
        }
    }

    Ok(new_sid)
}

fn remap_entity_key(
    key: &str,
    char_map: &std::collections::HashMap<i64, i64>,
    entity_map: &std::collections::HashMap<i64, i64>,
) -> String {
    if let Some(rest) = key.strip_prefix("char:") {
        if let Ok(old) = rest.parse::<i64>() {
            if let Some(&new) = char_map.get(&old) {
                return format!("char:{new}");
            }
        }
    } else if let Some(rest) = key.strip_prefix("entity:") {
        if let Ok(old) = rest.parse::<i64>() {
            if let Some(&new) = entity_map.get(&old) {
                return format!("entity:{new}");
            }
        }
    }
    key.to_string()
}

fn validate_schema(pkg: &Connection) -> Result<()> {
    for table in ["story", "episode", "scene", "character", "fact"] {
        let exists: Option<i64> = pkg
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(VenaError::InvalidPackage(format!("missing table {table}")));
        }
    }
    let n: i64 = pkg.query_row("SELECT COUNT(*) FROM story", [], |r| r.get(0))?;
    if n != 1 {
        return Err(VenaError::InvalidPackage(format!(
            "expected exactly one story, found {n}"
        )));
    }
    Ok(())
}

fn profile_asset_dir() -> Result<std::path::PathBuf> {
    // Covers imported from packages live beside the profile db. The app sets
    // VENA_ASSET_DIR; fall back to a temp dir for headless/CLI use.
    let dir = std::env::var("VENA_ASSET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("vena-assets"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
