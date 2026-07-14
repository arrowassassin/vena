# Vena — System Design & Architecture (v2.0)
Canonical build spec. Saved from the founder's v2.0 handoff (July 11 2026).
Key v2.0 deltas over v1.9: story graph (chapter-stamped `edge` table derived from
the ledger; graph-guided retrieval stage 1.5, §6b); finalized model policy
(always-on defaults, binary LOCAL/Cloud-Relay choice, tiers INK·3B=Qwen3-4B ~2.3GB /
QUILL·7B=Qwen3-8B ~4.8GB / ARCHIVIST·13B=Qwen3-14B ~8.5GB; embedder
multilingual-e5-small ~120MB; Paint Engine EASEL·XL=SDXL ~6.9GB / SKETCH·1.5 ~1.0GB);
keychain key storage; IPC additions set_image_config / test_relay / list_relay_models.
See the conversation handoff for the full text; this repo implements it segment by segment.
