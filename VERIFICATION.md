# VERIFICATION.md — dual-tester log (§11.5)

The build runs end-to-end, one segment at a time. After **every** segment, two
independent subagent reviewers audit it before the next begins:

- **UX TESTER** — audits against the canonical design files (`docs/design/Vena App.dc.html`,
  `docs/design/Vena Mobile.dc.html`): visual fidelity, interaction states, vocabulary
  lock, responsive desktop-vs-5-tab-mobile.
- **PM TESTER** — audits against `system-design.md` (uploaded as the task brief): requirement
  coverage, invariants (canon immutability; gate-before-generate; Cloud Relay never sends
  ungated content; network allowlist; spoiler consent for full wiki), IPC conformance, scope
  discipline.

Every **severity-high** finding is fixed before the next segment. A segment is **DONE** only
when both testers pass it.

**Segment order:** (1) kernel · (2) forge + Dracula · (3) eval + checkpoint · (4) Tauri shell +
IPC · (5) Library + Reader · (6) Companion · (7) Store + import · (8) Archive · (9) Settings +
onboarding + gate features · (10) dictionary/translate/serial/manga · (11) final integration.

---

## Segment 1 — Kernel (`vena-core`)

**Delivered:** `schema.sql` (Appendix A + `chat_memory`, `archived`), `store.rs` (the gate),
`model.rs` (DTOs + leak taxonomy + model-tier table), `inference.rs` (trait + `ScriptedInference`
+ `OpenAiClient`/Cloud Relay), `verify.rs` (claim extraction, lexical matching, leak taxonomy),
`engine.rs` (5-stage pipeline, gate modes, guard-fates, probes, recap, theory resolution),
`wiki.rs` (sealed/unsealed Archive + consent). 14 unit tests, all green.

### UX TESTER — VERDICT: PASS
Vocabulary lock, MODEL_TIERS (INK·3B/QUILL·7B/ARCHIVIST·13B, 1.9/4.6/7.9 GB, 16 GB RAM for
ARCHIVIST), gate modes + thresholds (0.5/0.6/0.7), leak taxonomy, and GATE→COMPOSE→VERIFY
stamps all match the canonical design. Note carried forward: the UI must render the
`TurnReport.redacted` flag as the product word **"INKED OUT"** (design §6 sanctions "redact"
as the internal term).

### PM TESTER — VERDICT: FAIL → resolved → PASS
- **HIGH — Cloud Relay invariant breach (FIXED).** The repair stage embedded verbatim
  forbidden/future fact text (`f.text`) into the prompt passed to `backend.complete()`, which
  may be the remote `OpenAiClient`; `is_remote()` existed but was never called. Ungated future
  spoilers could be POSTed off-device on any STANDARD/RELAXED first-draft violation.
  **Fix:** `engine::repair` now branches on `self.backend.is_remote()` — remote backends get a
  neutral "do not reveal any future outcome" instruction with **no** fact text; only a local
  (on-device) backend ever sees the specific forbidden topics. New regression test
  `repair_prompt_is_neutral_for_remote_backend` asserts the remote repair prompt contains no
  forbidden text. (15 tests green.)
- **LOW (accepted, doc-sanctioned):** lexical theory resolver only ever writes "confirmed"
  (never "busted") — the §6b LLM judgment call is the upgrade path; acceptable for Phase-1.
- **LOW (deferred, tracked):** `chat_memory` gated read/write is a Companion (seg 6) concern;
  primitive table is in place. Re-seal/reopen auto-wiring on rewind belongs to the Tauri layer
  (seg 4); the store primitives + rewind signal exist.
- Schema conformance, canon immutability, gate-before-generate, per-character scoping, wiki
  consent, 5-stage engine, scope discipline: all PASS.

**Segment 1 DONE** (both testers pass after the HIGH fix).

---

## Segment 2 — Forge + real Dracula package

**Delivered:** `vena-forge` (real EPUB + Gutenberg-txt importer, §F5c format detection, scene
segmentation, Appendix-B model ledger path + maintainer-curated path, story-graph edge
derivation), `vena-core::pkg` (.vena export/import with FK remapping), and the REAL
public-domain Dracula forged to `data/packages/dracula.vena` (27 ch · 440 scenes · 10 chars ·
40 facts · 16 edges · 81% coverage). End-to-end roundtrip test on the real book.

### UX TESTER — VERDICT: FAIL → resolved → PASS
- **MED — SHA label mismatch (FIXED).** The CLI printed a headline "PACKAGE SHA" = the zip-file
  hash, but the app persists/shows the *content* SHA — a maintainer cross-check would disagree.
  **Fix:** CLI now prints **LEDGER SHA** (the persisted content identity shown in-app,
  uppercased to match design status rows) and **ARCHIVE SHA** (file integrity, clearly labelled).
- LOW (evidence units chars/doc vs "words/page"; SHA case) — cosmetic; the UI formats the raw
  hex field itself. Profile taxonomy + status fields all PASS.

### PM TESTER — VERDICT: PASS (3 defects, all resolved)
- Real full Dracula (not a stub), real forge pipeline (model path wired to real HTTP inference;
  curated path a legitimate §7 prebuilt), .vena format §11.3, format detection §F5c, story graph
  §6b, canon immutability, Cloud-Relay-vs-forge separation: all PASS.
- **D1 MED — empty vena-eval broke root build (RESOLVED):** the eval crate is now a real member
  (segment 3).
- **D2 MED — derived edges didn't cite source fact (FIXED):** edge derivation moved into the
  forge (after facts exist); each derived `knows` edge now stores `source_fact_id`.
- **D3 LOW — package shipped a default `progress` row (FIXED):** the forge now clears the
  `progress` table so the .vena ships with user tables empty (§11.3).

**Segment 2 DONE** (both testers pass after fixes).

---

## Segment 3 — Eval harness + Phase-1 checkpoint

**Delivered:** `vena-eval` — real interview runner (Appendix C JSONL), two modes (generative
against a configured backend; deterministic gate-audit with none), reporting leak %,
consistency %, latency p50/p95, redaction %, and the GO/PIVOT/KILL verdict. Interview set
`data/eval/dracula.jsonl` (24 point-in-time interviews). Verdict recorded in **EVAL.md**.

**Checkpoint result:** gate-audit over the real Dracula package → **0 leaks / 24** →
**GO (containment)**. Generative consistency unmeasurable in-sandbox (no reachable GGUF host,
no API key), so per the §11.5 "below-GO → steer" rule the conservative choice is taken:
**Cloud Relay is the default chat mode; local chat is labelled "experimental (unmeasured)"**
until the generative eval is run (documented in EVAL.md). The run continues.

_(UX/PM tester entries for segment 3 appended below.)_
