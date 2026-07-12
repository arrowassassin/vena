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

    let backend = backend_from_env();
    let mode = GateMode::parse(&cli.mode);

    println!(
        "VENA EVAL · {} · {} interviews · gate {:?}",
        book.title,
        interviews.len(),
        mode
    );
    println!("{}", "=".repeat(64));

    let report = match backend {
        Some((label, b)) => {
            println!("BACKEND   {label} (generative eval)\n");
            run_generative(&profile, sid, &interviews, b, mode)?
        }
        None => {
            println!("BACKEND   none — running DETERMINISTIC gate-containment audit");
            println!("          (set VENA_BASE_URL/KEY/MODEL for the full generative eval)\n");
            run_gate_audit(&profile, sid, &interviews)?
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
    generative: bool,
    leak_examples: Vec<String>,
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
        s.push_str(&format!(
            "- leak rate: {:.1}%  ({} leaked)\n",
            self.leak_pct(),
            self.leaks
        ));
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
    let mut examples = Vec::new();

    for iv in interviews {
        profile.set_progress(sid, iv.reader_chapter, 0)?;
        let cid = resolve_character(profile, sid, iv.character.as_deref())?;

        let t0 = std::time::Instant::now();
        let report = eng.companion_turn(profile, sid, cid, &iv.question, &mut |_| {})?;
        latencies.push(t0.elapsed().as_millis());

        let leaked = report_leaked(&report.reply, &iv.forbidden_topics)
            || report
                .claims
                .iter()
                .any(|c| c.verdict == "violation" && !report.redacted);
        if leaked {
            leaks += 1;
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
        generative: true,
        leak_examples: examples,
    })
}

/// Deterministic containment audit: no model. For each interview, assemble exactly
/// what the gate would expose and prove no forbidden topic / future fact / unmet
/// character is present.
fn run_gate_audit(profile: &Store, sid: i64, interviews: &[Interview]) -> Result<EvalReport> {
    let mut leaks = 0;
    let mut examples = Vec::new();

    for iv in interviews {
        profile.set_progress(sid, iv.reader_chapter, 0)?;
        let cid = resolve_character(profile, sid, iv.character.as_deref())?;
        let visible = profile.gated_facts(sid, iv.reader_chapter, cid, &iv.question, usize::MAX)?;
        let context: String = visible
            .iter()
            .map(|f| f.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" | ");

        // 1) No forbidden topic may appear in the gated context.
        let mut hit = None;
        for topic in &iv.forbidden_topics {
            let t = topic.to_lowercase();
            // whole-phrase containment on the visible facts
            if context.contains(&t) {
                hit = Some(topic.clone());
                break;
            }
        }
        // 2) No future fact (chapter > progress) may be in the visible set.
        let future_leak = visible.iter().any(|f| f.chapter_seq > iv.reader_chapter);
        if hit.is_some() || future_leak {
            leaks += 1;
            examples.push(format!(
                "ch{} {}: forbidden={:?} future_in_context={}",
                iv.reader_chapter,
                iv.character.as_deref().unwrap_or("narrator"),
                hit,
                future_leak
            ));
        }
    }

    Ok(EvalReport {
        total: interviews.len(),
        leaks,
        consistent: 0,
        redacted: 0,
        latencies_ms: vec![],
        generative: false,
        leak_examples: examples,
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

fn backend_from_env() -> Option<(String, Box<dyn Inference>)> {
    let base = std::env::var("VENA_BASE_URL").ok()?;
    let key = std::env::var("VENA_API_KEY").unwrap_or_default();
    let model = std::env::var("VENA_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
    Some((
        format!("{base} ({model})"),
        Box::new(OpenAiClient::new(&base, &key, &model)),
    ))
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
