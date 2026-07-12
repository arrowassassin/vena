// Rebuild desktop.html / mobile.html from the canonical design + current patches.
import fs from "node:fs";

// The canonical desktop design file is truncated at exactly 256 KiB — the cut
// lands inside `renderVals()`, in the middle of the `wikiLead:` string of the
// `extra` object literal, and the closing `</script></body></html>` are gone.
// When we detect that (no `</script>` after the dc-script), we cut back to the
// last complete property, close the class with an epilogue that hands every
// renderVals local over to `_venaTail()` (defined in patch-desktop.js), and
// re-emit well-formed closing tags. The template (<x-dc> block) is complete in
// the source and is NEVER touched.
const TRUNCATION_EPILOGUE = `wikiLead: ''
    };
    /* design tail lost to the 256 KiB truncation — restored (and wired to the
       real backend) by Component.prototype._venaTail in patch-desktop.js */
    return this._venaTail ? Object.assign(extra, this._venaTail({
      st, ch, chRoman, pct, themeName, dark, met, unmet, charObj, last, go,
      navTabs, steps, theories, mkTicks, books, models, strictDescs, strictOpts,
      mkTgl, engineToggles, fsMap, fsOpts, visBooks, ab, compReady, compFresh,
      compForging, compEmpty, wikiData, wikiUnlockedB, curSec, seg, wq, wEntries,
      delB, advanceCh, whoObj, extra
    })) : extra;
  }
}
`;

function build(src, out, patchRef) {
  let h = fs.readFileSync(`../docs/design/${src}`, "utf8");
  h = h.replace('<script src="./support.js"></script>',
    '<link rel="stylesheet" href="./fonts/fonts.css">\n<script src="./react.js"></script>\n<script src="./react-dom.js"></script>\n<script src="./dc-shims.js"></script>\n<script src="./vena-bridge.js"></script>\n<script src="./support.js"></script>');
  h = h.replace(/<link rel="preconnect"[^>]*>\s*/g, "").replace(/<link href="https:\/\/fonts.googleapis[^>]*>\s*/, "");
  const patch = fs.readFileSync(patchRef, "utf8");
  const marker = "\n\n/* ===== VENA REAL-API PATCH (build-appended) ===== */\n";
  const i = h.indexOf("data-dc-script");
  const close = h.indexOf("</script>", i);
  if (close === -1) {
    // truncated design (see note above)
    const cut = h.lastIndexOf("wikiLead:");
    if (cut === -1) throw new Error(`${src}: truncated design but wikiLead seam not found`);
    h = h.slice(0, cut) + TRUNCATION_EPILOGUE + marker + patch + "\n</script>\n</body>\n</html>\n";
  } else {
    h = h.slice(0, close) + marker + patch + "\n" + h.slice(close);
  }
  fs.writeFileSync(out, h);
  console.log("built", out, h.length);
}
build("Vena App.dc.html", "desktop.html", "patch-desktop.js");
build("Vena Mobile.dc.html", "mobile.html", "patch-mobile.js");
