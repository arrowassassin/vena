#!/usr/bin/env bash
# Forge a shelf of public-domain classics into pre-forged .vena packages so the
# store's FEATURED section has real, chat-ready books beyond Dracula.
#
# Every package is REAL: the EPUB comes from Project Gutenberg and the ledger is
# extracted chapter-by-chapter by YOUR configured engine — so this needs the
# same relay you use for chat:
#
#   export VENA_BASE_URL="https://openrouter.ai/api"   # or your provider
#   export VENA_API_KEY="sk-…"
#   export VENA_MODEL="anthropic/claude-sonnet-4.5"    # any capable model
#   ./scripts/forge-classics.sh            # all classics
#   ./scripts/forge-classics.sh 1342 84    # just these Gutenberg ids
#
# Packages land in data/packages/ and appear in FEATURED on next launch.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${VENA_BASE_URL:-}" ] || [ -z "${VENA_API_KEY:-}" ] || [ -z "${VENA_MODEL:-}" ]; then
  echo "ERROR: set VENA_BASE_URL, VENA_API_KEY and VENA_MODEL first — the ledger" >&2
  echo "       is forged by a real model, chapter by chapter (see header)." >&2
  exit 1
fi

# id | slug | display title (title/author come from the EPUB itself at import)
CLASSICS="
1342|pride-and-prejudice|Pride and Prejudice
84|frankenstein|Frankenstein
1661|sherlock-holmes|The Adventures of Sherlock Holmes
11|alice-in-wonderland|Alice's Adventures in Wonderland
2701|moby-dick|Moby-Dick
"

want="${*:-}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
UA="vena-forge-classics/1.0 (+https://github.com/arrowassassin/vena)"
built=0

echo "$CLASSICS" | while IFS='|' read -r id slug title; do
  [ -z "$id" ] && continue
  if [ -n "$want" ] && ! echo " $want " | grep -q " $id "; then continue; fi
  out="data/packages/${slug}.vena"
  if [ -f "$out" ]; then echo "── $title — already built ($out), skipping"; continue; fi
  echo "── $title (Gutenberg #$id)"
  epub="$TMP/${slug}.epub"
  curl -fSL -A "$UA" "https://www.gutenberg.org/ebooks/${id}.epub.noimages" -o "$epub" \
    || { echo "   download failed — skipping"; continue; }
  cargo run --release -q --bin vena-forge -- forge \
    --input "$epub" --out "$out" --slug "$slug" --source "gutenberg:${id}" \
    || { echo "   forge failed — skipping"; rm -f "$out"; continue; }
  built=$((built+1))
  echo "   ✓ $out"
done

echo "── done. Restart Vena — new packages appear in FEATURED (and import in one tap)."
