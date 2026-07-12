//! vena-eval — the Phase-1 gate (§9, §11.6). Runs point-in-time interviews
//! (Appendix C) through the FULL 5-stage engine and reports consistency %, leak %,
//! latency p50/p95, redaction %, and the GO / PIVOT / KILL verdict.
//!
//! Two modes:
//!   * generative (a backend is configured via VENA_BASE_URL/KEY/MODEL, or a local
//!     OpenAI-compat server): real replies, real numbers — the actual Phase-1 gate.
//!   * gate-audit (no backend): exercises the DETERMINISTIC containment guarantee —
//!     for every interview, verify no forbidden/future fact and no unmet character
//!     can reach the model's context. This proves the gate holds even with no model,
//!     and is what ships as the in-app "Test the Gate" trust feature.

use anyhow::{Context, Result};
use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use vena_core::engine::Engine;
use vena_core::inference::{Inference, OpenAiClient};
use vena_core::pkg;
use vena_core::store::Store;
use vena_core::GateMode;

#[derive(Parser)]
#[command(name = "vena-eval", about = "Phase-1 spoiler-safety eval")]
struct Cli {
    /// The .vena package to evaluate.
    #[arg(long)]
    vena: PathBuf,
    /// Interview set (JSONL, Appendix C).
    #[arg(long)]
    interviews: PathBuf,
    /// Spoiler-gate mode.
    #[arg(long, default_value = "standard")]
    mode: String,
    /// Write the verdict block to this file (e.g. EVAL.md fragment).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Export the exact gated prompts (stages 1–2) to this JSONL and exit. A human
    /// or external LLM answers them; score with --replies.
    #[arg(long)]
    export_prompts: Option<PathBuf>,
    /// Score pre-generated replies (JSONL {idx, reply, repair_reply?}) through
    /// stages 4–5 — a full generative eval with an out-of-process model.
    #[arg(long)]
    replies: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct Interview {
    #[serde(default)]
    character: Option<String>,
    reader_chapter: i64,
    question: String,
    #[serde(default)]
    forbidden_topics: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let profile = Store::in_memory()?;
    let sid = pkg::import_vena(&profile, &cli.vena).context("importing .vena for eval")?;
    let book = profile.get_book(sid)?;

    let interviews: Vec<Interview> = std::fs::read_to_string(&cli.interviews)?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()
        .context("parsing interviews")?;

    let backend = vena_core::inference::backend_from_env();
    let mode = GateMode::parse(&cli.mode);

    println!(
        "VENA EVAL · {} · {} interviews · gate {:?}",
        book.title,
        interviews.len(),
        mode
    );
    println!("{}", "=".repeat(64));

    // Phase 1 of the out-of-process flow: dump the exact gated prompts and exit.
    if let Some(path) = cli.export_prompts {
        export_prompts(&profile, sid, &interviews, &path)?;
        println!(
            "PROMPTS   exported to {} — answer them, then re-run with --replies",
            path.display()
        );
        return Ok(());
    }

    let report = if let Some(replies_path) = cli.replies {
        println!("BACKEND   replies file (out-of-process model / human-in-the-loop)\n");
        let backend = RepliesBackend::load(&replies_path)?;
        run_generative(&profile, sid, &interviews, Box::new(backend), mode)?
    } else {
        match backend {
            Some((label, b)) => {
                println!("BACKEND   {label} (generative eval)\n");
                run_generative(&profile, sid, &interviews, b, mode)?
            }
            None => {
                println!("BACKEND   none — running DETERMINISTIC gate-containment audit");
                println!(
                    "          (set ANTHROPIC_API_KEY or VENA_BASE_URL for the generative eval)\n"
                );
                run_gate_audit(&profile, sid, &interviews)?
            }
        }
    };

    let block = report.render(&book.title, report.generative);
    println!("{block}");
    if let Some(out) = cli.out {
        let mut f = std::fs::File::create(out)?;
        f.write_all(block.as_bytes())?;
    }
    Ok(())
}

struct EvalReport {
    total: usize,
    leaks: usize,
    consistent: usize,
    redacted: usize,
    latencies_ms: Vec<u128>,
    /// Gate-stage-only latency (µs) — the design's "AVG GATE 0.41S" field.
    gate_latencies_us: Vec<u128>,
    generative: bool,
    leak_examples: Vec<String>,
    /// Per-kind leak breakdown (future_event / unmet_character / tone_implies_ending / other).
    by_kind: std::collections::BTreeMap<String, usize>,
}

impl EvalReport {
    fn blocked(&self) -> usize {
        self.total.saturating_sub(self.leaks)
    }
    fn avg_gate_ms(&self) -> f64 {
        if self.gate_latencies_us.is_empty() {
            0.0
        } else {
            self.gate_latencies_us.iter().sum::<u128>() as f64
                / self.gate_latencies_us.len() as f64
                / 1000.0
        }
    }
}

impl EvalReport {
    fn leak_pct(&self) -> f64 {
        pct(self.leaks, self.total)
    }
    fn consistency_pct(&self) -> f64 {
        pct(self.consistent, self.total)
    }
    fn redaction_pct(&self) -> f64 {
        pct(self.redacted, self.total)
    }
    fn p(&self, q: f64) -> u128 {
        if self.latencies_ms.is_empty() {
            return 0;
        }
        let mut v = self.latencies_ms.clone();
        v.sort_unstable();
        let idx = ((v.len() as f64 - 1.0) * q).round() as usize;
        v[idx]
    }

    fn verdict(&self) -> (&'static str, String) {
        if !self.generative {
            // Deterministic containment: the only pass/fail is "did any forbidden
            // fact reach the model context?" 0 leaks = the ledger approach holds.
            return if self.leaks == 0 {
                (
                    "GO (containment)",
                    "The gate structurally contained every future fact and unmet \
                     character across all probes. Generative consistency requires a \
                     configured backend — see the run note below."
                        .into(),
                )
            } else {
                (
                    "FAIL (containment)",
                    format!("{} probe(s) exposed forbidden content in the gated context — the gate has a hole.", self.leaks),
                )
            };
        }
        // §11.6 thresholds.
        let leak = self.leak_pct();
        let cons = self.consistency_pct();
        if leak <= 10.0 && cons >= 75.0 {
            (
                "GO",
                "leak ≤ 10% AND consistency ≥ 75% — nothing changes.".into(),
            )
        } else if cons >= 60.0 {
            (
                "PIVOT",
                "consistency 60–75% — set Cloud Relay as the default chat mode and label local 'experimental'.".into(),
            )
        } else {
            (
                "KILL",
                "consistency < 60% — the ledger approach did not hold on this backend.".into(),
            )
        }
    }

    fn render(&self, title: &str, generative: bool) -> String {
        let (verdict, why) = self.verdict();
        let mut s = String::new();
        s.push_str(&format!("\n## Phase-1 eval — {title}\n\n"));
        s.push_str(&format!("- interviews: {}\n", self.total));
        // The design's Test-the-Gate result string: "N/N … BLOCKED ✓ · 0 LEAKS · AVG GATE X.XXS".
        s.push_str(&format!(
            "- {}/{} probes blocked {} · {} leaks · avg gate {:.2} ms\n",
            self.blocked(),
            self.total,
            if self.leaks == 0 { "✓" } else { "✗" },
            self.leaks,
            self.avg_gate_ms()
        ));
        s.push_str(&format!(
            "- leak rate: {:.1}%  ({} leaked)\n",
            self.leak_pct(),
            self.leaks
        ));
        if !self.by_kind.is_empty() {
            let breakdown: Vec<String> = self
                .by_kind
                .iter()
                .map(|(k, n)| format!("{k} {n}"))
                .collect();
            s.push_str(&format!("- leak taxonomy: {}\n", breakdown.join(" · ")));
        }
        if generative {
            s.push_str(&format!("- consistency: {:.1}%\n", self.consistency_pct()));
            s.push_str(&format!(
                "- redaction rate: {:.1}%{}\n",
                self.redaction_pct(),
                if self.redaction_pct() > 30.0 {
                    "  ⚠️ >30% — companion may be too boring (distinct failure)"
                } else {
                    ""
                }
            ));
            s.push_str(&format!(
                "- latency p50/p95: {} ms / {} ms\n",
                self.p(0.50),
                self.p(0.95)
            ));
        } else {
            s.push_str("- consistency: n/a (deterministic gate-audit; no generation)\n");
        }
        s.push_str(&format!("\n**VERDICT: {verdict}** — {why}\n"));
        if !self.leak_examples.is_empty() {
            s.push_str("\nLeak examples:\n");
            for e in self.leak_examples.iter().take(5) {
                s.push_str(&format!("  - {e}\n"));
            }
        }
        s
    }
}

fn run_generative(
    profile: &Store,
    sid: i64,
    interviews: &[Interview],
    backend: Box<dyn Inference>,
    mode: GateMode,
) -> Result<EvalReport> {
    let eng = Engine::new(backend).with_mode(mode);
    let mut leaks = 0;
    let mut consistent = 0;
    let mut redacted = 0;
    let mut latencies = Vec::new();
    let mut gate_us = Vec::new();
    let mut examples = Vec::new();
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();

    for iv in interviews {
        profile.set_progress(sid, iv.reader_chapter, 0)?;
        let cid = resolve_character(profile, sid, iv.character.as_deref())?;

        // Time the GATE stage via the stamps: gate→compose brackets stage 1(+1.5).
        let gate_start = std::cell::Cell::new(None);
        let gate_dur = std::cell::Cell::new(0u128);
        let mut on_stage = |st: &str| match st {
            "gate" => gate_start.set(Some(std::time::Instant::now())),
            "compose" => {
                if let Some(t) = gate_start.get() {
                    gate_dur.set(t.elapsed().as_micros());
                }
            }
            _ => {}
        };
        let t0 = std::time::Instant::now();
        let report = eng.companion_turn(profile, sid, cid, &iv.question, &mut on_stage)?;
        latencies.push(t0.elapsed().as_millis());
        gate_us.push(gate_dur.get());

        // A leak = a forbidden phrase survived in the reply, OR an unredacted
        // violation. Categorize by the engine's own leak taxonomy.
        let phrase_leak = report_leaked(&report.reply, &iv.forbidden_topics);
        let unredacted_violation = report
            .claims
            .iter()
            .any(|c| c.verdict == "violation" && !report.redacted);
        let leaked = phrase_leak || unredacted_violation;
        if leaked {
            leaks += 1;
            let kind = report
                .leaks_caught
                .first()
                .map(|k| format!("{k:?}"))
                .unwrap_or_else(|| "other".into())
                .to_lowercase();
            *by_kind.entry(kind).or_insert(0) += 1;
            examples.push(format!(
                "ch{} {}: “{}” → {}",
                iv.reader_chapter,
                iv.character.as_deref().unwrap_or("narrator"),
                iv.question,
                truncate(&report.reply, 80)
            ));
        }
        if report.redacted {
            redacted += 1;
        }
        // Consistency: a clean, engaged, non-empty reply (redacted-clean counts).
        if !leaked && !report.reply.trim().is_empty() {
            consistent += 1;
        }
    }

    Ok(EvalReport {
        total: interviews.len(),
        leaks,
        consistent,
        redacted,
        latencies_ms: latencies,
        gate_latencies_us: gate_us,
        generative: true,
        leak_examples: examples,
        by_kind,
    })
}

/// Deterministic containment audit: no model. For each interview, assemble exactly
/// what the gate would expose and prove no forbidden topic / future fact / unmet
/// character is present.
fn run_gate_audit(profile: &Store, sid: i64, interviews: &[Interview]) -> Result<EvalReport> {
    use vena_core::verify;
    let mut leaks = 0;
    let mut examples = Vec::new();
    let mut by_kind: std::collections::BTreeMap<String, usize> = Default::default();
    let mut gate_us = Vec::new();

    for iv in interviews {
        profile.set_progress(sid, iv.reader_chapter, 0)?;
        let cid = resolve_character(profile, sid, iv.character.as_deref())?;

        // Time the GATE stage exactly (this is the design's "AVG GATE" number).
        let t0 = std::time::Instant::now();
        let visible = profile.gated_facts(sid, iv.reader_chapter, cid, &iv.question, usize::MAX)?;
        let unmet = profile.unmet_character_names(sid)?;
        gate_us.push(t0.elapsed().as_micros());

        let context: String = visible
            .iter()
            .map(|f| f.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" | ");

        let mut kinds: Vec<&'static str> = Vec::new();
        let mut hit: Option<String> = None;

        // future_event — a forbidden (future) topic present in the gated context.
        for topic in &iv.forbidden_topics {
            if context.contains(&topic.to_lowercase()) {
                hit = Some(topic.clone());
                kinds.push("future_event");
                break;
            }
        }
        // future_event — a fact whose chapter outruns progress in the visible set.
        // (Structurally impossible while gated_facts filters chapter_seq ≤ progress;
        // kept as a regression guard that would fire if that filter ever regressed.)
        if visible.iter().any(|f| f.chapter_seq > iv.reader_chapter) {
            kinds.push("future_event");
        }
        // unmet_character — the gated context must not name a not-yet-met character,
        // and no visible fact may have an unmet character as its subject.
        let unmet_named = verify::unmet_characters(&context, unmet.iter().map(String::as_str));
        let unmet_subject = {
            let met_ids: std::collections::HashSet<i64> = profile
                .list_characters(sid)?
                .into_iter()
                .filter(|c| c.met)
                .map(|c| c.id)
                .collect();
            visible
                .iter()
                .any(|f| f.subject_char_id.is_some_and(|c| !met_ids.contains(&c)))
        };
        if !unmet_named.is_empty() || unmet_subject {
            kinds.push("unmet_character");
        }

        if !kinds.is_empty() {
            leaks += 1;
            for k in &kinds {
                *by_kind.entry(k.to_string()).or_insert(0) += 1;
            }
            examples.push(format!(
                "ch{} {}: {} (forbidden={:?}, unmet={:?})",
                iv.reader_chapter,
                iv.character.as_deref().unwrap_or("narrator"),
                kinds.join("+"),
                hit,
                unmet_named
            ));
        }
    }

    Ok(EvalReport {
        total: interviews.len(),
        leaks,
        consistent: 0,
        redacted: 0,
        latencies_ms: vec![],
        gate_latencies_us: gate_us,
        generative: false,
        leak_examples: examples,
        by_kind,
    })
}

fn resolve_character(profile: &Store, sid: i64, name: Option<&str>) -> Result<Option<i64>> {
    let Some(name) = name else { return Ok(None) };
    for c in profile.list_characters(sid)? {
        if c.name.eq_ignore_ascii_case(name)
            || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        {
            return Ok(Some(c.id));
        }
    }
    anyhow::bail!("interview names unknown character: {name}")
}

fn report_leaked(reply: &str, forbidden: &[String]) -> bool {
    let l = reply.to_lowercase();
    forbidden.iter().any(|t| l.contains(&t.to_lowercase()))
}


/// Export the EXACT gated prompts (stages 1–2 via the production `gate_and_assemble`)
/// so an out-of-process model (or a person) can answer them. One JSON per line:
/// {idx, character, reader_chapter, system, user}.
fn export_prompts(
    profile: &Store,
    sid: i64,
    interviews: &[Interview],
    path: &std::path::Path,
) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for (idx, iv) in interviews.iter().enumerate() {
        profile.set_progress(sid, iv.reader_chapter, 0)?;
        let cid = resolve_character(profile, sid, iv.character.as_deref())?;
        // Guard Character Fates deflects these before generation — no reply needed;
        // marked so the replies file stays aligned with actual backend calls.
        if vena_core::engine::is_fate_question(&iv.question) {
            writeln!(
                f,
                "{}",
                serde_json::json!({ "idx": idx, "deflected": true, "user": iv.question })
            )?;
            continue;
        }
        let gated = vena_core::engine::gate_and_assemble(profile, sid, cid, &iv.question)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let line = serde_json::json!({
            "idx": idx,
            "character": iv.character,
            "reader_chapter": iv.reader_chapter,
            "system": gated.system,
            "user": iv.question,
        });
        writeln!(f, "{line}")?;
    }
    Ok(())
}

/// Backend that replays out-of-process replies. The engine calls it once per turn
/// (draft) and possibly once more (repair) — repair calls are recognized by the
/// repair marker in the system prompt, so drafts stay aligned with interview order.
struct RepliesBackend {
    replies: std::sync::Mutex<ReplayState>,
}

struct ReplayState {
    items: Vec<(String, Option<String>)>, // (reply, repair_reply)
    pos: usize,
}

impl RepliesBackend {
    fn load(path: &std::path::Path) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct Row {
            idx: usize,
            reply: String,
            #[serde(default)]
            repair_reply: Option<String>,
        }
        let mut rows: Vec<Row> = std::fs::read_to_string(path)?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()
            .context("parsing replies JSONL")?;
        rows.sort_by_key(|r| r.idx);
        Ok(RepliesBackend {
            replies: std::sync::Mutex::new(ReplayState {
                items: rows
                    .into_iter()
                    .map(|r| (r.reply, r.repair_reply))
                    .collect(),
                pos: 0,
            }),
        })
    }
}

impl Inference for RepliesBackend {
    fn name(&self) -> String {
        "replies-file (out-of-process model)".into()
    }
    fn is_remote(&self) -> bool {
        // Treat as remote so the engine exercises the Cloud Relay-safe repair path.
        true
    }
    fn complete(
        &self,
        system: &str,
        _user: &str,
        _opts: &vena_core::inference::GenOptions,
    ) -> vena_core::Result<String> {
        let mut st = self.replies.lock().unwrap();
        let is_repair = system.contains("IMPORTANT: Your previous reply drifted");
        if is_repair {
            // Repair regen for the CURRENT interview (pos-1).
            let cur = st.pos.saturating_sub(1);
            let (reply, repair) = &st.items[cur];
            return Ok(repair.clone().unwrap_or_else(|| reply.clone()));
        }
        let i = st.pos.min(st.items.len().saturating_sub(1));
        st.pos += 1;
        Ok(st.items[i].0.clone())
    }
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 * 100.0 / d as f64
    }
}
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
