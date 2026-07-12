//! Archive = a per-book wiki generated from the ledger (§0.5.4). No new AI infra —
//! pages are ledger facts grouped by entity. `synced` filters to chapter ≤ progress
//! ("sealed"); `full` ("unsealed") REQUIRES the per-book consent flag.

use crate::error::{Result, VenaError};
use crate::model::FactKind;
use crate::store::Store;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WikiMode {
    /// spoiler-safe, filtered to chapter_seq ≤ progress
    Synced,
    /// full-spoiler; requires consent
    Full,
}

impl WikiMode {
    pub fn parse(s: &str) -> WikiMode {
        match s {
            "full" => WikiMode::Full,
            _ => WikiMode::Synced,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WikiEntry {
    pub id: String,
    pub name: String,
    /// people | places | terms
    pub group: String,
    pub fact_count: i64,
    /// facts hidden by the seal at current progress (shown as "N SEALED")
    pub sealed_count: i64,
}

#[derive(Debug, Serialize)]
pub struct WikiIndex {
    pub mode: String,
    pub entries: Vec<WikiEntry>,
    pub sealed_total: i64,
}

#[derive(Debug, Serialize)]
pub struct WikiSection {
    pub heading: String,
    pub facts: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct WikiPage {
    pub entity_id: String,
    pub title: String,
    pub mode: String,
    pub sections: Vec<WikiSection>,
    pub unsealed: bool,
}

const CONSENT_KEY_PREFIX: &str = "spoiler_consent:";

pub fn set_consent(store: &Store, story_id: i64, granted: bool) -> Result<()> {
    store.set_setting(
        &format!("{CONSENT_KEY_PREFIX}{story_id}"),
        if granted { "1" } else { "0" },
    )
}

pub fn has_consent(store: &Store, story_id: i64) -> Result<bool> {
    Ok(store
        .get_setting(&format!("{CONSENT_KEY_PREFIX}{story_id}"))?
        .as_deref()
        == Some("1"))
}

fn require_full_allowed(store: &Store, story_id: i64, mode: WikiMode) -> Result<()> {
    if mode == WikiMode::Full && !has_consent(store, story_id)? {
        return Err(VenaError::SpoilerConsentRequired);
    }
    Ok(())
}

fn group_for(kind: FactKind) -> &'static str {
    match kind {
        FactKind::World => "places",
        FactKind::Relationship | FactKind::Secret | FactKind::Death | FactKind::Reveal => "people",
        FactKind::Event => "terms",
    }
}

pub fn get_wiki_index(store: &Store, story_id: i64, mode: WikiMode) -> Result<WikiIndex> {
    require_full_allowed(store, story_id, mode)?;
    let progress = store.get_progress(story_id)?.0;
    let characters = store.list_characters(story_id)?;
    let all_facts = store.facts_at_or_before(story_id, i64::MAX)?; // all facts

    let mut entries = Vec::new();
    let mut sealed_total = 0;

    for c in &characters {
        let subject_facts: Vec<_> = all_facts
            .iter()
            .filter(|f| f.subject_char_id == Some(c.id))
            .collect();
        let visible = subject_facts
            .iter()
            .filter(|f| f.chapter_seq <= progress)
            .count() as i64;
        let total = subject_facts.len() as i64;
        let sealed = total - visible;
        // In synced mode, an entity the reader hasn't met is itself sealed.
        let shown = match mode {
            WikiMode::Synced => c.first_appearance_chapter <= progress,
            WikiMode::Full => true,
        };
        if !shown {
            sealed_total += 1;
            continue;
        }
        sealed_total += if mode == WikiMode::Synced { sealed } else { 0 };
        entries.push(WikiEntry {
            id: format!("char:{}", c.id),
            name: c.name.clone(),
            group: "people".into(),
            fact_count: if mode == WikiMode::Synced {
                visible
            } else {
                total
            },
            sealed_count: if mode == WikiMode::Synced { sealed } else { 0 },
        });
    }

    // World/place/term entries grouped from subject-less facts by kind.
    for (label, group) in [("The World", "places"), ("Terms & Things", "terms")] {
        let facts: Vec<_> = all_facts
            .iter()
            .filter(|f| f.subject_char_id.is_none() && group_for(f.kind) == group)
            .collect();
        if facts.is_empty() {
            continue;
        }
        let visible = facts.iter().filter(|f| f.chapter_seq <= progress).count() as i64;
        let total = facts.len() as i64;
        sealed_total += if mode == WikiMode::Synced {
            total - visible
        } else {
            0
        };
        entries.push(WikiEntry {
            id: format!("group:{group}"),
            name: label.into(),
            group: group.into(),
            fact_count: if mode == WikiMode::Synced {
                visible
            } else {
                total
            },
            sealed_count: if mode == WikiMode::Synced {
                total - visible
            } else {
                0
            },
        });
    }

    Ok(WikiIndex {
        mode: if mode == WikiMode::Full {
            "full"
        } else {
            "synced"
        }
        .into(),
        entries,
        sealed_total,
    })
}

pub fn get_wiki_page(
    store: &Store,
    story_id: i64,
    entity_id: &str,
    mode: WikiMode,
) -> Result<WikiPage> {
    require_full_allowed(store, story_id, mode)?;
    let progress = store.get_progress(story_id)?.0;
    let all_facts = store.facts_at_or_before(story_id, i64::MAX)?;
    let cutoff = if mode == WikiMode::Full {
        i64::MAX
    } else {
        progress
    };

    let (title, mut relevant): (String, Vec<_>) = if let Some(cid) = entity_id.strip_prefix("char:")
    {
        let cid: i64 = cid.parse().unwrap_or(-1);
        let ch = store.get_character(story_id, cid)?;
        // Synced mode seals characters the reader hasn't met yet — the same rule the
        // index applies (silhouettes). Fetching the page directly must not bypass it:
        // an unmet character's name and pre-appearance facts stay hidden until Full
        // mode (which required consent above).
        if mode != WikiMode::Full && ch.first_appearance_chapter > progress {
            return Err(VenaError::NotFound(format!(
                "wiki entity {entity_id} is still sealed — keep reading to meet them"
            )));
        }
        (
            ch.name.clone(),
            all_facts
                .iter()
                .filter(|f| f.subject_char_id == Some(cid))
                .collect(),
        )
    } else if let Some(group) = entity_id.strip_prefix("group:") {
        let group = group.to_string();
        (
            if group == "places" {
                "The World"
            } else {
                "Terms & Things"
            }
            .into(),
            all_facts
                .iter()
                .filter(|f| f.subject_char_id.is_none() && group_for(f.kind) == group)
                .collect(),
        )
    } else {
        return Err(VenaError::NotFound(format!("wiki entity {entity_id}")));
    };

    relevant.retain(|f| f.chapter_seq <= cutoff);
    relevant.sort_by_key(|f| f.chapter_seq);

    // Group into sections by fact kind for a readable article.
    let mut sections: Vec<WikiSection> = Vec::new();
    for (kind, heading) in [
        (FactKind::Relationship, "Relationships"),
        (FactKind::Event, "Events"),
        (FactKind::Secret, "Secrets"),
        (FactKind::World, "World"),
        (FactKind::Death, "Fate"),
        (FactKind::Reveal, "Revelations"),
    ] {
        let facts: Vec<String> = relevant
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| format!("(Ch. {}) {}", f.chapter_seq, f.text))
            .collect();
        if !facts.is_empty() {
            sections.push(WikiSection {
                heading: heading.into(),
                facts,
            });
        }
    }

    Ok(WikiPage {
        entity_id: entity_id.into(),
        title,
        mode: if mode == WikiMode::Full {
            "full"
        } else {
            "synced"
        }
        .into(),
        sections,
        unsealed: mode == WikiMode::Full,
    })
}
