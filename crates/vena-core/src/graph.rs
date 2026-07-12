//! Story graph (v2.0, §6b): a derived view of the ledger. Chapter-stamped edges,
//! spoiler-gated by construction. Powers stage-1.5 graph-guided retrieval, Archive
//! cross-links, per-character knowledge maps, and multi-hop companion answers.
//!
//! No graph database, no GraphRAG — SQLite recursive CTEs for 1–2-hop walks.

use crate::error::Result;
use crate::store::Store;
use crate::verify;
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub id: i64,
    pub from_entity: String,
    pub to_entity: String,
    pub rel_type: String,
    pub since_chapter: i64,
    pub until_chapter: Option<i64>,
    pub source_fact_id: Option<i64>,
}

impl Store {
    pub fn add_entity(
        &self,
        story_id: i64,
        kind: &str,
        name: &str,
        aliases: &[String],
        first_appearance_chapter: i64,
    ) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO entity (story_id,kind,name,aliases_json,first_appearance_chapter)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                story_id,
                kind,
                name,
                serde_json::to_string(aliases)?,
                first_appearance_chapter
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_edge(
        &self,
        story_id: i64,
        from_entity: &str,
        to_entity: &str,
        rel_type: &str,
        since_chapter: i64,
        until_chapter: Option<i64>,
        source_fact_id: Option<i64>,
    ) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO edge (story_id,from_entity,to_entity,rel_type,since_chapter,until_chapter,source_fact_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![story_id, from_entity, to_entity, rel_type, since_chapter, until_chapter, source_fact_id],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Edges valid at `progress`: since_chapter ≤ progress and not yet expired.
    pub fn gated_edges(&self, story_id: i64, progress: i64) -> Result<Vec<Edge>> {
        let mut stmt = self.conn().prepare(
            "SELECT id,from_entity,to_entity,rel_type,since_chapter,until_chapter,source_fact_id
             FROM edge
             WHERE story_id=?1 AND since_chapter<=?2
               AND (until_chapter IS NULL OR until_chapter>?2)
             ORDER BY since_chapter, id",
        )?;
        let rows = stmt
            .query_map(params![story_id, progress], |r| {
                Ok(Edge {
                    id: r.get(0)?,
                    from_entity: r.get(1)?,
                    to_entity: r.get(2)?,
                    rel_type: r.get(3)?,
                    since_chapter: r.get(4)?,
                    until_chapter: r.get(5)?,
                    source_fact_id: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Gated ego-network: entity keys reachable within `hops` from any seed,
    /// walking only edges valid at `progress`. Recursive CTE, no graph DB.
    pub fn ego_network(
        &self,
        story_id: i64,
        progress: i64,
        seeds: &[String],
        hops: i64,
    ) -> Result<Vec<Edge>> {
        if seeds.is_empty() {
            return Ok(vec![]);
        }
        // Walk in Rust over the gated edge set (small per book) — clearer than a
        // multi-seed CTE and identically correct for 1–2 hops.
        let edges = self.gated_edges(story_id, progress)?;
        let mut frontier: std::collections::HashSet<String> = seeds.iter().cloned().collect();
        let mut reached: std::collections::HashSet<String> = frontier.clone();
        let mut out: Vec<Edge> = Vec::new();
        let mut seen_edges = std::collections::HashSet::new();
        for _ in 0..hops {
            let mut next = std::collections::HashSet::new();
            for e in &edges {
                let touches = frontier.contains(&e.from_entity) || frontier.contains(&e.to_entity);
                if touches && seen_edges.insert(e.id) {
                    out.push(e.clone());
                    for end in [&e.from_entity, &e.to_entity] {
                        if reached.insert(end.clone()) {
                            next.insert(end.clone());
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(out)
    }

    /// Resolve entities named in a message to their string keys (met characters +
    /// discovered entities only — unmet ones can't be named without leaking).
    pub fn resolve_entities(&self, story_id: i64, message: &str) -> Result<Vec<String>> {
        let progress = self.get_progress(story_id)?.0;
        let lower = message.to_lowercase();
        let mut keys = Vec::new();
        for c in self.list_characters(story_id)? {
            if !c.met {
                continue;
            }
            // Match the full name, any alias, or any significant name token (so
            // "Jonathan" resolves "Jonathan Harker"). Tokens < 4 chars are skipped
            // to avoid matching particles like "van"/"de".
            let mut candidates: Vec<String> = vec![c.name.to_lowercase()];
            candidates.extend(c.aliases.iter().map(|a| a.to_lowercase()));
            candidates.extend(
                c.name
                    .split_whitespace()
                    .filter(|w| w.len() >= 4)
                    .map(|w| w.to_lowercase()),
            );
            if candidates.iter().any(|n| mentions_word(&lower, n)) {
                keys.push(format!("char:{}", c.id));
            }
        }
        // Non-character entities visible at progress.
        let mut stmt = self.conn().prepare(
            "SELECT id,name,aliases_json FROM entity
             WHERE story_id=?1 AND first_appearance_chapter<=?2",
        )?;
        let rows = stmt.query_map(params![story_id, progress], |r| {
            let id: i64 = r.get(0)?;
            let name: String = r.get(1)?;
            let aliases: String = r.get(2)?;
            Ok((id, name, aliases))
        })?;
        for row in rows {
            let (id, name, aliases_json) = row?;
            let mut names = vec![name];
            if let Ok(a) = serde_json::from_str::<Vec<String>>(&aliases_json) {
                names.extend(a);
            }
            if names
                .iter()
                .any(|n| mentions_word(&lower, &n.to_lowercase()))
            {
                keys.push(format!("entity:{id}"));
            }
        }
        Ok(keys)
    }

    /// STAGE 1.5 — graph-guided retrieval (§6b). Resolve entities in the message,
    /// pull their gated ego-network (≤ `hops`), and return the gated facts linked to
    /// any reached character. The engine merges these with keyword results.
    pub fn graph_facts(
        &self,
        story_id: i64,
        progress: i64,
        character_id: Option<i64>,
        message: &str,
        hops: i64,
    ) -> Result<Vec<crate::model::Fact>> {
        let seeds = self.resolve_entities(story_id, message)?;
        if seeds.is_empty() {
            return Ok(vec![]);
        }
        let net = self.ego_network(story_id, progress, &seeds, hops)?;
        // Collect character ids reached in the ego-network.
        let mut char_ids = std::collections::HashSet::new();
        for e in &net {
            for end in [&e.from_entity, &e.to_entity] {
                if let Some(rest) = end.strip_prefix("char:") {
                    if let Ok(id) = rest.parse::<i64>() {
                        char_ids.insert(id);
                    }
                }
            }
        }
        // Also include the source facts of the edges themselves.
        let source_fact_ids: std::collections::HashSet<i64> =
            net.iter().filter_map(|e| e.source_fact_id).collect();

        // Gated facts whose subject is a reached character OR that a walked edge cites.
        let gated = self.gated_facts(story_id, progress, character_id, "", usize::MAX)?;
        Ok(gated
            .into_iter()
            .filter(|f| {
                f.subject_char_id
                    .map(|c| char_ids.contains(&c))
                    .unwrap_or(false)
                    || source_fact_ids.contains(&f.id)
            })
            .collect())
    }
}

/// Whole-word (token-boundary) containment, so "helsing" matches "van helsing" but
/// "art" does not match "smart". Inputs are already lowercased.
fn mentions_word(haystack_lower: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    let bytes = haystack_lower.as_bytes();
    let mut start = 0;
    while let Some(pos) = haystack_lower[start..].find(needle_lower) {
        let i = start + pos;
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let after = i + needle_lower.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = i + needle_lower.len();
    }
    false
}

/// Merge keyword-ranked facts with graph-retrieved facts, dedup by id, re-rank the
/// combined set by relevance to the message, and take top-k (§6b stage-1.5 merge).
pub fn merge_retrieval(
    keyword: Vec<crate::model::Fact>,
    graph: Vec<crate::model::Fact>,
    message: &str,
    k: usize,
) -> Vec<crate::model::Fact> {
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<crate::model::Fact> = Vec::new();
    for f in keyword.into_iter().chain(graph.into_iter()) {
        if seen.insert(f.id) {
            merged.push(f);
        }
    }
    if !message.trim().is_empty() {
        merged.sort_by(|a, b| {
            verify::similarity(message, &b.text)
                .partial_cmp(&verify::similarity(message, &a.text))
                .unwrap()
        });
    }
    merged.truncate(k);
    merged
}
