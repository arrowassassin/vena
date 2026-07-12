#!/usr/bin/env bash
# Run the generative eval once per local tier (Ollama), writing one JSON per tier.
# Each model must already be pulled: `ollama pull qwen3:8b`.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p data/eval/results
for model in "$@"; do
  echo "── eval tier: $model ─────────────────────────────"
  safe="${model//[:\/]/_}"
  VENA_BASE_URL="${VENA_BASE_URL:-http://localhost:11434/v1}" VENA_MODEL="$model" \
    cargo run -q -p vena-eval -- \
      --vena data/packages/dracula.vena \
      --interviews data/eval/dracula.jsonl \
      --tier "$model" \
      --json "data/eval/results/${safe}.json" || echo "  (tier $model failed — is it pulled and served?)"
done
