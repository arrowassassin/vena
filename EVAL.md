# EVAL.md — Phase-1 gate verdict (§9, §11.6)

The Phase-1 eval is a **steering checkpoint, not a stop** (§11.5). It runs when the
kernel/forge/eval segments complete (segment 3). Verdict recorded here.

- **GO** — leak ≤ 10% AND consistency ≥ 75% (baselines: ~47% zero-shot 7B, ~52% naive RAG),
  ≤ 10 s/turn on a 16 GB laptop. Nothing changes.
- **PIVOT** — consistency 60–75% → set **Cloud Relay** as the default chat mode, label local
  "experimental," continue.
- **KILL** — < 60% on both local AND api backends.

Also tracked: redaction rate (> 30% = "companion too boring," a distinct failure; redacted
replies count as clean but are reported).

_Verdict pending until segment 3._
