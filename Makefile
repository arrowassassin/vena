# Vena — common tasks. The eval targets are the Phase-1 gate (§11.5/§11.6).
.PHONY: build test eval eval-local eval-tiers fmt clippy ui

build:
	cargo build --workspace

test:
	cargo test --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace

ui:
	node ui-dc/build.mjs

# Deterministic gate-containment audit (no model needed) — runs anywhere.
eval:
	cargo run -q -p vena-eval -- \
	  --vena data/packages/dracula.vena \
	  --interviews data/eval/dracula.jsonl \
	  --json data/eval/results/gate-audit.json

# REAL generative eval against a LOCAL model server (Ollama / llama-server).
# Usage:  ollama serve & ollama pull qwen3:8b && make eval-local MODEL=qwen3:8b
MODEL ?= qwen3:8b
eval-local:
	VENA_BASE_URL=http://localhost:11434/v1 VENA_MODEL=$(MODEL) \
	cargo run -q -p vena-eval -- \
	  --vena data/packages/dracula.vena \
	  --interviews data/eval/dracula.jsonl \
	  --tier "$(MODEL)" \
	  --out EVAL-local.md \
	  --json data/eval/results/$(MODEL).json

# A/B the three local tiers (each must be pulled in Ollama first), then aggregate
# leak%/consistency%/latency/verdict into a table. Edit TIERS to your pulled models.
TIERS ?= qwen3:4b qwen3:8b qwen3:14b
eval-tiers:
	@bash scripts/eval-tiers.sh $(TIERS)
	@python3 scripts/eval-report.py data/eval/results/*.json
