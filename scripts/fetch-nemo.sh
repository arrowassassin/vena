#!/usr/bin/env bash
# Build a REAL public-domain "Little Nemo in Slumberland" CBZ from Wikimedia Commons
# (Winsor McCay, 1905-1914 plates — US public domain) and drop it into data/packages/
# so Vena seeds it on next launch. Needs curl + zip (both ship with macOS).
#
# Usage: ./scripts/fetch-nemo.sh [N]   (N = number of plates, default 16)
set -euo pipefail
cd "$(dirname "$0")/.."

N="${1:-16}"
case "$N" in
  (*[!0-9]*|'') echo "note: '$N' isn't a number — using 16 plates"; N=16 ;;
esac

UA="vena-fetch-nemo/1.0 (+https://github.com/arrowassassin/vena)"
API='https://commons.wikimedia.org/w/api.php'
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fetch_urls() {
  # $1 = query params selecting a file-page generator
  curl -fsSL -A "$UA" "$API?$1&prop=imageinfo&iiprop=url&iiurlwidth=1200&format=json" \
    -o "$TMP/api.json" || return 1
  python3 - "$TMP/api.json" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(0)
for p in (d.get("query", {}).get("pages", {}) or {}).values():
    for ii in p.get("imageinfo", []):
        print(ii.get("thumburl") or ii.get("url", ""))
PY
}

echo "── fetching up to $N Little Nemo plates from Wikimedia Commons…"
urls=$(fetch_urls "action=query&generator=categorymembers&gcmtitle=Category:Little_Nemo_in_Slumberland&gcmtype=file&gcmlimit=$N" || true)
if [ -z "$urls" ]; then
  echo "   category empty/unavailable — falling back to a Commons file search"
  urls=$(fetch_urls "action=query&generator=search&gsrsearch=Little%20Nemo%20in%20Slumberland%20McCay&gsrnamespace=6&gsrlimit=$N" || true)
fi
if [ -z "$urls" ]; then
  echo "ERROR: Wikimedia Commons returned nothing." >&2
  echo "  raw API response (first 300 chars):" >&2
  head -c 300 "$TMP/api.json" 2>/dev/null >&2 || echo "  (no response — check your network)" >&2
  exit 1
fi

i=0
while IFS= read -r u; do
  [ -z "$u" ] && continue
  ext="${u##*.}"; ext="${ext%%\?*}"
  case "$(echo "$ext" | tr '[:upper:]' '[:lower:]')" in
    (jpg|jpeg|png|gif|webp) ;;
    (*) continue ;;   # skip pdf/djvu/tif entries
  esac
  n=$((i+1))
  if curl -fsSL -A "$UA" "$u" -o "$TMP/$(printf '%04d' "$n").$ext"; then
    i=$n; echo "  page $i"
  fi
  [ "$i" -ge "$N" ] && break
done <<< "$urls"

[ "$i" -gt 0 ] || { echo "ERROR: no plates downloaded — check your network."; exit 1; }
OUT="data/packages/Little Nemo in Slumberland.cbz"
rm -f "$OUT"
(cd "$TMP" && rm -f api.json && zip -q -j "$OLDPWD/$OUT" ./*)
echo "── built '$OUT' ($i pages). Restart Vena — it seeds onto the shelf automatically."
