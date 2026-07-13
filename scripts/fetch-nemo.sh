#!/usr/bin/env bash
# Build a REAL public-domain "Little Nemo in Slumberland" CBZ from Wikimedia Commons
# (Winsor McCay, 1905-1914 plates — US public domain) and drop it into data/packages/
# so Vena seeds it on next launch. Needs curl + zip (both ship with macOS).
set -euo pipefail
cd "$(dirname "$0")/.."
N="${1:-16}"   # number of plates
TMP=$(mktemp -d)
echo "── fetching up to $N Little Nemo plates from Wikimedia Commons…"
API='https://commons.wikimedia.org/w/api.php'
urls=$(curl -fsSL "$API?action=query&generator=categorymembers&gcmtitle=Category:Little_Nemo_in_Slumberland&gcmtype=file&gcmlimit=$N&prop=imageinfo&iiprop=url&iiurlwidth=1200&format=json" \
  | python3 -c "import json,sys
d=json.load(sys.stdin)
for p in (d.get('query',{}).get('pages',{}) or {}).values():
    for ii in p.get('imageinfo',[]):
        print(ii.get('thumburl') or ii['url'])")
i=0
while IFS= read -r u; do
  [ -z "$u" ] && continue
  i=$((i+1))
  ext="${u##*.}"; ext="${ext%%\?*}"
  curl -fsSL "$u" -o "$TMP/$(printf '%04d' $i).${ext:-jpg}" && echo "  page $i" || i=$((i-1))
done <<< "$urls"
[ "$i" -gt 0 ] || { echo "no plates fetched — check your network"; exit 1; }
OUT="data/packages/Little Nemo in Slumberland.cbz"
rm -f "$OUT"; (cd "$TMP" && zip -q -j "$OLDPWD/$OUT" ./*)
rm -rf "$TMP"
echo "── built '$OUT' ($i pages). Restart Vena — it seeds onto the shelf automatically."
