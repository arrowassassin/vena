#!/usr/bin/env bash
# Serve a downloaded voice tier locally so THE CAST can speak — 100% on-device.
#
# NOTE: since the embedded runtime landed, Vena speaks in-process — this script
# is only for power users who prefer an external server. It
# starts llama.cpp's llama-server (OpenAI-compatible) on the port Vena's local
# mode expects (11434), loading the newest .gguf from your profile's models
# dir — or a path you pass:
#
#   brew install llama.cpp        # once
#   ./scripts/serve-local.sh      # keep it running while you read
#   ./scripts/serve-local.sh /path/to/model.gguf
#
# (Already running Ollama or LM Studio? That works too — anything serving an
# OpenAI-compatible API on localhost:11434.)
set -euo pipefail

MODEL="${1:-}"
if [ -z "$MODEL" ]; then
  for dir in \
    "${VENA_DATA_DIR:-/nonexistent}/models" \
    "${TMPDIR:-/tmp}/vena-dev/models" \
    "/tmp/vena-dev/models" \
    "$HOME/Library/Application Support/vena/models" \
    "$HOME/Library/Application Support/com.vena.app/models" \
    "$HOME/.local/share/vena/models"; do
    [ -d "$dir" ] || continue
    cand=$(ls -t "$dir"/*.gguf 2>/dev/null | head -1 || true)
    if [ -n "$cand" ]; then MODEL="$cand"; break; fi
  done
fi
if [ -z "$MODEL" ] || [ ! -f "$MODEL" ]; then
  echo "ERROR: no downloaded .gguf found — download a tier in Settings first," >&2
  echo "       or pass the file: ./scripts/serve-local.sh /path/to/model.gguf" >&2
  exit 1
fi
if ! command -v llama-server >/dev/null 2>&1; then
  echo "ERROR: llama-server not found — install llama.cpp first:" >&2
  echo "  brew install llama.cpp" >&2
  exit 1
fi

echo "── serving $(basename "$MODEL") at http://localhost:11434 (Vena's local engine port)"
echo "   keep this terminal open while you read — Ctrl-C stops the engine"
exec llama-server -m "$MODEL" --port 11434 -c 8192
