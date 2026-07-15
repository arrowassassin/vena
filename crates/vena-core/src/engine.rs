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
        self.companion_turn_with_history(
            store,
            story_id,
            character_id,
            message,
            None,
            &[],
            on_stage,
        )
    }

    /// Condense a stretch of conversation into a short durable memory note —
    /// the compaction half of chatbot memory. Derives ONLY from already-gated
    /// dialogue, so the note inherits the gate.
    pub fn condense(&self, turns: &[(String, String)]) -> Result<String> {
        let mut convo = String::new();
        for (role, text) in turns {
            convo.push_str(if role == "user" {
                "READER"
            } else {
                "COMPANION"
            });
            convo.push_str(": ");
            let line: String = text.chars().take(400).collect();
            convo.push_str(&line);
            convo.push('\n');
        }
        self.backend.complete(
            "You keep the private notebook of an in-story companion. Condense this conversation \
             into at most five short sentences of durable memory: what the reader asked about, \
             cared about, was told, or promised. Plain prose. Add NOTHING that is not in the text.",
            &convo,
            &GenOptions {
                max_tokens: 220,
                temperature: 0.2,
                json: false,
            },
        )
    }

    /// Companion turn with conversation memory, chatbot-shaped: `memory` is
    /// the rolling condensed note over older exchanges (see `condense`), and
    /// `history` is the recent verbatim window — (role, text) pairs, oldest
    /// first. Both were spoiler-gated when written AND replay only at-or-below
    /// the reader's current bookmark (Store::recent_messages /
    /// latest_chat_memory), so continuity never opens a side door past the
    /// gate. VERIFY still runs on every new reply.
    #[allow(clippy::too_many_arguments)]
    pub fn companion_turn_with_history(
        &self,
        store: &Store,
        story_id: i64,
        character_id: Option<i64>,
        message: &str,
        memory: Option<&str>,
        history: &[(String, String)],
        on_stage: &mut dyn FnMut(&str),
    ) -> Result<TurnReport> {
        // ---- STAGE 1: GATE + ASSEMBLE (the only stage that needs the store) ----
        on_stage("gate");
        let mut gated = gate_and_assemble(store, story_id, character_id, message)?;
        apply_memory(&mut gated.system, memory);
        // Stages 3–5 touch no store, so the caller can drop the profile lock
        // before this (a local 7B turn is tens of seconds — see AppApi).
        self.finish_turn(gated, message, history, on_stage)
    }

    /// Stages 2.5–5 (guard → generate → verify → repair) over an already-gated
    /// turn. Deliberately takes NO store: the gate ran up front, so inference —
    /// the slow part — happens with no lock held.
    pub fn finish_turn(
        &self,
        gated: GatedTurn,
        message: &str,
        history: &[(String, String)],
        on_stage: &mut dyn FnMut(&str),
    ) -> Result<TurnReport> {
        let GatedTurn {
            system,
            visible,
            forbidden,
            unmet,
            character,
        } = gated;

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

        // ---- STAGE 3: GENERATE ----
        // History rides as REAL chat turns (native chat template on backends
        // that have one) — that, not prompt text, is what makes a character
        // hold a conversation instead of reciting.
        // Chat replies are capped well below the default 512: the prompt asks
        // for 1–5 sentences, and on a local 7B every surplus token is latency.
        on_stage("compose");
        let chat_opts = GenOptions {
            max_tokens: 256,
            temperature: 0.8,
            json: false,
        };
        let draft = self.backend.chat(&system, history, message, &chat_opts)?;

        // ---- STAGE 4: VERIFY ----
        on_stage("verify");
        let mut report = self.verify_reply(&draft, &visible, &forbidden, &unmet, &character);

        // ---- STAGE 5: REPAIR ----
        if report_has_violation(&report) {
            on_stage("repair"); // design stamp: "INKING OUT A SPOILER"
            report = self.repair(
                message, &system, &visible, &forbidden, &unmet, &character, report,
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

        // STANDARD / RELAXED: one repair regeneration with a "you do not know X yet"
        // instruction, then re-verify.
        //
        // INVARIANT (§11.4a Cloud Relay): a remote backend must NEVER receive ungated
        // content. The forbidden facts are exactly the ungated/future spoilers, so we
        // only ever disclose their verbatim text to a LOCAL backend (it stays on the
        // device). For Cloud Relay we send a neutral instruction with no spoiler text.
        let repair_system = if self.backend.is_remote() {
            format!(
                "{system}\n\nIMPORTANT: Your previous reply drifted into events beyond \
                 where the reader has read. Do NOT speculate about, reveal, or imply any \
                 future outcome — answer using ONLY what you already know."
            )
        } else {
            let forbidden_topics: Vec<String> = first
                .claims
                .iter()
                .filter(|c| c.verdict == "violation")
                .filter_map(|c| c.matched_fact_id)
                .filter_map(|fid| forbidden.iter().find(|f| f.id == fid))
                .map(|f| f.text.clone())
                .collect();
            format!(
                "{system}\n\nIMPORTANT: You do NOT yet know any of the following — do not \
                 reference or imply them: {}",
                forbidden_topics.join("; ")
            )
        };
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
        // unmet_character violations carry a synthetic claim ("names unmet
        // character: X") that never text-matches the offending sentence, so the
        // name itself is what must be scrubbed; every other violation is matched
        // against the sentence text by similarity.
        let mut violating: Vec<String> = Vec::new();
        let mut leaked_names: Vec<String> = Vec::new();
        for c in report.claims.iter().filter(|c| c.verdict == "violation") {
            match c.claim.strip_prefix("names unmet character: ") {
                Some(name) => leaked_names.push(name.to_string()),
                None => violating.push(c.claim.clone()),
            }
        }
        let mut kept: Vec<String> = report
            .reply
            .split_inclusive(['.', '!', '?'])
            .filter(|sent| {
                !violating.iter().any(|v| verify::similarity(sent, v) >= 0.5)
                    && verify::unmet_characters(sent, leaked_names.iter().map(String::as_str))
                        .is_empty()
            })
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
            // A probe "leaks" if the FINAL reply the user would see still confirms
            // the future fact — verified INDEPENDENTLY of the pipeline's own
            // repaired/redacted bookkeeping (redaction can miss a sentence; a
            // repaired-but-still-leaking reply is still a leak). We re-match the
            // probed fact against the delivered text with the verifier.
            let leaked =
                crate::verify::similarity(&report.reply, &f.text) >= self.gate_mode.threshold();
            let leak_kind = report.leaks_caught.first().copied().or(if leaked {
                Some(LeakKind::FutureEvent)
            } else {
                None
            });
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

/// The output of stages 1 + 1.5 + 2 — everything deterministic that happens before
/// any model is invoked. Public so the eval harness can export the EXACT prompts
/// the production pipeline would send (out-of-process / human-in-the-loop eval).
pub struct GatedTurn {
    pub system: String,
    pub visible: Vec<Fact>,
    pub forbidden: Vec<Fact>,
    pub unmet: Vec<String>,
    pub character: Option<Character>,
}

/// Append the character's condensed relationship memory to the assembled
/// system prompt. Kept out of gate_and_assemble so the API can run it after
/// dropping the store lock. The memory note was itself spoiler-gated when
/// written, so this opens no side door past the gate.
pub fn apply_memory(system: &mut String, memory: Option<&str>) {
    if let Some(m) = memory {
        system.push_str(
            "\n\n== WHAT YOU REMEMBER OF YOUR EARLIER TALKS WITH THE READER (your own private notes) ==\n",
        );
        system.push_str(m);
        system.push('\n');
    }
}

/// STAGE 1 (gate) + 1.5 (graph retrieval) + 2 (assemble). Deterministic; no model.
pub fn gate_and_assemble(
    store: &Store,
    story_id: i64,
    character_id: Option<i64>,
    message: &str,
) -> Result<GatedTurn> {
    let progress = store.get_progress(story_id)?.0;
    let keyword = store.gated_facts(story_id, progress, character_id, message, 24)?;
    // Graph edges are chapter-stamped, so stage-1.5 retrieval is spoiler-safe by
    // construction (§6b).
    let graph = store.graph_facts(story_id, progress, character_id, message, 2)?;
    let visible = crate::graph::merge_retrieval(keyword, graph, message, 24);
    let forbidden = store.forbidden_facts(story_id, progress, character_id)?;
    let unmet = store.unmet_character_names(story_id)?;
    let character = match character_id {
        Some(cid) => Some(store.get_character(story_id, cid)?),
        None => None,
    };
    let system = assemble_prompt(&character, progress, &visible);
    Ok(GatedTurn {
        system,
        visible,
        forbidden,
        unmet,
        character,
    })
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
                 A sample of your voice (for TONE only — never quote it, never reuse its \
                 imagery): \"{sample}\".\n\
                 You exist at chapter {progress} — this moment is your present. You know ONLY \
                 what you have lived. If asked about things you have not yet lived, respond with \
                 in-character ignorance or curiosity. Never reference narrator knowledge, never \
                 wink at the reader.\n\
                 You are in CONVERSATION with a reader — talk like a person, not a novel:\n\
                 - Match their register. A short or casual message gets a short, warm reply \
                 (one or two sentences). Only go longer when they ask for more.\n\
                 - Answer what they actually said, plainly first; color it in your voice second.\n\
                 - Vary your language. NEVER repeat a phrase, image, or sentence you already \
                 used earlier in this conversation — say something new each time.\n\
                 - Do not end every reply with a question. Most replies should simply end; ask \
                 a question back only when you genuinely want their answer, at most one in three.\n\
                 - Never monologue, never recite your journal unprompted, never speak in \
                 riddles when a straight answer exists. When they float a theory, weigh it \
                 honestly against what you have lived.\n\
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
             Talk like a person: answer directly and briefly — a casual message gets one or two \
             sentences. Vary your language between replies; never reuse an earlier phrase or \
             image. Do not end most replies with a question.\n\
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
/// Public: the eval's prompt-export uses it to mark interviews the engine would
/// deflect without ever calling a model.
pub fn is_fate_question(message: &str) -> bool {
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
