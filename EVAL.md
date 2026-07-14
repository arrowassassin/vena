# EVAL.md — Phase-1 gate verdict (§9, §11.6)

The Phase-1 eval is a **steering checkpoint, not a stop** (§11.5): run at segment 3; on GO
nothing changes; below GO thresholds, set **Cloud Relay** as the default chat mode, label
local "experimental," and continue. Thresholds (§11.6):

- **GO** — leak ≤ 10% AND consistency ≥ 75% (baselines ~47% zero-shot 7B, ~52% naive RAG),
  ≤ 10 s/turn on a 16 GB laptop.
- **PIVOT** — consistency 60–75% → Cloud Relay default, local "experimental."
- **KILL** — < 60% on both local AND api backends.
- Redaction > 30% = "companion too boring," a distinct failure (redacted counts as clean).

The harness is `crates/vena-eval` (kept forever as the regression suite; the same loop ships
as the in-app **Test the Gate — RUN 12 PROBES**). It has two modes:

- **generative** — with a backend configured (`VENA_BASE_URL` / `VENA_API_KEY` / `VENA_MODEL`,
  or a local OpenAI-compat server such as llama.cpp/ollama/LM Studio): real replies → real
  consistency %, leak %, latency p50/p95, redaction %.
- **gate-audit** — no backend: exercises the DETERMINISTIC containment guarantee. For every
  interview it proves no forbidden/future fact and no unmet character can reach the model's
  context. This is the property the whole architecture rests on (§2, §6).

## Run @ segment 3 (gate-audit — real Dracula package, 24 interviews)

```
VENA EVAL · Dracula · 24 interviews · gate Standard
- interviews: 24
- 24/24 probes blocked ✓ · 0 leaks · avg gate 1.85 ms
- leak rate: 0.0%  (0 leaked)
- consistency: n/a (deterministic gate-audit; no generation)
VERDICT: GO (containment)
```

The gate **structurally contained every future fact and every unmet character** across all
24 point-in-time interviews (ch 4/6/8/12, half narrator / half in-character; direct-future,
innocent-recall, theory-bait, and who-is questions per Appendix C). **0 leaks.** The audit
checks all three leak-taxonomy vectors it can decide without a model: `future_event`
(forbidden phrase / future fact in the gated context) and `unmet_character` (a not-yet-met
character named in the context, or a visible fact whose subject is unmet). `tone_implies_ending`
is LLM-judged and is exercised only in the generative run. The ledger approach holds: what the
model never sees, it cannot leak.

## Run @ segment 4 (generative — model-in-the-loop, 24 interviews)

The founder authorized a model-in-the-loop generative eval: `--export-prompts` dumps the
EXACT gated stage-1–2 prompts the production pipeline assembles (via `gate_and_assemble`);
a real frontier LLM (Claude, the build assistant, acting as the backend) answered every
non-deflected prompt in character from ONLY the gated facts; `--replies` scored those
answers through the real stage-4–5 verify/repair pipeline (guard-fates active — 2 of 24
interviews deflected pre-generation, counted per protocol).

```
VENA EVAL · Dracula · 24 interviews · gate Standard  (backend: replies-file / Claude)
- 24/24 probes blocked ✓ · 0 leaks · avg gate 3.37 ms
- leak rate: 0.0%   - consistency: 100.0%   - redaction rate: 0.0%
VERDICT: GO — leak ≤ 10% AND consistency ≥ 75%
```

Honest caveats: (a) the p50/p95 shown by a replay run measures pipeline overhead, not model
latency — the ≤10 s/turn budget applies to a live backend; (b) this GO is for a
**relay-class (frontier) backend**. The §9 full-GO clause is specified against the 8B LOCAL
tier, which this sandbox still cannot run — so the steer below stands until a local run.

## Generative consistency on the LOCAL tier — measurement pending a backend

The build environment could reach **no GGUF host** (Hugging Face blocked by the sandbox
network allowlist) and had **no API key**, so a live local/relay model could not be run here
to measure generative **consistency %**. Per the §11.5 rule ("below GO → steer, never stop"),
the conservative, spec-aligned decision when local-model quality is **unverified** is to take
the below-GO branch:

> **STEER: Cloud Relay is the DEFAULT chat mode; local-model chat is labelled
> "experimental (unmeasured)" until the generative eval is run.**

The Segment-4 app implements this steer: `get_ai_status` defaults `default_chat_mode` to
`cloud`, reports `local_experimental: true`, and onboarding (Segment 9) presents **Cloud Relay**
as the recommended mode with the "experimental" stamp on local until it is validated.

### To upgrade to a full GO (on a normal machine)

```bash
# with your own key (OpenRouter/Anthropic-compat/etc.) …
export VENA_BASE_URL=https://openrouter.ai/api  VENA_API_KEY=sk-…  VENA_MODEL=…
# …or a local server: export VENA_BASE_URL=http://localhost:11434  (ollama / LM Studio)
cargo run -p vena-eval -- --vena data/packages/dracula.vena --interviews data/eval/dracula.jsonl
```

If that run reports leak ≤ 10% AND consistency ≥ 75%, flip the default back to local (GO) by
setting `default_chat_mode = local` in Settings; the branding tiers (INK/QUILL/ARCHIVIST) and
everything else are unchanged — spoiler-safety lives in the ledger, so the model is a swap.

---

## Local-tier benchmarking (run on your hardware)

The sandbox has no GPU, so the LOCAL generative numbers must be produced on a real machine.
The harness + interview set + aggregation are ready so it's one command:

```bash
# 1. Serve a local model (any OpenAI-compatible server works):
ollama serve &
ollama pull qwen3:8b

# 2. Real generative eval for one tier → writes EVAL-local.md + a JSON result:
make eval-local MODEL=qwen3:8b

# 3. A/B all three tiers and print a comparison table:
ollama pull qwen3:4b && ollama pull qwen3:14b
make eval-tiers TIERS="qwen3:4b qwen3:8b qwen3:14b"
```

The interview set is now **64 point-in-time interviews** (`data/eval/dracula.jsonl`) spanning
reader positions ch. 4/6/8/10/12/14/16/18 and a mix of direct-future, innocent-recall,
theory-bait, and who-is questions (half narrator, half in-character) — enough for a real
verdict rather than a spot check. The deterministic gate-audit over this set: **64/64 probes
blocked, 0 leaks** (structural containment holds at scale).

**The steer is now device-correct.** Instead of hardcoding local chat "experimental," each
tier is validated per device: a clean in-app **RUN 12 PROBES** (0 leaks on a local backend)
promotes that tier via `set_local_validated`, and `get_ai_status.local_experimental` flips
off — so a tier that GO's on a 32 GB desktop but not a phone is handled correctly. `make
eval-tiers` records the same verdict from the CLI for maintainers.
