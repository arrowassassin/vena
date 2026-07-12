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

<!-- tester findings appended below -->
