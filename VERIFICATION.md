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
