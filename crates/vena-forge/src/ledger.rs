//! Ledger extraction (§7, Appendix B) — the expensive step. Two real paths.
//! `extract_with_model` runs a strong model over each chapter (the full-tier
//! forge; used by maintainers with a BYO key, or a local big model).
//! `load_curated` loads a maintainer-authored/verified ledger for a flagship
//! prebuilt package (§0.5.5, §7) — real data, hand-checked.
//! Both feed the SAME downstream: scene segmentation, story-graph edge derivation,
//! and package assembly.

use crate::import::Chapter;
use anyhow::{Context, Result};
use serde::Deserialize;
use vena_core::inference::{GenOptions, Inference};
use vena_core::model::{FactKind, VoiceCard};

/// A resolved, forge-ready ledger (names not yet mapped to ids).
#[derive(Debug, Default)]
pub struct Ledger {
    pub characters: Vec<CharacterSpec>,
    pub entities: Vec<EntitySpec>,
    pub facts: Vec<FactSpec>,
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Clone)]
pub struct CharacterSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub first_appearance_chapter: i64,
    pub voice: VoiceCard,
}

#[derive(Debug, Clone)]
pub struct EntitySpec {
    pub kind: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub first_appearance_chapter: i64,
}

#[derive(Debug, Clone)]
pub struct FactSpec {
    pub chapter: i64,
    pub subject: Option<String>,
    pub kind: FactKind,
    pub text: String,
    /// (character name, learned_at_chapter)
    pub known_by: Vec<(String, i64)>,
    pub spoiler_weight: i64,
}

#[derive(Debug, Clone)]
pub struct EdgeSpec {
    pub from: String, // entity name
    pub to: String,
    pub rel_type: String,
    pub since_chapter: i64,
    pub until_chapter: Option<i64>,
}

// ---------- curated (maintainer prebuilt) ----------

#[derive(Debug, Deserialize)]
pub struct CuratedLedger {
    pub title: String,
    pub author: Option<String>,
    #[serde(default = "default_license")]
    pub license: String,
    pub slug: String,
    #[serde(default)]
    pub characters: Vec<CuratedCharacter>,
    #[serde(default)]
    pub entities: Vec<CuratedEntity>,
    #[serde(default)]
    pub facts: Vec<CuratedFact>,
    #[serde(default)]
    pub edges: Vec<CuratedEdge>,
}
fn default_license() -> String {
    "public-domain".into()
}

#[derive(Debug, Deserialize)]
pub struct CuratedCharacter {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub first_appearance_chapter: i64,
    #[serde(default)]
    pub voice: VoiceCard,
}
#[derive(Debug, Deserialize)]
pub struct CuratedEntity {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default = "one")]
    pub first_appearance_chapter: i64,
}
#[derive(Debug, Deserialize)]
pub struct CuratedFact {
    pub chapter: i64,
    #[serde(default)]
    pub subject: Option<String>,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub known_by: Vec<CuratedKnownBy>,
    #[serde(default = "one")]
    pub spoiler_weight: i64,
}
#[derive(Debug, Deserialize)]
pub struct CuratedKnownBy {
    pub character: String,
    #[serde(default)]
    pub learned_at: Option<i64>,
}
#[derive(Debug, Deserialize)]
pub struct CuratedEdge {
    pub from: String,
    pub to: String,
    pub rel_type: String,
    pub since_chapter: i64,
    #[serde(default)]
    pub until_chapter: Option<i64>,
}
fn one() -> i64 {
    1
}

pub fn load_curated(json: &str) -> Result<(CuratedLedger, Ledger)> {
    let c: CuratedLedger = serde_json::from_str(json).context("parsing curated ledger")?;
    let mut l = Ledger::default();
    for ch in &c.characters {
        l.characters.push(CharacterSpec {
            name: ch.name.clone(),
            aliases: ch.aliases.clone(),
            first_appearance_chapter: ch.first_appearance_chapter,
            voice: ch.voice.clone(),
        });
    }
    for e in &c.entities {
        l.entities.push(EntitySpec {
            kind: e.kind.clone(),
            name: e.name.clone(),
            aliases: e.aliases.clone(),
            first_appearance_chapter: e.first_appearance_chapter,
        });
    }
    for f in &c.facts {
        l.facts.push(FactSpec {
            chapter: f.chapter,
            subject: f.subject.clone(),
            kind: FactKind::parse(&f.kind),
            text: f.text.clone(),
            known_by: f
                .known_by
                .iter()
                .map(|k| (k.character.clone(), k.learned_at.unwrap_or(f.chapter)))
                .collect(),
            spoiler_weight: f.spoiler_weight,
        });
    }
    for e in &c.edges {
        l.edges.push(EdgeSpec {
            from: e.from.clone(),
            to: e.to.clone(),
            rel_type: e.rel_type.clone(),
            since_chapter: e.since_chapter,
            until_chapter: e.until_chapter,
        });
    }
    // NB: derivation of edges from relationship facts (v2.0) happens in the forge,
    // AFTER facts are inserted, so each derived edge can cite its source fact id.
    Ok((c, l))
}

// ---------- model extraction (Appendix B) ----------

#[derive(Debug, Deserialize)]
struct ChapterExtract {
    #[serde(default)]
    facts: Vec<RawFact>,
    #[serde(default)]
    new_characters: Vec<RawChar>,
}
#[derive(Debug, Deserialize)]
struct RawFact {
    text: String,
    #[serde(default = "ev")]
    kind: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    known_by: Vec<RawKnown>,
    #[serde(default = "one")]
    spoiler_weight: i64,
}
fn ev() -> String {
    "event".into()
}
#[derive(Debug, Deserialize)]
struct RawKnown {
    character: String,
    #[serde(default)]
    learned_this_chapter: bool,
}
#[derive(Debug, Deserialize)]
struct RawChar {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    voice: VoiceCard,
}

const LEDGER_PROMPT: &str = r#"You are building a spoiler-safety knowledge ledger for a fiction reading companion. You will be given ONE chapter of a novel. Extract every fact a reader learns in THIS chapter.

Rules:
- Each fact must be ONE atomic clause (subject-verb-object). Split compound facts.
- kind: one of event | relationship | secret | world | death | reveal.
- spoiler_weight: 0 ambient background, 1 ordinary plot event, 2 significant development, 3 twist/major reveal/death. Be strict: weight-3 facts must NEVER leak early - mark every death, identity reveal, betrayal, and mystery solution as 3.
- known_by: which CHARACTERS witness or learn this fact in this chapter (the reader always learns it; characters may not). Use exact names from the known-character list; new characters go in new_characters.
- new_characters: first-time characters, with 2-4 aliases and a voice card: {"diction":"...","temperament":"...","speech_sample":"verbatim quote"}.
- Do NOT include facts from other chapters, your memory of this novel, or literary analysis. Only what THIS text states.

Output STRICT JSON only:
{"facts":[{"text","kind","subject","known_by":[{"character","learned_this_chapter"}],"spoiler_weight"}],"new_characters":[{"name","aliases":[],"voice":{"diction","temperament","speech_sample"}}]}"#;

/// Full-tier forge: run the model over each chapter (Appendix B). Real inference —
/// works with Cloud Relay (BYO key) or a local big model. `on_progress(seq, total)`.
pub fn extract_with_model(
    backend: &dyn Inference,
    chapters: &[Chapter],
    mut on_progress: impl FnMut(i64, i64),
) -> Result<Ledger> {
    let mut ledger = Ledger::default();
    let mut known: Vec<String> = Vec::new();
    let total = chapters.len() as i64;

    for ch in chapters {
        let partial = extract_chapter(backend, ch, &mut known)?;
        ledger.characters.extend(partial.characters);
        ledger.entities.extend(partial.entities);
        ledger.facts.extend(partial.facts);
        ledger.edges.extend(partial.edges);
        on_progress(ch.seq, total);
    }

    Ok(ledger)
}

/// Extract ONE chapter's ledger slice — the facts a reader learns in this chapter
/// plus any first-appearing characters. `known` is the running roster (mutated so
/// later chapters know prior characters). Returned as a single-chapter `Ledger` so
/// the caller can INSERT INCREMENTALLY: the chapter-gated store makes those facts
/// live for chat the instant they land, so a reader can chat about chapter 1 while
/// chapter 20 is still forging (§6 — the gate is per-fact, not per-book).
pub fn extract_chapter(
    backend: &dyn Inference,
    ch: &Chapter,
    known: &mut Vec<String>,
) -> Result<Ledger> {
    let mut ledger = Ledger::default();
    let body = ch
        .paragraphs
        .join("\n\n")
        .chars()
        .take(24_000)
        .collect::<String>();
    let user = format!(
        "Chapter number: {}\nKnown characters so far: {}\n\nChapter text:\n{}",
        ch.seq,
        known.join(", "),
        body
    );
    let opts = GenOptions {
        json: true,
        temperature: 0.2,
        max_tokens: 2048,
    };
    let raw = backend
        .complete(LEDGER_PROMPT, &user, &opts)
        .map_err(|e| anyhow::anyhow!("forge inference failed at chapter {}: {e}", ch.seq))?;
    let extract: ChapterExtract = parse_json_lax(&raw)
        .with_context(|| format!("parsing ledger JSON for chapter {}", ch.seq))?;

    for nc in extract.new_characters {
        if !known.iter().any(|k| k.eq_ignore_ascii_case(&nc.name)) {
            known.push(nc.name.clone());
            ledger.characters.push(CharacterSpec {
                name: nc.name,
                aliases: nc.aliases,
                first_appearance_chapter: ch.seq,
                voice: nc.voice,
            });
        }
    }
    for f in extract.facts {
        // Record only characters who actually learn the fact in THIS chapter
        // (learned_this_chapter); others mentioned but not yet witnessing are
        // excluded so per-character knowledge lags the reader correctly.
        let known_by: Vec<(String, i64)> = f
            .known_by
            .iter()
            .filter(|k| k.learned_this_chapter)
            .map(|k| (k.character.clone(), ch.seq))
            .collect();
        ledger.facts.push(FactSpec {
            chapter: ch.seq,
            subject: f.subject,
            kind: FactKind::parse(&f.kind),
            text: f.text,
            known_by,
            spoiler_weight: f.spoiler_weight.clamp(0, 3),
        });
    }
    Ok(ledger)
}

/// Extract a JSON object from a model reply that may wrap it in prose/markdown.
fn parse_json_lax<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<T> {
    let start = raw.find('{').context("no JSON object in reply")?;
    let end = raw.rfind('}').context("no closing brace in reply")? + 1;
    Ok(serde_json::from_str(&raw[start..end])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_curated_parses_full_ledger() {
        let json = r#"{
          "title": "Test Book", "author": "A. Writer", "slug": "test-book",
          "characters": [
            {"name": "Alice", "aliases": ["Al"], "first_appearance_chapter": 1}
          ],
          "entities": [{"kind": "place", "name": "The Manor"}],
          "facts": [
            {"chapter": 1, "kind": "event", "text": "Alice arrives",
             "known_by": [{"character": "Alice", "learned_at": 1}], "spoiler_weight": 1},
            {"chapter": 5, "kind": "death", "text": "Someone dies"}
          ],
          "edges": [
            {"from": "char:Alice", "to": "place:The Manor", "rel_type": "visits", "since_chapter": 1}
          ]
        }"#;
        let (curated, ledger) = load_curated(json).unwrap();
        assert_eq!(curated.slug, "test-book");
        assert_eq!(curated.license, "public-domain"); // defaulted
        assert_eq!(ledger.characters.len(), 1);
        assert_eq!(ledger.entities.len(), 1);
        assert_eq!(ledger.facts.len(), 2);
        assert_eq!(ledger.edges.len(), 1);
        // spoiler_weight defaults to 1 when omitted (the ch5 death)
        assert_eq!(ledger.facts[1].spoiler_weight, 1);
        // known_by learned_at defaults to the fact's chapter when omitted
        assert_eq!(ledger.characters[0].name, "Alice");
    }

    #[test]
    fn load_curated_rejects_malformed_json() {
        assert!(load_curated("{ not json").is_err());
        assert!(load_curated(r#"{"title":"x"}"#).is_err()); // missing required slug
    }
}
