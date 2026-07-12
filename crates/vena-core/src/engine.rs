//! The 5-stage spoiler-resistance engine (§6). Everything before GENERATE is
//! deterministic data filtering — the model cannot leak what it never sees.
//!
//! GATE → ASSEMBLE → GENERATE → VERIFY → REPAIR. Stage progress is surfaced to the
//! UI as GATE → COMPOSE → VERIFY "engine stamps" (§11.4a) via the `on_stage` hook.

use crate::error::Result;
use crate::inference::{GenOptions, Inference};
use crate::model::*;
use crate::store::Store;
use crate::verify;

pub struct Engine {
    pub backend: Box<dyn Inference>,
    pub gate_mode: GateMode,
    pub guard_fates: bool,
    pub tone_check: bool, // STRICT telegraph check (LLM-judged in prod)
}

/// A probe result for "Test the Gate — RUN 12 PROBES".
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeResult {
    pub question: String,
    pub leaked: bool,
    pub leak_kind: Option<LeakKind>,
    pub reply: String,
}

impl Engine {
    pub fn new(backend: Box<dyn Inference>) -> Self {
        Engine {
            gate_mode: GateMode::Standard,
            guard_fates: true,
            tone_check: false,
            backend,
        }
    }

    pub fn with_mode(mut self, mode: GateMode) -> Self {
        self.gate_mode = mode;
        self.tone_check = matches!(mode, GateMode::Strict);
        self
    }

    /// One companion turn through all five stages. `on_stage` receives
    /// "gate"|"compose"|"verify" for the engine-stamp animation.
    pub fn companion_turn(
        &self,
        store: &Store,
        story_id: i64,
        character_id: Option<i64>,
        message: &str,
        on_stage: &mut dyn FnMut(&str),
    ) -> Result<TurnReport> {
        let progress = store.get_progress(story_id)?.0;

        // ---- STAGE 1: GATE (deterministic SQL) ----
        on_stage("gate");
        let visible = store.gated_facts(story_id, progress, character_id, message, 24)?;
        let forbidden = store.forbidden_facts(story_id, progress, character_id)?;
        let unmet = store.unmet_character_names(story_id)?;
        let character = match character_id {
            Some(cid) => Some(store.get_character(story_id, cid)?),
            None => None,
        };

        // ---- Guard Character Fates: short-circuit before generation ----
        if self.guard_fates && is_fate_question(message) {
            let reply = deflection(&character);
            return Ok(TurnReport {
                reply,
                repaired: false,
                redacted: false,
                claims: vec![],
                leaks_caught: vec![],
            });
        }

        // ---- STAGE 2: ASSEMBLE ----
        let system = assemble_prompt(&character, progress, &visible);

        // ---- STAGE 3: GENERATE ----
        on_stage("compose");
        let draft = self
            .backend
            .complete(&system, message, &GenOptions::default())?;

        // ---- STAGE 4: VERIFY ----
        on_stage("verify");
        let mut report = self.verify_reply(&draft, &visible, &forbidden, &unmet, &character);

        // ---- STAGE 5: REPAIR ----
        if report_has_violation(&report) {
            report = self.repair(
                store,
                story_id,
                character_id,
                message,
                &system,
                &visible,
                &forbidden,
                &unmet,
                &character,
                report,
            )?;
        }

        Ok(report)
    }

    fn verify_reply(
        &self,
        draft: &str,
        visible: &[Fact],
        forbidden: &[Fact],
        unmet_names: &[String],
        character: &Option<Character>,
    ) -> TurnReport {
        let threshold = self.gate_mode.threshold();
        let mut claims = Vec::new();
        let mut leaks = Vec::new();

        // 4a — unmet_character (deterministic, runs first)
        let unmet_hits = verify::unmet_characters(draft, unmet_names.iter().map(String::as_str));
        if !unmet_hits.is_empty() {
            leaks.push(LeakKind::UnmetCharacter);
            for name in &unmet_hits {
                claims.push(ClaimCheck {
                    claim: format!("names unmet character: {name}"),
                    verdict: "violation".into(),
                    leak_kind: Some(LeakKind::UnmetCharacter),
                    matched_fact_id: None,
                    score: 1.0,
                });
            }
        }

        // 4b — claim extraction + temporal matching
        for claim in verify::extract_claims(draft) {
            let mut check = verify::match_claim(&claim, visible, forbidden, threshold);
            // RELAXED: only weight ≥ 2 temporal violations count.
            if check.verdict == "violation" {
                if let (GateMode::Relaxed, Some(fid)) = (self.gate_mode, check.matched_fact_id) {
                    let w = forbidden
                        .iter()
                        .find(|f| f.id == fid)
                        .map(|f| f.spoiler_weight)
                        .unwrap_or(3);
                    if w < 2 {
                        check.verdict = "drift".into();
                        check.leak_kind = None;
                    }
                }
                if check.verdict == "violation" && !leaks.contains(&LeakKind::FutureEvent) {
                    leaks.push(LeakKind::FutureEvent);
                }
            }
            claims.push(check);
        }

        // 4c — tone_implies_ending (STRICT only)
        if self.tone_check && verify::tone_implies_ending(draft, forbidden, threshold) {
            leaks.push(LeakKind::ToneImpliesEnding);
            claims.push(ClaimCheck {
                claim: "tone telegraphs the outcome".into(),
                verdict: "violation".into(),
                leak_kind: Some(LeakKind::ToneImpliesEnding),
                matched_fact_id: None,
                score: threshold,
            });
        }

        let _ = character;
        TurnReport {
            reply: draft.to_string(),
            repaired: false,
            redacted: false,
            claims,
            leaks_caught: leaks,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn repair(
        &self,
        _store: &Store,
        _story_id: i64,
        _character_id: Option<i64>,
        message: &str,
        system: &str,
        visible: &[Fact],
        forbidden: &[Fact],
        unmet: &[String],
        character: &Option<Character>,
        first: TurnReport,
    ) -> Result<TurnReport> {
        // STRICT: no second chance — redact immediately.
        if matches!(self.gate_mode, GateMode::Strict) {
            return Ok(self.redact(first, character));
        }

        // STANDARD / RELAXED: one repair regeneration with an explicit "you do not
        // know X yet" instruction appended, then re-verify.
        let forbidden_topics: Vec<String> = first
            .claims
            .iter()
            .filter(|c| c.verdict == "violation")
            .filter_map(|c| c.matched_fact_id)
            .filter_map(|fid| forbidden.iter().find(|f| f.id == fid))
            .map(|f| f.text.clone())
            .collect();
        let repair_system = format!(
            "{system}\n\nIMPORTANT: You do NOT yet know any of the following — do not \
             reference or imply them: {}",
            forbidden_topics.join("; ")
        );
        let redraft = self
            .backend
            .complete(&repair_system, message, &GenOptions::default())?;
        let mut second = self.verify_reply(&redraft, visible, forbidden, unmet, character);
        second.repaired = true;

        if report_has_violation(&second) {
            // Still leaking → redact + in-character deflection.
            Ok(self.redact(second, character))
        } else {
            Ok(second)
        }
    }

    /// Redact the offending sentences and replace with an in-character deflection.
    fn redact(&self, mut report: TurnReport, character: &Option<Character>) -> TurnReport {
        let violating: Vec<String> = report
            .claims
            .iter()
            .filter(|c| c.verdict == "violation")
            .map(|c| c.claim.clone())
            .collect();
        let mut kept: Vec<String> = report
            .reply
            .split_inclusive(['.', '!', '?'])
            .filter(|sent| !violating.iter().any(|v| verify::similarity(sent, v) >= 0.5))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        kept.push(deflection(character));
        report.reply = kept.join(" ");
        report.repaired = true;
        report.redacted = true;
        report
    }

    /// "Test the Gate — RUN 12 PROBES" (§11.4a): sample future weight≥2 facts,
    /// phrase them as questions, run each through the full pipeline, report leaks.
    pub fn run_probes(&self, store: &Store, story_id: i64, n: usize) -> Result<Vec<ProbeResult>> {
        let progress = store.get_progress(story_id)?.0;
        let probes = store.future_probe_facts(story_id, progress)?;
        let mut out = Vec::new();
        for f in probes.into_iter().take(n) {
            let q = format!("Is it true that {}?", f.text.trim_end_matches('.'));
            let mut noop = |_: &str| {};
            let report = self.companion_turn(store, story_id, None, &q, &mut noop)?;
            let leaked = report_has_violation(&report) && !report.redacted && !report.repaired;
            let leak_kind = report.leaks_caught.first().copied();
            out.push(ProbeResult {
                question: q,
                leaked,
                leak_kind,
                reply: report.reply,
            });
        }
        Ok(out)
    }

    /// Narrator recap of everything up to current position (§11.2 get_recap).
    pub fn recap(&self, store: &Store, story_id: i64) -> Result<String> {
        let progress = store.get_progress(story_id)?.0;
        let facts = store.facts_at_or_before(story_id, progress)?;
        if facts.is_empty() {
            return Ok("Nothing has happened yet — the horizon starts at Chapter I.".into());
        }
        let bullets: Vec<String> = facts
            .iter()
            .filter(|f| f.spoiler_weight >= 1)
            .map(|f| format!("• {}", f.text))
            .collect();
        let context = format!(
            "You are the narrator. Recap ONLY the events below (up to chapter {progress}). \
             Never hint at what comes next. Facts:\n{}",
            bullets.join("\n")
        );
        self.backend.complete(
            &context,
            "Give me a 'previously on' recap of where I am.",
            &GenOptions::default(),
        )
    }
}

// ---------- theory resolution (progress-gated, never early) ----------

/// On progress advance, flip OPEN theories that newly-visible weight≥2 facts settle
/// (§6b). Lexical match now; never resolves from facts beyond progress.
pub fn resolve_theories(store: &Store, story_id: i64) -> Result<()> {
    let progress = store.get_progress(story_id)?.0;
    let facts = store.facts_at_or_before(story_id, progress)?;
    let reveals: Vec<&Fact> = facts.iter().filter(|f| f.spoiler_weight >= 2).collect();
    for t in store.list_theories(story_id)? {
        if t.resolved_status.is_some() {
            continue; // never re-open a resolved theory (except via re-seal rewind)
        }
        let best = reveals
            .iter()
            .map(|f| (f.chapter_seq, verify::similarity(&t.text, &f.text)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        if let Some((chapter, score)) = best {
            if score >= 0.5 {
                // High-confidence match → confirmed. (A model judgment call would
                // decide confirmed vs busted for ambiguous matches; lexical default
                // treats a strong match as confirmation.)
                store.set_theory_resolution(t.id, "confirmed", chapter)?;
            }
        }
    }
    Ok(())
}

// ---------- prompt assembly (§6, Appendix B) ----------

fn assemble_prompt(character: &Option<Character>, progress: i64, visible: &[Fact]) -> String {
    let facts_block = if visible.is_empty() {
        "(You have witnessed nothing of note yet.)".to_string()
    } else {
        visible
            .iter()
            .map(|f| format!("- {}", f.text))
            .collect::<Vec<_>>()
            .join("\n")
    };
    match character {
        Some(c) => {
            let vc = &c.voice_card;
            format!(
                "You are {name}. Diction: {diction}. Temperament: {temperament}. \
                 A sample of your voice: \"{sample}\".\n\
                 You exist at chapter {progress} — this moment is your present. You know ONLY \
                 what you have lived. If asked about things you have not yet lived, respond with \
                 in-character ignorance or curiosity. Never reference narrator knowledge, never \
                 wink at the reader.\n\
                 What you know so far:\n{facts}",
                name = c.name,
                diction = vc.diction,
                temperament = vc.temperament,
                sample = vc.speech_sample,
                progress = progress,
                facts = facts_block
            )
        }
        None => format!(
            "You are the story's narrator-companion. Discuss ONLY events up to chapter {progress}. \
             Engage with theories using only known evidence; never confirm or deny with certainty \
             anything you have not yet read. Never reference future events.\n\
             What is known so far:\n{facts}",
            progress = progress,
            facts = facts_block
        ),
    }
}

fn deflection(character: &Option<Character>) -> String {
    match character {
        Some(_) => {
            "That lies beyond where my own story has carried me — I truly cannot say.".to_string()
        }
        None => "We haven't reached that part of the tale yet — keep reading and it will come."
            .to_string(),
    }
}

/// Cheap intent check for "Guard Character Fates" (§11.4a) — a regex on fate-shaped
/// questions ("does X die?", "who's the killer?", "how does it end?"), run before
/// generation so we deflect without spending inference or risking a leak.
fn is_fate_question(message: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?ix)
              \b(does|will|do|did|is|are) \b .* \b (die|dies|died|survive|survives|live|lives|perish|betray|killed?) \b
            | \bwho \s+ (dies|survives|lives|is \s+ the \s+ killer|did \s+ it|is \s+ behind)
            | \bhow \s+ does \s+ (it|the \s+ book|the \s+ story|this) \s+ end
            | \bwhat \s+ happens \s+ to \b
            | \b(the|a) \s+ (killer|murderer|traitor|culprit) \b
            | \bhow \s+ does \s+ .* \b (die|end) \b
            ",
        )
        .expect("fate regex")
    });
    re.is_match(message)
}

fn report_has_violation(r: &TurnReport) -> bool {
    r.claims.iter().any(|c| c.verdict == "violation")
}
