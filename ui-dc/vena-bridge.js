// VENA bridge — the real §11.2 surface for the design runtime. One global:
//   VENA.call(cmd, args) -> Promise (Tauri invoke, or devserver HTTP)
//   VENA.onEvent(fn) — forge:progress / companion:stage / model:progress / …
(function () {
  const tauri = window.__TAURI__;
  async function call(cmd, args = {}) {
    if (tauri) {
      try { return await tauri.core.invoke(cmd, args); }
      catch (e) { throw Object.assign(new Error(e?.message || String(e)), { code: e?.code || "Other" }); }
    }
    const res = await fetch(`/api/${cmd}`, {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(args),
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) throw Object.assign(new Error(body.message || "request failed"), { code: body.code || "Other" });
    return body;
  }
  const listeners = new Set();
  function onEvent(fn) { listeners.add(fn); return () => listeners.delete(fn); }
  if (tauri && tauri.event) {
    for (const name of ["forge:progress","forge:done","companion:stage","companion:done","model:progress","store:progress","image:progress","image:done"]) {
      tauri.event.listen(name, (e) => listeners.forEach((fn) => fn({ name, payload: e.payload })));
    }
  } else {
    (async function poll() {
      try {
        const text = await (await fetch("/api/events")).text();
        for (const block of text.split("\n\n")) {
          const ev = /event: (.+)/.exec(block)?.[1];
          const data = /data: (.+)/.exec(block)?.[1];
          if (ev && data) { const payload = JSON.parse(data); listeners.forEach((fn) => fn({ name: ev, payload })); }
        }
      } catch {}
      setTimeout(poll, 400);
    })();
  }
  window.VENA = { call, onEvent };
})();
