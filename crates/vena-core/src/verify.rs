//! Stage 4 — claim verification with the leak taxonomy (§6, §11.4a).
//!
//! Phase-1 matching is **lexical** (token-overlap), threshold tunable by GateMode.
//! The upgrade path to semantic matching (sqlite-vec) is documented in §6b; the
//! interface here (`match_claim`) is the seam where that swaps in.

use crate::model::{ClaimCheck, Fact, LeakKind};
use std::collections::HashSet;

/// Split a draft reply into atomic claim strings. The lexical stand-in for the
/// "cheap second pass, same model, JSON schema" extractor in §6 — sentence-level
/// segmentation, pleasantries dropped. Good enough for gate regression; a model
/// extractor can replace this behind the same signature.
pub fn extract_claims(draft: &str) -> Vec<String> {
    draft
        .split(['.', '!', '?', '\n'])
        .map(|s| s.trim())
        .filter(|s| s.split_whitespace().count() >= 3) // ignore "Yes.", "Indeed!"
        .filter(|s| !is_pleasantry(s))
        .map(|s| s.to_string())
        .collect()
}

fn is_pleasantry(s: &str) -> bool {
    let l = s.to_lowercase();
    const OPENERS: &[&str] = &[
        "thank you",
        "i am glad",
        "how kind",
        "good day",
        "my dear friend",
        "it is a pleasure",
    ];
    OPENERS.iter().any(|o| l.starts_with(o))
}

fn tokenize(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2 && !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

const STOPWORDS: &[&str] = &[
    "the", "and", "that", "have", "for", "not", "with", "you", "this", "but", "his", "her", "she",
    "him", "they", "them", "was", "are", "were", "had", "has", "who", "what", "when", "there",
    "would", "could", "should", "will", "from", "been", "their", "your",
];

/// Token-overlap similarity in [0,1] (Szymkiewicz–Simpson overlap coefficient —
/// robust when the fact clause is much shorter than the reply sentence).
pub fn similarity(a: &str, b: &str) -> f32 {
    let ta = tokenize(a);
    let tb = tokenize(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f32;
    let denom = ta.len().min(tb.len()) as f32;
    inter / denom
}

/// Match one claim against the ledger split into visible (chapter ≤ progress and
/// character-visible) and future/forbidden facts. Returns the verdict + leak kind.
///
/// - matches a visible fact              -> ok
/// - matches a future/forbidden fact     -> violation (future_event)
/// - no match, but touches a hidden      -> handled by caller via weight rules
/// - no match at all                     -> drift (tolerated; only temporal blocked)
pub fn match_claim(
    claim: &str,
    visible: &[Fact],
    forbidden: &[Fact],
    threshold: f32,
) -> ClaimCheck {
    let best_forbidden = best_match(claim, forbidden);
    let best_visible = best_match(claim, visible);

    // A temporal violation only fires when the forbidden match is both above
    // threshold AND at least as strong as any visible match (else the claim is
    // really about something the reader already knows).
    if let Some((fid, fscore)) = best_forbidden {
        let vscore = best_visible.map(|(_, s)| s).unwrap_or(0.0);
        if fscore >= threshold && fscore >= vscore {
            return ClaimCheck {
                claim: claim.to_string(),
                verdict: "violation".into(),
                leak_kind: Some(LeakKind::FutureEvent),
                matched_fact_id: Some(fid),
                score: fscore,
            };
        }
    }
    if let Some((fid, vscore)) = best_visible {
        if vscore >= threshold {
            return ClaimCheck {
                claim: claim.to_string(),
                verdict: "ok".into(),
                leak_kind: None,
                matched_fact_id: Some(fid),
                score: vscore,
            };
        }
    }
    // No confident match: invented detail. Tolerated (only temporal leaks block).
    ClaimCheck {
        claim: claim.to_string(),
        verdict: "drift".into(),
        leak_kind: None,
        matched_fact_id: None,
        score: best_visible.map(|(_, s)| s).unwrap_or(0.0),
    }
}

fn best_match(claim: &str, facts: &[Fact]) -> Option<(i64, f32)> {
    facts
        .iter()
        .map(|f| (f.id, similarity(claim, &f.text)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .filter(|(_, s)| *s > 0.0)
}

/// Deterministic pre-check (§11.4a): does the reply name a character whose
/// first_appearance is beyond the reader's progress? Runs before claim
/// extraction — cheap and catches "unmet_character" leaks the fact matcher can't.
/// Returns the offending names.
pub fn unmet_characters<'a>(
    reply: &str,
    unmet_names: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let lower = reply.to_lowercase();
    let mut hits = Vec::new();
    for name in unmet_names {
        // whole-token match to avoid "Al" matching "although"
        if name_mentioned(&lower, &name.to_lowercase()) {
            hits.push(name.to_string());
        }
    }
    hits
}

fn name_mentioned(haystack_lower: &str, name_lower: &str) -> bool {
    if name_lower.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(pos) = haystack_lower[start..].find(name_lower) {
        let i = start + pos;
        let before_ok = i == 0 || !haystack_lower.as_bytes()[i - 1].is_ascii_alphanumeric();
        let after = i + name_lower.len();
        let after_ok = after >= haystack_lower.len()
            || !haystack_lower.as_bytes()[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = i + name_lower.len();
    }
    false
}

/// STRICT-only heuristic stand-in for the LLM-judged `tone_implies_ending` check.
/// Flags replies whose certainty about a *forbidden* topic telegraphs the outcome
/// (finality words co-occurring with a strong forbidden-fact match). In production
/// STRICT mode this is replaced by a one-shot model judgment; the interface holds.
pub fn tone_implies_ending(reply: &str, forbidden: &[Fact], threshold: f32) -> bool {
    const FINALITY: &[&str] = &[
        "in the end",
        "ultimately",
        "eventually",
        "fate",
        "doomed",
        "destined",
        "will die",
        "will fall",
        "final",
        "at last",
        "you'll see",
        "trust me",
        "mark my words",
    ];
    let l = reply.to_lowercase();
    let has_finality = FINALITY.iter().any(|w| l.contains(w));
    if !has_finality {
        return false;
    }
    // finality tone AND a meaningful brush against a forbidden fact
    forbidden
        .iter()
        .any(|f| similarity(reply, &f.text) >= (threshold - 0.15).max(0.25))
}
