// Rebuild desktop.html / mobile.html from the canonical design + current patches.
import fs from "node:fs";
function build(src, out, patchRef) {
  let h = fs.readFileSync(`../docs/design/${src}`, "utf8");
  h = h.replace('<script src="./support.js"></script>',
    '<link rel="stylesheet" href="./fonts/fonts.css">\n<script src="./react.js"></script>\n<script src="./react-dom.js"></script>\n<script src="./vena-bridge.js"></script>\n<script src="./support.js"></script>');
  h = h.replace(/<link rel="preconnect"[^>]*>\s*/g, "").replace(/<link href="https:\/\/fonts.googleapis[^>]*>\s*/, "");
  const patch = fs.readFileSync(patchRef, "utf8");
  const i = h.indexOf("data-dc-script");
  const close = h.indexOf("</script>", i);
  h = h.slice(0, close) + "\n\n/* ===== VENA REAL-API PATCH (build-appended) ===== */\n" + patch + "\n" + h.slice(close);
  fs.writeFileSync(out, h);
  console.log("built", out, h.length);
}
build("Vena App.dc.html", "desktop.html", "patch-desktop.js");
build("Vena Mobile.dc.html", "mobile.html", "patch-mobile.js");
