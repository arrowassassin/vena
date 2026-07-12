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

## Generative consistency — measurement pending a backend

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
