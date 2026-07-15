// ============================================================================
// VENA mobile patch — wires the canonical mobile design to the REAL engine
// via window.VENA (vena-bridge.js). Appended inside the dc-script AFTER the
// Component class; patches Component.prototype only — the template HTML and
// the class body above are verbatim canonical design.
//
// Real:   shelf (list_books), cast (list_characters, met→silhouette),
//         chat (companion_turn + companion:stage phases), recap (get_recap),
//         probes (run_probes), theories (add/list_theories), wiki (spoiler
//         consent + get_wiki_index/get_wiki_page), reader text (get_episode),
//         progress (set_progress), model download (download_local_model +
//         model:progress), relay (set_api_config/test_relay/list_relay_models/
//         set_chat_mode), settings (get_settings/set_setting), leak reports
//         (report_leak), burn (delete_book), import (import_book).
// Demo-honest (unchanged local design features): offline dictionary entries,
//         manga demo pages, language-pack install simulation, what-if panel.
// Honest toasts (absent capability, never faked): rolling translate,
//         selection translate.
// ============================================================================
(function () {
  if (typeof Component === "undefined" || typeof window === "undefined" || !window.VENA) return;
  var P = Component.prototype;
  var V = window.VENA;

  var LEGACY_ART = [
    [/van helsing/i, "vh"], [/dracula/i, "dr"], [/mina/i, "mina"],
    [/lucy/i, "lucy"], [/seward/i, "js"], [/harker|jonathan/i, "jh"],
    [/holmwood|arthur/i, "ah"], [/quincey|morris/i, "qm"],
  ];
  var LANG_CODE = { French: "fr", Japanese: "ja", German: "de", Spanish: "es" };
  var LANG_NAME = { fr: "French", ja: "Japanese", de: "German", es: "Spanish" };
  var MODEL_DESCS = {
    ink: "Fast and sure-footed. Replies in ~4s on this phone.",
    quill: "Richer, more period-true voices. Twice the wait.",
    arch: "The scholar. Too heavy for this phone.",
  };

  P._up = function (e) {
    var m = (e && e.message) ? String(e.message) : String(e || "request failed");
    this._toast(m.toUpperCase());
  };
  P._bookId = function () {
    var b = (this.bookDefs || []).find(function (x) { return x.id === this.state.book; }, this) || this.bookDefs[0];
    return b && b.nid != null ? b.nid : null;
  };
  P._activeBookDef = function () {
    var st = this.state;
    var vis = (this.bookDefs || []).filter(function (b) { return !st.deleted[b.id]; });
    return vis.find(function (b) { return b.id === st.book; }) || vis[0] || null;
  };
  P._chNow = function () {
    var ab = this._activeBookDef();
    var cap = ab ? ab.total : 27;
    var raw = this.state.chOverride != null ? this.state.chOverride : (this.props.chapter != null ? this.props.chapter : 12);
    return Math.max(1, Math.min(cap, raw));
  };

  // ------------------------------------------------------------------ mount
  var origMount = P.componentDidMount;
  P.componentDidMount = function () {
    origMount.call(this);
    // The demo class fakes a forge in progress; the real forge state comes
    // from list_books + forge:progress events.
    if (this._forge) { clearInterval(this._forge); this._forge = null; }
    var self = this;
    this._legacyIds = {};
    // demo-state honesty: no fake serial countdown (no pacing engine exists),
    // no prefilled margin note from a chapter the reader never opened
    this.setState({ tglSerial: false, notes: [] });
    this._offVena = V.onEvent(function (ev) { self._onVenaEvent(ev); });
    // device reading prefs + reduced-motion, same as desktop
    try {
      this._readerScale = parseFloat(localStorage.getItem("vena_reader_scale")) || 1;
      this._readerLine = localStorage.getItem("vena_reader_line") || "";
    } catch (_) { this._readerScale = 1; this._readerLine = ""; }
    if (!document.getElementById("vena-a11y-css")) {
      var sty = document.createElement("style");
      sty.id = "vena-a11y-css";
      sty.textContent = "@media (prefers-reduced-motion: reduce){*,*::before,*::after{animation-duration:.001s !important;animation-iteration-count:1 !important;transition-duration:.001s !important;scroll-behavior:auto !important}}";
      document.head.appendChild(sty);
    }
    this._hydrate();
  };

  var origUnmount = P.componentWillUnmount;
  P.componentWillUnmount = function () {
    origUnmount.call(this);
    if (this._offVena) { this._offVena(); this._offVena = null; }
  };

  P._onVenaEvent = function (ev) {
    var name = ev.name, p = ev.payload || {};
    if (name === "companion:stage") {
      if (this._busy && p.turnId === this._turnId && !this._iv) {
        var phase = ({ gate: "gate", compose: "gen", verify: "verify", repair: "repair" })[p.stage];
        if (phase) this.setState({ phase: phase });
      }
    } else if (name === "model:progress") {
      if (p.kind === "paint") {
        var pd = this.state.paintDl || {};
        if (pd.status === "downloading") {
          this.setState({ paintDl: { status: "downloading", pct: Math.max(pd.pct || 0, p.pct | 0), tier: pd.tier || p.tier } });
        }
        return;
      }
      var dl = this.state.dl || {};
      if (dl.status === "downloading" && (!dl.tier || dl.tier === p.tier)) {
        this.setState({ dl: { status: "downloading", pct: Math.max(dl.pct || 0, p.pct | 0), tier: dl.tier || p.tier } });
      }
    } else if (name === "forge:progress") {
      // forgedThrough = highest chapter whose facts are committed (streaming forge)
      var fupd = {};
      if (p.pct != null) fupd.forgePct = p.pct | 0;
      if (p.forgedThrough != null) {
        fupd.forgedThrough = p.forgedThrough | 0;
        if (p.bookId != null) (this._forgeState || (this._forgeState = {}))[p.bookId] = { forgedThrough: p.forgedThrough | 0 };
      }
      this.setState(fupd);
    } else if (name === "forge:done") {
      this._hydrate();
    }
  };

  // ---------------------------------------------------------------- hydrate
  P._hydrate = function () {
    var self = this;
    V.call("list_books").then(function (books) {
      self._bySlug = {};
      var demoCovers = {};
      // keep the design's hand-made covers where slugs match the demo shelf
      (self._demoBookDefs || (self._demoBookDefs = self.bookDefs)).forEach(function (b) { demoCovers[b.id] = b.cover; });
      var fallbackCover = "radial-gradient(circle at 50% 30%, rgba(240,198,106,.4) 0 12%, transparent 32%), linear-gradient(165deg,#1c3040,#0b131d)";
      self.bookDefs = books.map(function (b) {
        self._bySlug[b.slug] = b;
        return {
          id: b.slug, nid: b.id,
          title: String(b.title || b.slug).toUpperCase(),
          author: String(b.author || "UNKNOWN").toUpperCase(),
          cover: demoCovers[b.slug] || fallbackCover,
          status: b.forge_state === "sealed" ? "forged" : b.forge_state === "forging" ? "forging" : "fresh",
          total: b.episode_count || 0,
          stats: (b.fact_count || 0).toLocaleString() + " FACTS · COVERAGE " +
            Math.round((b.ledger_coverage || 0) * 100) + "% · SHA " +
            String(b.package_sha || "????").slice(0, 4).toUpperCase() + "…" +
            String(b.package_sha || "??").slice(-2).toUpperCase(),
        };
      });
      // open where you left off: last-read book wins over book #1
      var last = null;
      try { last = localStorage.getItem("vena_last_book"); } catch (_) {}
      var cur = self.bookDefs.find(function (b) { return b.id === self.state.book; })
        || self.bookDefs.find(function (b) { return b.id === last; })
        || self.bookDefs[0] || null;
      self._navRestored = true;
      var meta = cur ? self._bySlug[cur.id] : null;
      var upd = { deleted: {} };
      if (cur) upd.book = cur.id;
      if (meta) {
        upd.chOverride = Math.max(1, Math.min(meta.episode_count || 1, meta.progress_episode || 1));
        upd.forgePct = meta.forge_state === "sealed" ? 100 : self.state.forgePct;
        upd.forgeFacts = meta.fact_count || 0;
      }
      self.setState(upd);
      if (!cur) return;
      self._loadCharacters();
      self._loadTheories();
      self._loadEpisode(upd.chOverride || 1);
      self._loadTerms();
    }).catch(function (e) { self._up(e); });

    V.call("get_settings").then(function (s) {
      self._settings = s;
      var brandToId = {};
      (s.tiers || []).forEach(function (t) { brandToId[t.brand] = t.id; });
      var installedId = s.local_ready ? brandToId[s.local_model] : null;
      if (!s.onboarded && self.state.onboardStep == null) self.setState({ onboardStep: 0 });
      var upd = {
        strict: s.gate_mode || "standard",
        tglStamps: !!s.show_engine_stamps,
        tglFates: !!s.guard_fates,
        tglReseal: !!s.reseal_on_reread,
        targetLang: LANG_CODE[s.target_language] || "fr",
      };
      if (installedId) upd.model = installedId;
      if (s.cloud_base_url) upd.relayUrl = s.cloud_base_url;
      if (s.cloud_model) upd.relayModel = s.cloud_model;
      self.setState(upd);
    }).catch(function () {});
    V.call("get_image_status").then(function (is) { self._imageStatus = is; self.forceUpdate(); }).catch(function () {});
    V.call("paint_tiers").then(function (ts) { self._paintTiers = ts || []; self.forceUpdate(); }).catch(function () {});
    V.call("relay_presets").then(function (ps) { self._relayPresets = ps || []; self.forceUpdate(); }).catch(function () {});
    if (!this._autoPainted) {
      this._autoPainted = true;
      V.call("auto_paint").then(function (r) {
        if (r && r.error && !r.covers && !r.portraits) { self._toast("PAINT ENGINE FAILED — " + String(r.error).slice(0, 120)); return; }
        if (r && (r.covers || r.portraits)) self._toast("PAINTED " + r.covers + " COVERS · " + r.portraits + " PORTRAITS — STAMPED ✦ AI");
      }).catch(function () {});
    }
    this._refreshAi();
  };

  P._refreshAi = function () {
    var self = this;
    return V.call("get_ai_status").then(function (ai) {
      self._ai = ai;
      self.setState({ relay: ai.mode === "cloud" });
    }).catch(function () {});
  };

  P._loadCharacters = function () {
    var self = this, bookId = this._bookId();
    if (bookId == null) return;
    var ch = this._chNow();
    return V.call("list_characters", { bookId: bookId }).then(function (chars) {
      if (!chars || !chars.length) return;
      self._legacyIds = {};
      var cast = chars.map(function (c) {
        var legacy = null;
        for (var i = 0; i < LEGACY_ART.length; i++) {
          if (LEGACY_ART[i][0].test(c.name)) { legacy = LEGACY_ART[i][1]; break; }
        }
        var id = String(c.id);
        if (legacy) {
          self._legacyIds[legacy] = id;
          if (self.artMap[legacy]) self.artMap[id] = self.artMap[legacy];
          if (self.bustMap[legacy]) self.bustMap[id] = self.bustMap[legacy];
        }
        var vc = c.voice_card || {};
        self.charBios[id] = vc.worldview || vc.temperament || "";
        var words = c.name.split(/\s+/).filter(Boolean);
        var init = words.length > 1
          ? (words[0][0] + words[words.length - 1][0]).toUpperCase()
          : c.name.slice(0, 2).toUpperCase();
        // trust the engine's met flag over raw chapter arithmetic
        var metCh = c.met
          ? Math.min(c.first_appearance_chapter || 1, ch)
          : Math.max(c.first_appearance_chapter || (ch + 1), ch + 1);
        return {
          id: id, _legacy: legacy,
          name: c.name,
          short: (c.aliases && c.aliases[0]) || words[0],
          init: init,
          role: String((vc.diction || "").split(";")[0] || "Of the story"),
          metCh: metCh,
          hint: "Keep reading to meet them",
        };
      });
      cast.sort(function (a, b) { return a.metCh - b.metCh; });
      self.fullCast = cast;
      self.whoPeople = cast.filter(function (c) { return c.metCh <= ch; }).map(function (c) {
        return { name: c.name, desc: self.charBios[c.id] || c.role };
      });
      var met = cast.filter(function (c) { return c.metCh <= ch; });
      var curValid = cast.some(function (c) { return c.id === self.state.char && c.metCh <= ch; });
      if (!curValid) {
        var pick = (self._legacyIds.vh && met.some(function (c) { return c.id === self._legacyIds.vh; }))
          ? self._legacyIds.vh
          : (met.length ? met[met.length - 1].id : cast[0].id);
        self.setState({ char: pick });
      } else {
        self.forceUpdate();
      }
    }).catch(function () {});
  };

  P._loadTheories = function () {
    var self = this, bookId = this._bookId();
    if (bookId == null) return Promise.resolve();
    return V.call("list_theories", { bookId: bookId }).then(function (ts) {
      self._theoryCount = ts.length;
      self.baseTheories = ts.map(function (t) {
        return {
          text: t.text,
          status: t.resolved_status === "confirmed" ? "CONFIRMED"
            : t.resolved_status === "busted" ? "BUSTED" : "OPEN",
          at: t.resolved_at_chapter ? self.roman(t.resolved_at_chapter) : "",
          logged: self.roman(t.logged_at_chapter || 1),
        };
      });
      self.setState({ userTheories: [] });
    }).catch(function () {});
  };

  P._pinTheory = function (text) {
    var self = this, bookId = this._bookId();
    text = (text || "").trim();
    if (!text || bookId == null) return;
    V.call("add_theory", { bookId: bookId, text: text }).then(function () {
      self._toast("THEORY PINNED — IT TURNS WHEN THE STORY DOES");
      return self._loadTheories();
    }).catch(function (e) { self._up(e); });
  };

  P._loadEpisode = function (seq) {
    var self = this, ab = this._activeBookDef();
    if (!ab || ab.nid == null || ab.status === "forging") return;
    V.call("get_episode", { bookId: ab.nid, seq: seq }).then(function (ep) {
      self._episode = ep;
      self._readerSeq = seq;
      // DOMParser inert-parses the book's chapter HTML: unlike div.innerHTML it
      // never runs scripts or fires <img onerror> — a crafted EPUB can't inject
      // JS into the reader. We only read textContent out of it anyway.
      var doc = new DOMParser().parseFromString(ep.content_html || "", "text/html");
      var ps = Array.prototype.map.call(doc.querySelectorAll("p"), function (p) {
        // strip raw emphasis underscores / em-dash digraphs the plain-text
        // extraction leaves behind (matches the desktop reader's cleaning)
        return (p.textContent || "").replace(/_/g, "").replace(/--/g, "—").replace(/\s+/g, " ").trim();
      }).filter(function (t) { return t.length > 1; });
      if (ps.length) {
        self._readerPs = ps;
        self.enPs = ps.slice(0, 5);
        var chR = "CH." + self.roman(seq);
        self.corpus = ps.map(function (t) { return { ch: chR, line: t }; });
      }
      self.forceUpdate();
    }).catch(function () { self._episode = null; self._readerPs = null; });
  };

  P._loadTerms = function () {
    var self = this, bookId = this._bookId();
    if (bookId == null) return;
    V.call("get_wiki_page", { bookId: bookId, entityId: "group:terms", mode: "synced" }).then(function (pg) {
      var out = [];
      (pg.sections || []).forEach(function (s) {
        (s.facts || []).forEach(function (f) {
          var m = /^\(Ch\.\s*(\d+)\)\s*(.*)$/.exec(f);
          out.push(m ? { name: "CH. " + self.roman(+m[1]), desc: m[2] } : { name: s.heading.toUpperCase(), desc: f });
        });
      });
      if (out.length) { self.whoPlaces = out; self.forceUpdate(); }
    }).catch(function () {});
  };

  // ------------------------------------------------------------------- chat
  // per-character chat memory: hydrate the REAL stored thread on chat open
  // (spoiler-gated turns + a PREVIOUSLY divider) — parity with desktop
  P._loadChatHistory = function (charId) {
    var self = this;
    var bookId = this._bookId ? this._bookId() : null;
    var key = String(charId);
    this._histLoaded = this._histLoaded || {};
    if (bookId == null || (this.state.chats[charId] || []).length || this._histLoaded[key]) return;
    this._histLoaded[key] = true;
    var cid = /^\d+$/.test(String(charId)) ? Number(charId) : null;
    V.call("get_conversation", { bookId: bookId, characterId: cid }).then(function (h) {
      if (!h || !h.count || (self.state.chats[charId] || []).length) return;
      var items = [{
        bot: true,
        text: "◆ PREVIOUSLY — " + h.count + " EXCHANGE" + (h.count === 1 ? "" : "S")
          + ", UP TO CH. " + Math.max(1, h.last_chapter | 0)
          + ". THE THREAD PICKS UP WHERE YOU LEFT IT."
      }];
      (h.turns || []).forEach(function (t) {
        items.push(t.role === "user" ? { user: true, text: t.text } : { bot: true, text: t.text });
      });
      var chats = Object.assign({}, self.state.chats);
      chats[charId] = items.concat(chats[charId] || []);
      self.setState({ chats: chats });
      self._scrollChat();
    }).catch(function () { self._histLoaded[key] = false; });
  };


  // ------------------------------- first-run welcome stepper (parity w/ desktop)
  P._onboardAct = function (a) {
    var self = this;
    if (a === "skip" || a === "finish") {
      V.call("set_setting", { key: "onboarded", value: "1" }).catch(function () {});
      this.setState({ onboardStep: null });
      if (a === "finish") this._toast("DRACULA IS ON YOUR SHELF, PRE-FORGED — SAY HELLO TO THE CAST");
      return;
    }
    if (a === "next") { this.setState({ onboardStep: Math.min(4, (this.state.onboardStep | 0) + 1) }); return; }
    if (a === "back") { this.setState({ onboardStep: Math.max(0, (this.state.onboardStep | 0) - 1) }); return; }
    if (a === "dl-ink") this._startDl("ink");
    else if (a === "dl-quill") this._startDl("quill");
    else if (a === "dl-sketch") this._startPaintDl("sketch");
    else if (a === "dl-easel") this._startPaintDl("easel");
    else if (a.indexOf("gate-") === 0) {
      var g = a.slice(5);
      this.setState({ strict: g });
      V.call("set_setting", { key: "gate_mode", value: g }).catch(function () {});
    } else if (a.indexOf("theme-") === 0) {
      var t = a.slice(6);
      this.setState({ theme: t });
      V.call("set_setting", { key: "theme", value: t }).catch(function () {});
    }
  };

  P._applyOnboard = function () {
    var self = this;
    var step = this.state.onboardStep;
    var ov = document.querySelector("[data-vena-onboard]");
    if (step == null) { if (ov) ov.remove(); return; }
    if (!ov) {
      ov = document.createElement("div");
      ov.setAttribute("data-vena-onboard", "1");
      ov.setAttribute("role", "dialog");
      ov.setAttribute("aria-label", "Welcome to Vena — first-run setup");
      ov.style.cssText = "position:fixed;inset:0;z-index:200;background:rgba(20,16,20,.62);display:flex;align-items:flex-end;justify-content:center";
      ov.addEventListener("click", function (e) {
        var el = e.target && e.target.closest ? e.target.closest("[data-ob]") : null;
        if (el) self._onboardAct(el.getAttribute("data-ob"));
      });
      document.body.appendChild(ov);
    }
    var mono = "font-family:'IBM Plex Mono',monospace;letter-spacing:.08em;";
    function btn(act, label, primary) {
      return '<button data-ob="' + act + '" style="' +
        (primary ? "background:#d83a2c;color:#fff;border:none;box-shadow:3px 3px 0 #141014;" : "background:none;border:2px solid #141014;color:#141014;") +
        "padding:12px 16px;font-family:'Anton',sans-serif;font-size:13px;letter-spacing:.06em;cursor:pointer;min-height:44px\">" + label + "</button>";
    }
    function chip(act, label, on) {
      return '<button data-ob="' + act + '" style="background:' + (on ? "#141014" : "none") + ";color:" + (on ? "#f6f3ec" : "#141014") + ";border:2px solid #141014;padding:10px 12px;" + mono + 'font-size:9px;font-weight:600;cursor:pointer;min-height:40px">' + label + "</button>";
    }
    function dlRow(act, brand, size, desc, dl, tier, installed) {
      var busy = dl.status === "downloading" && dl.tier === tier;
      return '<div style="border:2.5px solid #141014;padding:11px 12px;margin-top:10px;display:flex;align-items:center;gap:10px">' +
        '<div style="flex:1"><div style="font-family:\'Anton\',sans-serif;font-size:13px">' + brand + ' <span style="' + mono + 'font-size:7.5px;color:#8a8286">' + size + "</span></div>" +
        '<div style="font-family:\'Source Serif 4\',serif;font-size:11px;color:#5c5458;margin-top:2px">' + desc + "</div>" +
        (busy ? '<div style="height:8px;border:2px solid #141014;margin-top:6px;position:relative"><div style="position:absolute;left:0;top:0;bottom:0;background:#2f9d95;width:' + (dl.pct | 0) + '%"></div></div>' : "") +
        "</div>" +
        (installed ? '<span style="' + mono + 'font-size:8.5px;color:#2f9d95;font-weight:600">INSTALLED ✓</span>'
          : busy ? '<span style="' + mono + 'font-size:8.5px;color:#8a8286">' + ((dl.pct | 0) >= 99 ? "VERIFYING…" : (dl.pct | 0) + "%") + "</span>"
            : btn(act, "GET", false)) + "</div>";
    }
    var st = this.state;
    var s = this._settings || {};
    function inst(id) { var t = (s.tiers || []).filter(function (x) { return x.id === id; })[0]; return !!(t && t.installed); }
    function pinst(id) { var t = (self._paintTiers || []).filter(function (x) { return x.id === id; })[0]; return !!(t && t.installed); }
    var titles = ["WELCOME TO VENA", "THE SPOILER GATE", "THE VOICE ENGINE", "THE PAINT ENGINE", "READY TO READ"];
    var serif = "font-family:'Source Serif 4',serif;font-size:13px;line-height:1.65;color:#5c5458";
    var bodies = [
      '<p style="' + serif + '">Vena is a <b>spoiler-safe reading companion</b>. Your books and every conversation live on <b>this device</b> — no account, no telemetry. Dracula is already on your shelf, pre-forged and ready to chat.</p><p style="' + serif + ';margin-top:8px">Every character reply is <b>gated to your bookmark</b>: they only know what you have read.</p>',
      '<p style="' + serif + '">Before a character speaks, the gate strips everything past your bookmark; a verifier checks each reply and anything that slips is <b style="color:#d83a2c">INKED OUT</b>. Feel a spoiler anyway? Tap <b>REPORT A LEAK</b>.</p>' +
        '<div style="display:flex;gap:8px;margin-top:12px;flex-wrap:wrap">' + chip("gate-strict", "STRICT", st.strict === "strict") + chip("gate-standard", "STANDARD", st.strict === "standard" || !st.strict) + chip("gate-relaxed", "RELAXED", st.strict === "relaxed") + "</div>",
      '<p style="' + serif + '">Characters need a voice. Download a model — it runs <b>inside the app</b> — or add a Cloud Relay key in Settings later.</p>' +
        dlRow("dl-ink", "INK·3B", "1.9 GB", "Fast and sure-footed.", st.dl || {}, "ink", inst("ink")) +
        dlRow("dl-quill", "QUILL·7B", "4.6 GB", "Richer, period-true voices.", st.dl || {}, "quill", inst("quill")),
      '<p style="' + serif + '">The paint engine draws <b>covers and portraits</b> on-device and refreshes them as you read. Every image is stamped ✦ AI. Optional.</p>' +
        dlRow("dl-sketch", "SKETCH·1.5", "2.0 GB", "Quick covers and portraits.", st.paintDl || {}, "sketch", pinst("sketch")) +
        dlRow("dl-easel", "EASEL·XL", "4.3 GB", "Richer paint.", st.paintDl || {}, "easel", pinst("easel")),
      '<p style="' + serif + '">Pick a light to read by — changeable any time.</p>' +
        '<div style="display:flex;gap:8px;margin-top:12px;flex-wrap:wrap">' + chip("theme-light", "☀ DAY", st.theme === "light" || !st.theme) + chip("theme-sepia", "▤ SEPIA", st.theme === "sepia") + chip("theme-dark", "☾ NIGHT", st.theme === "dark") + chip("theme-oled", "● OLED", st.theme === "oled") + "</div>" +
        '<p style="' + serif + ';margin-top:12px">Open the companion and say hello to Jonathan Harker — he only knows Chapter I, and so should you.</p>'
    ];
    var dots = titles.map(function (_, i) {
      return '<span style="width:9px;height:9px;display:inline-block;margin-right:5px;background:' + (i === step ? "#d83a2c" : i < step ? "#141014" : "#8a8286") + '"></span>';
    }).join("");
    ov.innerHTML = '<div style="background:#f6f3ec;color:#141014;border:3px solid #141014;border-bottom:none;box-shadow:0 -6px 0 rgba(0,0,0,.3);width:100%;max-height:88vh;overflow:auto;padding:20px 18px 30px">' +
      '<div style="display:flex;align-items:baseline;justify-content:space-between"><span style="' + mono + 'font-size:8px;color:#8a8286">FIRST-RUN SETUP · ' + (step + 1) + "/5</span><button data-ob=\"skip\" style=\"background:none;border:none;" + mono + 'font-size:8.5px;color:#8a8286;cursor:pointer;text-decoration:underline;min-height:40px">SKIP</button></div>' +
      '<h2 style="font-family:\'Anton\',sans-serif;font-size:22px;letter-spacing:.03em;margin:6px 0 10px">' + titles[step] + "</h2>" +
      bodies[step] +
      '<div style="display:flex;align-items:center;justify-content:space-between;margin-top:18px"><div>' + dots + '</div><div style="display:flex;gap:8px">' +
      (step > 0 ? btn("back", "← BACK", false) : "") +
      (step < 4 ? btn("next", "NEXT →", true) : btn("finish", "BEGIN →", true)) +
      "</div></div></div>";
  };

  /* keep the chat thread pinned to the newest message — the scroll column is
   * the parent of the "EVERY REPLY GATED" pill (no id in the design) */
  P._scrollChat = function () {
    requestAnimationFrame(function () {
      try {
        var divs = document.querySelectorAll("div");
        for (var i = 0; i < divs.length; i++) {
          var d = divs[i];
          if (d.childElementCount === 0 && (d.textContent || "").indexOf("EVERY REPLY GATED") === 0) {
            var sc = d.parentElement;
            if (sc) sc.scrollTop = sc.scrollHeight;
            return;
          }
        }
      } catch (e) { /* cosmetic — never break the turn */ }
    });
  };

  P._send = function (forcedText) {
    var self = this, st = this.state;
    if (this._busy) return;
    var charId = st.char;
    var text = (forcedText != null ? forcedText : st.input).trim();
    if (!text) return;
    var bookId = this._bookId();
    if (bookId == null) { this._toast("NO BOOK ON THE SHELF"); return; }
    var chats = Object.assign({}, st.chats);
    var list = (chats[charId] || []).slice();
    list.push({ user: true, text: text });
    chats[charId] = list;
    this._busy = true;
    var turnId = this._turnId = (this._turnId || 0) + 1;
    this.setState({ chats: chats, input: "", phase: "gate", stream: "" });
    this._scrollChat();
    var cid = /^\d+$/.test(String(charId)) ? Number(charId) : null;
    V.call("companion_turn", { bookId: bookId, characterId: cid, message: text, turnId: turnId })
      .then(function (rep) {
        var full = String(rep.reply || "");
        var shield = !!(rep.repaired || rep.redacted);
        self.setState({ phase: "gen" });
        var i = 0;
        self._iv = setInterval(function () {
          i += 3;
          self.setState({ stream: full.slice(0, i) });
          if (i >= full.length) {
            clearInterval(self._iv); self._iv = null;
            self.setState({ phase: "verify" });
            self._later(function () {
              var finish = function () {
                var c2 = Object.assign({}, self.state.chats);
                var l2 = (c2[charId] || []).slice();
                l2.push({ bot: true, text: full, shield: shield });
                c2[charId] = l2;
                self._busy = false;
                self.setState({ chats: c2, phase: null, stream: "" });
                self._scrollChat();
              };
              if (shield) { self.setState({ phase: "repair" }); self._later(finish, 1100); }
              else finish();
            }, shield ? 900 : 750);
          }
        }, 14);
      })
      .catch(function (e) {
        self._busy = false;
        self.setState({ phase: null, stream: "" });
        self._up(e);
      });
  };

  // ------------------------------------------------------------------ recap
  P._streamRecap = function () {
    var self = this;
    if (this.state.recapDone || this._recapBusy) return;
    var bookId = this._bookId();
    if (bookId == null) { this.setState({ recapOpen: false }); this._toast("NO BOOK ON THE SHELF"); return; }
    this._recapBusy = true;
    V.call("get_recap", { bookId: bookId }).then(function (txt) {
      self.recapText = String(txt || "");
      var words = self.recapText.split(" ");
      var i = 0;
      var step = function () {
        i += 2;
        self.setState({ recap: words.slice(0, i).join(" ") });
        if (i < words.length) self._later(step, 42);
        else { self._recapBusy = false; self.setState({ recapDone: true }); }
      };
      self._later(step, 400);
    }).catch(function (e) {
      self._recapBusy = false;
      self.setState({ recapOpen: false, recap: "", recapDone: false });
      self._up(e);
    });
  };

  // --------------------------------------------------------- model download
  P._startDl = function (tier) {
    var self = this;
    tier = tier || "quill";
    if (this._dlBusy) { this._toast("A DOWNLOAD IS ALREADY RUNNING"); return; }
    this._dlBusy = tier;
    this.setState({ dl: { status: "downloading", pct: 0, tier: tier } });
    var brand = ((this._settings && this._settings.tiers) || []).filter(function (t) { return t.id === tier; })
      .map(function (t) { return t.brand; })[0] || tier.toUpperCase();
    V.call("download_local_model", { tier: tier }).then(function () {
      self._dlBusy = null;
      self.setState({ dl: { status: "done", pct: 100, tier: tier } });
      self._toast(brand + " DOWNLOADED — READY TO ACTIVATE");
      return V.call("get_settings").then(function (s) { self._settings = s; self.forceUpdate(); });
    }).catch(function (e) {
      self._dlBusy = null;
      self.setState({ dl: { status: "idle", pct: 0, tier: tier } });
      V.call("get_settings").then(function (s) { self._settings = s; self.forceUpdate(); }).catch(function () {});
      if (/stopped/i.test(String((e && e.message) || e))) self._toast("DOWNLOAD STOPPED — PARTIAL KEPT, RESUME ANYTIME");
      else self._up(e);
    });
  };

  // paint weights: real download_paint_model, progress via model:progress kind:'paint'
  P._startPaintDl = function (tier) {
    var self = this;
    if (this._paintDlBusy) { this._toast("A DOWNLOAD IS ALREADY RUNNING"); return; }
    this._paintDlBusy = true;
    this.setState({ paintDl: { status: "downloading", pct: 0, tier: tier } });
    V.call("download_paint_model", { tier: tier }).then(function (r) {
      self._paintDlBusy = false;
      self.setState({ paintDl: { status: "done", pct: 100, tier: tier } });
      self._toast(r && r.engine_present === false
        ? "WEIGHTS INSTALLED — ALSO INSTALL stable-diffusion.cpp (sd CLI) TO PAINT LOCALLY"
        : ((r && r.brand) || "PAINT MODEL") + " INSTALLED — READY TO PAINT");
      V.call("paint_tiers").then(function (ts) { self._paintTiers = ts || []; self.forceUpdate(); }).catch(function () {});
      V.call("get_image_status").then(function (is) { self._imageStatus = is; self.forceUpdate(); }).catch(function () {});
      V.call("auto_paint").then(function (r) {
        if (r && r.error && !r.covers && !r.portraits) { self._toast("PAINT ENGINE FAILED — " + String(r.error).slice(0, 120)); return; }
        if (r && (r.covers || r.portraits)) self._toast("PAINTED " + r.covers + " COVERS · " + r.portraits + " PORTRAITS — STAMPED ✦ AI");
      }).catch(function () {});
    }).catch(function (e) {
      self._paintDlBusy = false;
      self.setState({ paintDl: { status: "idle", pct: 0 } });
      V.call("paint_tiers").then(function (ts) { self._paintTiers = ts || []; self.forceUpdate(); }).catch(function () {});
      if (/stopped/i.test(String((e && e.message) || e))) self._toast("DOWNLOAD STOPPED — PARTIAL KEPT, RESUME ANYTIME");
      else self._up(e);
    });
  };

  P._activateLocal = function (tierId, brand) {
    var self = this;
    V.call("set_chat_mode", { mode: "local" }).then(function () {
      self.setState({ model: tierId, relay: false });
      self._toast(brand + " NOW ANSWERS FOR THE CAST");
      self._refreshAi();
    }).catch(function (e) { self._up(e); });
  };

  // ----------------------------------------------------------------- probes
  P._runProbes = function () {
    var self = this;
    if (this.state.gateState === "running") return;
    var bookId = this._bookId();
    if (bookId == null) { this._toast("NO BOOK ON THE SHELF"); return; }
    this.setState({ gateState: "running" });
    var t0 = Date.now();
    V.call("run_probes", { bookId: bookId, n: 12 }).then(function (res) {
      var leaks = res.filter(function (r) { return r.leaked; });
      var kinds = { future_event: 0, unmet_character: 0, tone_implies_ending: 0 };
      leaks.forEach(function (l) { if (l.leak_kind && kinds[l.leak_kind] != null) kinds[l.leak_kind]++; });
      var avg = res.length ? ((Date.now() - t0) / res.length / 1000).toFixed(2) : "0.00";
      self._gateResult = (res.length - leaks.length) + "/" + res.length + " FUTURE PROBES BLOCKED " +
        (leaks.length ? "· " + leaks.length + " LEAKED" : "✓ · 0 LEAKS") +
        " · FUTURE EVENT " + kinds.future_event +
        " · UNMET CHARACTER " + kinds.unmet_character +
        " · TONE " + kinds.tone_implies_ending +
        " · AVG GATE " + avg + "S";
      self.setState({ gateState: "done" });
    }).catch(function (e) {
      self.setState({ gateState: "idle" });
      self._up(e);
    });
  };

  // --------------------------------------------------------------- progress
  P._setCh = function (next, toastMsg) {
    var self = this, bookId = this._bookId();
    if (bookId == null) return;
    V.call("set_progress", { bookId: bookId, episodeSeq: next, sceneSeq: 0 }).then(function () {
      self._buzz();
      if (self._bySlug && self._bySlug[self.state.book]) self._bySlug[self.state.book].progress_episode = next;
      self.setState({ chOverride: next, tocOpen: false });
      if (toastMsg) self._toast(toastMsg);
      self._loadEpisode(next);
      self._loadCharacters();
      self._loadTheories();
    }).catch(function (e) { self._up(e); });
  };

  // ------------------------------------------------------------------- wiki
  P._adoptWikiIndex = function (ab, idx) {
    var groups = {};
    (idx.entries || []).forEach(function (e) { (groups[e.group] = groups[e.group] || []).push(e); });
    var labels = { people: "PEOPLE — FULL FATES", terms: "TERMS & THINGS", places: "PLACES" };
    var sections = Object.keys(groups).map(function (g) {
      return { id: g, label: labels[g] || g.toUpperCase(), entries: [], _pending: groups[g] };
    });
    this.wiki[ab.id] = { infobox: null, sections: sections };
    if (sections.length) this._loadWikiSection(ab, sections[0]);
  };

  P._loadWikiSection = function (ab, sec) {
    var self = this;
    if (!sec || sec._loading || !sec._pending) return;
    sec._loading = true;
    Promise.all(sec._pending.map(function (e) {
      return V.call("get_wiki_page", { bookId: ab.nid, entityId: e.id, mode: "full" })
        .then(function (p) { return { meta: e, page: p }; })
        .catch(function () { return null; });
    })).then(function (rows) {
      rows = rows.filter(Boolean);
      rows.sort(function (a, b) { return (b.meta.fact_count || 0) - (a.meta.fact_count || 0); });
      sec.entries = rows.map(function (r) {
        var body = (r.page.sections || []).map(function (s) { return s.facts.join(" "); }).join(" ").trim();
        return {
          head: String(r.page.title || r.meta.name).toUpperCase(),
          body: body || "The ledger holds nothing on them yet.",
          stamp: (r.meta.fact_count || 0) + " FACTS",
        };
      });
      // infobox — the richest unsealed entity, straight from the ledger
      if (sec.id === "people" && rows.length && !self.wiki[ab.id].infobox) {
        var top = rows[0];
        self.wiki[ab.id].infobox = {
          title: String(top.page.title || "").toUpperCase() + " — UNSEALED",
          rows: (top.page.sections || []).map(function (s) {
            return { k: s.heading.toUpperCase(), v: s.facts.join(" ") };
          }),
        };
      }
      sec._pending = null;
      sec._loading = false;
      self.forceUpdate();
    });
  };

  P._unseal = function () {
    var self = this, ab = this._activeBookDef();
    if (!ab || ab.nid == null) return;
    V.call("set_spoiler_consent", { bookId: ab.nid, granted: true })
      .then(function () { return V.call("get_wiki_index", { bookId: ab.nid, mode: "full" }); })
      .then(function (idx) {
        self._adoptWikiIndex(ab, idx);
        var wu = Object.assign({}, self.state.wikiUnlocked); wu[ab.id] = true;
        self.setState({ wikiUnlocked: wu, wikiArmed: false, wikiSection: "" });
        self._toast("THE ARCHIVE IS OPEN — NOTHING BELOW IS SAFE");
      })
      .catch(function (e) { self.setState({ wikiArmed: false }); self._up(e); });
  };

  P._reseal = function () {
    var self = this, ab = this._activeBookDef();
    if (!ab || ab.nid == null) return;
    V.call("set_spoiler_consent", { bookId: ab.nid, granted: false }).then(function () {
      delete self.wiki[ab.id];
      var wu = Object.assign({}, self.state.wikiUnlocked); delete wu[ab.id];
      self.setState({ wikiUnlocked: wu, wikiArmed: false });
      self._toast("RE-SEALED. WHAT WAS READ, THOUGH, WAS READ.");
    }).catch(function (e) { self._up(e); });
  };

  // ----------------------------------------------------------------- import
  // BROWSE FILES, for real: Tauri → native dialog + import_book(path);
  // browser → hidden <input type=file> → base64 → import_book_data.
  P._venaBookInput = function () {
    if (this.__venaBookEl) return this.__venaBookEl;
    var self = this;
    var inp = document.createElement("input");
    inp.type = "file";
    inp.accept = ".epub,.txt,.cbz";
    inp.style.display = "none";
    inp.setAttribute("data-vena-book-input", "1");
    inp.addEventListener("change", function () {
      var f = inp.files && inp.files[0];
      inp.value = "";
      if (!f) return;
      var r = new FileReader();
      r.onload = function () {
        var b64 = String(r.result || "").split(",")[1] || "";
        if (!b64) { self._toast("COULD NOT READ THAT FILE"); return; }
        self._toast("FORGING — THE LEDGER IS BEING WRITTEN");
        V.call("import_book_data", { name: f.name, data: b64 }).then(function (meta) {
          self._toast("LEDGER FORGED ✓ — " + String(meta.title || "BOOK").toUpperCase() + " IS ON THE SHELF");
          self._hydrate();
        }).catch(function (e) { self._up(e); });
      };
      r.onerror = function () { self._toast("COULD NOT READ THAT FILE"); };
      r.readAsDataURL(f);
    });
    document.body.appendChild(inp);
    this.__venaBookEl = inp;
    return inp;
  };

  P._importPrompt = function () {
    var self = this;
    var T = window.__TAURI__;
    if (T) {
      var options = { multiple: false, filters: [{ name: "Books", extensions: ["epub", "txt", "cbz"] }] };
      var opened = (T.dialog && T.dialog.open)
        ? T.dialog.open(options)
        : T.core.invoke("plugin:dialog|open", { options: options });
      Promise.resolve(opened).then(function (sel) {
        var path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        self._toast("FORGING — THE LEDGER IS BEING WRITTEN");
        return V.call("import_book", { path: String(path) }).then(function (meta) {
          self._toast("LEDGER FORGED ✓ — " + String(meta.title || "BOOK").toUpperCase() + " IS ON THE SHELF");
          self._hydrate();
        });
      }).catch(function (e) { self._up(e); });
      return;
    }
    this._venaBookInput().click();
  };

  // -------------------------------------------- real CBZ pages (comic profile)
  // the viewer's shown header title (the design's demo header until replaced) —
  // used to pick WHICH real comic the viewer means when several are on the shelf
  P._mangaShownTitle = function () {
    var spans = document.querySelectorAll("span");
    for (var i = 0; i < spans.length; i++) {
      var o = spans[i].__vMgO != null ? spans[i].__vMgO : spans[i].textContent;
      if (String(o || "").indexOf("LITTLE NEMO") === 0) return String(o);
    }
    return "LITTLE NEMO IN SLUMBERLAND"; // the design's hardcoded demo header
  };

  // Resolve the REAL comic regardless of the current-book context: an explicit
  // target (a shelf comic card) wins; else prefer a title match against the
  // viewer's shown title; else the open book if it is a comic; else the first
  // comic on the shelf. Only a shelf with NO comic at all keeps the demo view.
  P._loadManga = function (target) {
    var self = this;
    var metas = [];
    if (this._bySlug) Object.keys(this._bySlug).forEach(function (k) { metas.push(self._bySlug[k]); });
    var comics = metas.filter(function (m) { return m.profile === "comic"; });
    var norm = function (s) { return String(s || "").toUpperCase().replace(/\s+/g, " ").trim(); };
    var shown = norm(this._mangaShownTitle());
    var meta = target
      || comics.filter(function (m) {
        var t = norm(m.title);
        return t && shown && (t === shown || t.indexOf(shown) === 0 || shown.indexOf(t) === 0);
      })[0]
      || comics.filter(function (m) { return m.slug === self.state.book; })[0]
      || comics[0];
    if (!meta) { this._manga = null; return; }
    if (target) this._mangaFail = null; // an explicit open always retries
    if (this._manga && this._manga.bookId === meta.id) return;
    if (this._mangaLoading === meta.id || this._mangaFail === meta.id) return;
    if (target && this._manga && this._manga.bookId !== meta.id) this._manga = null;
    this._mangaLoading = meta.id;
    V.call("get_manga_pages", { bookId: meta.id }).then(function (r) {
      if (self._mangaLoading === meta.id) self._mangaLoading = null;
      var count = (r && r.count) | 0;
      if (count <= 0) { self._mangaFail = meta.id; self._manga = null; self.setState({}); return; }
      self._manga = { bookId: meta.id, title: String(meta.title || "").toUpperCase(), count: count, cache: {}, pending: {} };
      self.setState({ mangaPage: 1 });
    }).catch(function (e) {
      if (self._mangaLoading === meta.id) self._mangaLoading = null;
      self._mangaFail = meta.id;
      self._manga = null;
      self._up(e);
    });
  };

  P._fetchMangaPage = function (n) {
    var self = this;
    var M = this._manga;
    if (!M || M.pending[n] || M.cache[n] || n < 1 || n > M.count) return;
    M.pending[n] = true;
    V.call("get_manga_page", { bookId: M.bookId, page: n }).then(function (r) {
      M.pending[n] = false;
      if (self._manga !== M || !r || !r.data) return;
      M.cache[n] = "data:" + (r.mime || "image/jpeg") + ";base64," + r.data;
      self.setState({});
    }).catch(function (e) { M.pending[n] = false; self._up(e); });
  };

  P._applyMangaPages = function () {
    if (!this.state.mangaOpen) {
      this._mangaZoom = 1; // next open starts at fit
      return;
    }
    var self = this;
    // the viewer can open from ANY path (demo showcase card, a shelf comic
    // card, restored state) — resolve the real comic whenever it is open
    if (!this._manga && !this._mangaLoading) this._loadManga();
    var M = this._manga;
    var els = document.querySelectorAll("span, div");
    Array.prototype.forEach.call(els, function (el) {
      if (el.__vMgO == null) el.__vMgO = el.textContent || "";
      if (!M) return; // prose: the design's demo view stays untouched
      if (el.tagName === "SPAN" && el.__vMgO.indexOf("LITTLE NEMO") === 0 && el.textContent !== M.title) {
        el.textContent = M.title;
      } else if (el.__vMgO.indexOf("PLACEHOLDER PAGES") === 0) {
        var want = "REAL CBZ · " + M.count + " PAGES FROM YOUR FILE · ⊖/⊕ OR DOUBLE-TAP TO ZOOM";
        if (el.textContent !== want) el.textContent = want;
      }
    });
    this._applyMangaZoomBtns(!!M);
    if (!M) return;
    Array.prototype.forEach.call(els, function (el) {
      if (el.tagName !== "SPAN" || !/^P\.\d+$/.test(el.textContent || "")) return;
      var n = +String(el.textContent || "").slice(2);
      var panel = el.parentElement;
      if (!panel) return;
      var img = panel.querySelector("img[data-vena-manga]");
      if (!img) {
        img = document.createElement("img");
        img.setAttribute("data-vena-manga", "1");
        img.style.cssText = "position:absolute;inset:0;width:100%;height:100%;object-fit:contain;display:none;image-rendering:pixelated";
        panel.appendChild(img);
      }
      img.alt = "Comic page " + n;
      var grid = panel.querySelector("div");
      if (grid) grid.style.visibility = "hidden";
      var src = n >= 1 && n <= M.count ? M.cache[n] : null;
      if (src) {
        if (img.getAttribute("src") !== src) img.setAttribute("src", src);
        img.style.display = "block";
        el.style.visibility = "hidden";
      } else {
        img.style.display = "none";
        el.style.visibility = "visible"; // the page number doubles as the loading mark
        if (n >= 1 && n <= M.count) self._fetchMangaPage(n);
      }
      // fit-to-page: the plate takes the scan's REAL aspect ratio (no 2:3
      // letterbox bars), and the vertical strip zooms by widening the plate —
      // panning is then plain scrolling
      if (!img.__vFit) {
        img.__vFit = true;
        img.addEventListener("load", function () { self._applyMangaPages(); });
      }
      if (img.naturalWidth > 0 && img.style.display === "block") {
        panel.style.aspectRatio = img.naturalWidth + " / " + img.naturalHeight;
      }
      var strip = panel.parentElement;
      var scrollMode = strip && /vh-scroll/.test(strip.className || "");
      if (scrollMode) {
        var z = self._mangaZoom || 1;
        panel.style.width = Math.round(92 * z) + "%";
        strip.style.overflowX = z > 1 ? "auto" : "";
        strip.style.alignItems = z > 1 ? "flex-start" : "center";
        if (!panel.__vZoomWired) {
          panel.__vZoomWired = true;
          panel.addEventListener("dblclick", function () { self._mangaZoomStep((self._mangaZoom || 1) > 1 ? -9 : 1); });
          panel.addEventListener("touchend", function (e) {
            var now = Date.now();
            if (now - (panel.__vLastTap || 0) < 320) {
              e.preventDefault();
              self._mangaZoomStep((self._mangaZoom || 1) > 1 ? -9 : 1);
            }
            panel.__vLastTap = now;
          });
        }
      }
    });
  };

  // ⊖ / ⊕ zoom for the vertical strip (real comics only); -9 = back to fit
  P._mangaZoomStep = function (d) {
    var zs = [1, 1.5, 2, 3];
    var i = zs.indexOf(this._mangaZoom || 1);
    if (i < 0) i = 0;
    this._mangaZoom = zs[d === -9 ? 0 : Math.max(0, Math.min(zs.length - 1, i + d))];
    this._applyMangaPages();
  };

  P._applyMangaZoomBtns = function (show) {
    if (!this.state.mangaOpen) return;
    var self = this;
    var close = null;
    Array.prototype.forEach.call(document.querySelectorAll("button"), function (b) {
      if ((b.textContent || "") === "✕" && b.parentElement && /P\./.test(b.parentElement.textContent || "")) close = b;
    });
    if (!close) return;
    var box = close.parentElement.querySelector("[data-vena-zoom]");
    if (!box) {
      box = document.createElement("span");
      box.setAttribute("data-vena-zoom", "1");
      box.style.cssText = "display:inline-flex;gap:6px;align-items:center";
      var mk = function (label, d) {
        var b = document.createElement("button");
        b.textContent = label;
        b.style.cssText = "background:none;border:2px solid var(--ink);color:var(--ink);height:34px;width:34px;font-size:13px;font-weight:600;cursor:pointer;padding:0";
        b.addEventListener("click", function () { self._mangaZoomStep(d); });
        box.appendChild(b);
      };
      mk("⊖", -1);
      mk("⊕", 1);
      close.parentElement.insertBefore(box, close);
    }
    box.style.display = show ? "inline-flex" : "none";
  };

  // -------------------------------------------- real reader text (DOM swap)
  // The template typesets its five reader paragraphs as literal HTML (they
  // are not bound), so the real chapter text from get_episode is applied to
  // those static <p> nodes after render. React never updates static text
  // children, so this is reconciliation-safe; the design's drop cap and the
  // margin-note pin are preserved.
  var origDidUpdate = P.componentDidUpdate;
  P.componentDidUpdate = function (prev) {
    origDidUpdate && origDidUpdate.call(this, prev);
    this._applyReaderText();
    this._applyDesignFacts();
    this._applyDataPrivacy();
    this._applyMangaPages();
    this._applyModelDel();
    this._applyReaderA11y();
    this._applyOnboard();
    // chat threads belong to a book: switching books (any path, including the
    // design's own cover taps) drops the hydrated threads of the old one
    if (this._chatBook !== this.state.book) {
      this._chatBook = this.state.book;
      this._histLoaded = {};
      if (Object.keys(this.state.chats || {}).length) this.setState({ chats: {} });
    }
    if (this.state.chatOpen) this._loadChatHistory(this.state.char);
    // remember the open book so the next launch resumes it (never before the
    // boot restore in _hydrate has read the previous value)
    if (this._navRestored) {
      try {
        if (this.state.book) localStorage.setItem("vena_last_book", this.state.book);
      } catch (_) {}
    }
  };

  // ------------------------------- reader type controls (Kindle-parity Aa)
  P._readerScaleSet = function (dir) {
    var steps = [0.85, 0.95, 1, 1.1, 1.2, 1.35, 1.5, 1.7];
    var s = this._readerScale || 1;
    if (dir === 0) s = 1;
    else {
      var i = steps.indexOf(s);
      if (i < 0) i = 2;
      s = steps[Math.max(0, Math.min(steps.length - 1, i + dir))];
    }
    this._readerScale = s;
    try { localStorage.setItem("vena_reader_scale", String(s)); } catch (_) {}
    this._applyReaderA11y();
  };

  P._readerLineSet = function () {
    var cyc = ["", "1.9", "2.15"];
    this._readerLine = cyc[(cyc.indexOf(this._readerLine || "") + 1) % cyc.length];
    try { localStorage.setItem("vena_reader_line", this._readerLine); } catch (_) {}
    this._applyReaderA11y();
  };

  P._applyReaderA11y = function () {
    var self = this;
    var box = document.querySelector("[data-vena-type]");
    var ps = document.querySelectorAll(".vhub p");
    // only over an open chapter — not the fresh-ledger / forging empty states
    if (this.state.screen !== "reader" || !ps.length) { if (box) box.style.display = "none"; return; }
    if (!box) {
      box = document.createElement("div");
      box.setAttribute("data-vena-type", "1");
      box.setAttribute("role", "group");
      box.setAttribute("aria-label", "Text size and line spacing");
      box.style.cssText = "position:fixed;right:10px;bottom:96px;z-index:80;display:flex;gap:5px;background:var(--panel);border:2.5px solid var(--ink);box-shadow:3px 3px 0 var(--shdw);padding:5px";
      var mk = function (label, aria, fn) {
        var b = document.createElement("button");
        b.textContent = label;
        b.setAttribute("aria-label", aria);
        b.style.cssText = "background:none;border:2px solid var(--ink);color:var(--ink);min-width:40px;min-height:40px;font-family:'IBM Plex Mono',monospace;font-size:12px;font-weight:700;cursor:pointer";
        b.addEventListener("click", fn);
        box.appendChild(b);
        return b;
      };
      mk("A−", "Smaller text", function () { self._readerScaleSet(-1); });
      box.__vPct = mk("100%", "Reset text size", function () { self._readerScaleSet(0); });
      mk("A+", "Larger text", function () { self._readerScaleSet(1); });
      box.__vLh = mk("≡", "Cycle line spacing (normal / roomy / airy)", function () { self._readerLineSet(); });
      document.body.appendChild(box);
    }
    box.style.display = "flex";
    var scale = this._readerScale || 1;
    if (box.__vPct) box.__vPct.textContent = Math.round(scale * 100) + "%";
    if (box.__vLh) {
      box.__vLh.textContent = this._readerLine === "1.9" ? "≡+" : this._readerLine === "2.15" ? "≡++" : "≡";
    }
    Array.prototype.forEach.call(ps, function (p) {
      if (p.__venaBaseFs == null) p.__venaBaseFs = parseFloat(window.getComputedStyle(p).fontSize) || 15;
      var wantFs = scale === 1 ? "" : (p.__venaBaseFs * scale).toFixed(1) + "px";
      if (p.style.fontSize !== wantFs) p.style.fontSize = wantFs;
      if (p.style.lineHeight !== (self._readerLine || "")) p.style.lineHeight = self._readerLine || "";
    });
  };

  // -------------------------------------------- model DELETE (parity w/ desktop)
  // Installed or partial (dead download) weights get a two-tap DELETE ✕ on the
  // tier row. Never anchors on the system bar's model badge — a row must hold
  // the brand label, a "GB" size line and the tier's action button.
  P._applyModelDel = function () {
    var self = this;
    var s = this._settings || {};
    var dlNow = (this.state.dl || {}).status === "downloading" ? (this.state.dl || {}).tier : null;
    var pdlNow = (this.state.paintDl || {}).status === "downloading" ? (this.state.paintDl || {}).tier : null;
    var rows = [];
    (s.tiers || []).forEach(function (t) {
      if ((t.installed || t.partial) && t.id !== dlNow) {
        rows.push({
          brand: t.brand, gb: t.size_gb, cmd: "delete_local_model", tier: t.id,
          after: function () { V.call("get_settings").then(function (x) { self._settings = x; self.forceUpdate(); }).catch(function () {}); }
        });
      }
    });
    (this._paintTiers || []).forEach(function (t) {
      if ((t.installed || t.partial) && t.id !== pdlNow) {
        rows.push({
          brand: t.brand, gb: t.size_gb, cmd: "delete_paint_model", tier: t.id,
          after: function () {
            V.call("paint_tiers").then(function (ts) { self._paintTiers = ts || []; self.forceUpdate(); }).catch(function () {});
            V.call("get_image_status").then(function (is) { self._imageStatus = is; self.forceUpdate(); }).catch(function () {});
          }
        });
      }
    });
    var want = {};
    rows.forEach(function (r) { want[r.brand] = true; });
    Array.prototype.forEach.call(document.querySelectorAll("[data-vena-model-del]"), function (b) {
      if (!want[b.getAttribute("data-vena-model-del")]) b.remove();
    });
    var findRow = function (brand) {
      var labels = Array.prototype.filter.call(document.querySelectorAll("div,span"), function (el) {
        return el.childElementCount === 0 && el.textContent === brand;
      });
      for (var i = 0; i < labels.length; i++) {
        var p = labels[i].parentElement;
        for (var d = 0; d < 5 && p; d++, p = p.parentElement) {
          var txt = p.textContent || "";
          if (txt.length > 600) break; // climbed out of the row
          var btn = Array.prototype.filter.call(p.querySelectorAll("button"), function (x) {
            return /DOWNLOAD|ACTIVATE|ACTIVE|INSTALLED|TOO BIG|VERIFYING|STOP|RESUME|GETTING/.test(x.textContent || "");
          })[0];
          if (btn && / GB/.test(txt)) return { row: p, main: btn };
        }
      }
      return null;
    };
    rows.forEach(function (r) {
      var hit = findRow(r.brand);
      if (!hit || hit.row.querySelector("[data-vena-model-del]")) return;
      var b = document.createElement("button");
      b.setAttribute("data-vena-model-del", r.brand);
      b.setAttribute("aria-label", "Delete " + r.brand + " from this device");
      var rest = function () {
        b.__vArmed = false;
        b.textContent = "DELETE ✕";
        b.style.borderColor = "var(--mut2)";
        b.style.color = "var(--mut)";
      };
      b.style.cssText = "background:none;border:2px solid var(--mut2);color:var(--mut);min-height:34px;padding:6px 10px;margin-left:6px;font-family:'IBM Plex Mono',monospace;font-size:8px;letter-spacing:.06em;font-weight:600;cursor:pointer";
      rest();
      b.addEventListener("click", function (e) {
        e.stopPropagation();
        if (!b.__vArmed) {
          b.__vArmed = true;
          b.textContent = "SURE? ✕";
          b.style.borderColor = "var(--red)";
          b.style.color = "var(--red)";
          setTimeout(function () { if (b.isConnected && b.__vArmed) rest(); }, 4000);
          return;
        }
        b.textContent = "DELETING…";
        V.call(r.cmd, { tier: r.tier }).then(function () {
          self._toast(r.brand + " DELETED — " + r.gb.toFixed(1) + " GB FREED");
          b.remove();
          r.after();
        }).catch(function (err) { b.remove(); self._up(err); });
      });
      hit.main.after(b);
    });
  };

  // -------------------------------------------- static demo copy → real data
  // A few plates in the canonical template are hardcoded showcase HTML with no
  // bindings (the theory flip card, the WHO'S WHO tally, the reader kicker).
  // Their text nodes are rewritten from REAL data after each render — the same
  // reconciliation-safe technique as the reader text.
  P._applyDesignFacts = function () {
    var self = this;
    var ch = this._chNow();
    var els = document.querySelectorAll("span, div");
    var orig = function (el) { if (el.__vOrig == null) el.__vOrig = el.textContent; return el.__vOrig; };
    var put = function (el, txt) { if (el.__vTxt !== txt) { el.__vTxt = txt; el.textContent = txt; } };
    Array.prototype.forEach.call(els, function (el) {
      // reader kicker (has an <em> child) → the real episode's own heading
      if (el.children.length) {
        var f = el.firstChild;
        if (el.__vKick || (f && f.nodeType === 3 && f.nodeValue.indexOf("DR. SEWARD") === 0)) {
          el.__vKick = true;
          var ep = self._episode;
          if (ep && ep.title && f && f.nodeType === 3) {
            var want = String(ep.title).toUpperCase() + " — ";
            if (f.nodeValue !== want) f.nodeValue = want;
          }
        }
        return;
      }
      var o = orig(el);
      // theory flip card → the first ledger-CONFIRMED theory, or hidden
      if (o === "REVEAL REACHED!!" && el.tagName === "SPAN") {
        var card = el.parentElement;
        while (card && (card.getAttribute("style") || "").indexOf("perspective") === -1) card = card.parentElement;
        if (!card) return;
        var th = (self.baseTheories || []).filter(function (t) { return t.status === "CONFIRMED"; })[0];
        card.style.display = th ? "" : "none";
        if (!th) return;
        Array.prototype.forEach.call(card.querySelectorAll("span, div"), function (c) {
          if (c.children.length) return;
          var co = orig(c);
          if (/^PINNED CH\./.test(co)) put(c, "PINNED CH." + th.logged);
          else if (/ran aground are connected/.test(co)) put(c, "“" + th.text + "”");
          else if (/^RESOLVED · CHAPTER/.test(co)) put(c, "RESOLVED · CHAPTER " + (th.at || th.logged));
          else if (/^The pieces met in Chapter/.test(co)) put(c, "The story caught up at Chapter " + (th.at || th.logged) + " — confirmed by the ledger, never by guesswork.");
        });
        return;
      }
      // WHO'S WHO tally → real cast/terms counts at the current horizon
      if (o === "13 ENTRIES · 4 SEALED") {
        var entries = (self.whoPeople || []).length + (self.whoPlaces || []).length;
        var sealed = (self.fullCast || []).filter(function (c) { return c.metCh > ch; }).length;
        if (entries) put(el, entries + " ENTRIES · " + sealed + " SEALED");
      }
    });
  };

  // ---------------- portable-data layer (SETTINGS ▸ DATA & PRIVACY) --------
  // The canonical DATA & PRIVACY plate is static markup with only exportData /
  // wipeBook anchors. We inject the portable-data actions (sync export/import,
  // per-book theory share, forget conversations) into the design's own button
  // column after render, cloning the plate's exact button style so it matches
  // the house style. Idempotent + self-healing; the template is never edited.
  P._venaDownload = function (filename, obj) {
    try {
      var blob = new Blob([JSON.stringify(obj, null, 2)], { type: "application/json" });
      var a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      setTimeout(function () { try { URL.revokeObjectURL(a.href); a.remove(); } catch (e) {} }, 0);
      return true;
    } catch (err) { return false; }
  };
  P._venaExportSync = function () {
    var self = this;
    V.call("export_bundle", { scope: "sync" }).then(function (bundle) {
      var n = ((bundle && bundle.books) || []).length;
      if (self._venaDownload("vena-sync.json", bundle)) {
        self._toast("EXPORTED · " + n + " BOOK" + (n === 1 ? "" : "S") + " · YOUR DATA, YOUR FILE");
      } else self._toast("EXPORT FAILED — COULD NOT WRITE THE FILE");
    }).catch(function (e) { self._up(e); });
  };
  P._venaImportInput = function () {
    if (this.__venaImportEl) return this.__venaImportEl;
    var self = this;
    var inp = document.createElement("input");
    inp.type = "file";
    inp.accept = ".json,application/json";
    inp.style.display = "none";
    inp.addEventListener("change", function () {
      var f = inp.files && inp.files[0];
      inp.value = "";
      if (!f) return;
      var r = new FileReader();
      r.onload = function () { self._venaImportText(String(r.result || "")); };
      r.onerror = function () { self._toast("COULD NOT READ THAT FILE"); };
      r.readAsText(f);
    });
    document.body.appendChild(inp);
    this.__venaImportEl = inp;
    return inp;
  };
  P._venaImportPick = function () { this._venaImportInput().click(); };
  P._venaImportText = function (text) {
    var self = this;
    V.call("import_bundle", { json: text }).then(function (rep) {
      rep = rep || {};
      var parts = ["SYNCED"];
      var mb = rep.matched_books || 0;
      parts.push(mb + " BOOK" + (mb === 1 ? "" : "S"));
      if (rep.progress_updated) parts.push(rep.progress_updated + " PROGRESS");
      var ta = rep.theories_added || 0;
      parts.push(ta + " THEOR" + (ta === 1 ? "Y" : "IES") + " ADDED");
      var skipped = rep.skipped_not_on_shelf || [];
      if (skipped.length) parts.push(skipped.length + " NOT ON SHELF");
      self._toast(parts.join(" · ").slice(0, 88));
      self._hydrate();
    }).catch(function (e) { self._up(e); });
  };
  P._venaShareTheories = function () {
    var self = this;
    var ab = this._activeBookDef();
    var bid = this._bookId();
    if (!ab || bid == null) { this._toast("NO BOOK OPEN — NOTHING TO SHARE"); return; }
    V.call("export_bundle", { bookId: bid, scope: "theories" }).then(function (bundle) {
      var b = ((bundle && bundle.books) || [])[0] || {};
      var n = (b.theories || []).length;
      if (self._venaDownload("vena-theories-" + ab.id + ".json", bundle)) {
        self._toast("SHARED · " + n + " THEOR" + (n === 1 ? "Y" : "IES") + " · PASS IT ROUND THE BOOK CLUB");
      } else self._toast("SHARE FAILED — COULD NOT WRITE THE FILE");
    }).catch(function (e) { self._up(e); });
  };
  P._venaForgetChats = function () {
    var self = this;
    var bid = this._bookId();
    if (bid == null) { this._toast("NO BOOK OPEN"); return; }
    if (this.__venaForgetArmed !== bid) {
      this.__venaForgetArmed = bid;
      this._toast("TAP AGAIN TO FORGET EVERY CONVERSATION FOR THIS BOOK");
      setTimeout(function () { if (self.__venaForgetArmed === bid) self.__venaForgetArmed = null; }, 4000);
      return;
    }
    this.__venaForgetArmed = null;
    V.call("forget_conversations", { bookId: bid }).then(function () {
      self._histLoaded = {}; // hydrated history went with the store rows
      self.setState({ chats: {} });
      self._toast("CONVERSATIONS FORGOTTEN — THE BOOK, LEDGER & THEORIES REMAIN");
    }).catch(function (e) { self._up(e); });
  };
  P._applyDataPrivacy = function () {
    var self = this;
    var btns = Array.prototype.slice.call(document.querySelectorAll("button"));
    var exportBtn = null, burnBtn = null;
    for (var i = 0; i < btns.length; i++) {
      var t = (btns[i].textContent || "").trim();
      if (!exportBtn && t.indexOf("EXPORT THEORIES") === 0) exportBtn = btns[i];
      if (!burnBtn && t === "BURN THIS BOOK'S DATA") burnBtn = btns[i];
    }
    var anchor = exportBtn || burnBtn;
    if (!anchor) return;
    var row = anchor.parentElement;
    if (!row) return;
    var ab = this._activeBookDef();

    if (!row.querySelector('[data-vena-dp="share"]')) {
      var inkStyle = (exportBtn || burnBtn).getAttribute("style") || "";
      var redStyle = (burnBtn || exportBtn).getAttribute("style") || "";
      var cyanStyle = inkStyle.replace(/var\(--ink\)/g, "var(--cyan)");
      var mk = function (label, style, key, onClick) {
        var b = document.createElement("button");
        b.textContent = label;
        b.setAttribute("style", style);
        b.setAttribute("data-vena-dp", key);
        b.addEventListener("click", onClick);
        row.appendChild(b);
        return b;
      };
      mk("EXPORT MY DATA", inkStyle, "exportSync", function () { self._venaExportSync(); });
      mk("IMPORT", inkStyle, "import", function () { self._venaImportPick(); });
      mk("SHARE THEORIES", cyanStyle, "share", function () { self._venaShareTheories(); });
      mk("FORGET OUR CONVERSATIONS", redStyle, "forget", function () { self._venaForgetChats(); });
    }
    var has = !!ab;
    ["share", "forget"].forEach(function (key) {
      var b = row.querySelector('[data-vena-dp="' + key + '"]');
      if (!b) return;
      b.disabled = !has;
      b.style.opacity = has ? "1" : ".4";
      b.style.cursor = has ? "pointer" : "not-allowed";
    });
  };

  P._applyReaderText = function () {
    if (this.state.screen !== "reader" || !this._readerPs || !this._readerPs.length) return;
    var key = "s" + this._readerSeq;
    var ps = document.querySelectorAll(".vhub p");
    for (var i = 0; i < ps.length; i++) {
      var p = ps[i], t = this._readerPs[i];
      if (!t || p.getAttribute("data-vena-real") === key) continue;
      var first = p.firstElementChild;
      if (i === 0 && first && first.tagName === "SPAN" && (first.textContent || "").length <= 2) {
        // keep the design's drop cap
        while (first.nextSibling) p.removeChild(first.nextSibling);
        first.textContent = t.charAt(0);
        p.appendChild(document.createTextNode(t.slice(1)));
      } else if (first && (first.textContent || "").trim() === "✎") {
        // keep the margin-note pin
        while (first.nextSibling) p.removeChild(first.nextSibling);
        p.appendChild(document.createTextNode(t));
      } else {
        p.textContent = t;
      }
      p.setAttribute("data-vena-real", key);
    }
  };

  // ------------------------------------------------- renderVals: swap fakes
  var origRV = P.renderVals;
  P.renderVals = function () {
    var self = this;
    var v = origRV.call(this);
    var st = this.state;
    var ab = this._activeBookDef();
    var meta = (ab && this._bySlug) ? this._bySlug[ab.id] : null;
    var ch = v.ch;

    // --- shelf: per-book real numbers
    if (this._bySlug) {
      v.books.forEach(function (b) {
        var m = self._bySlug[b.id];
        if (!m) return;
        // a comic card opens the manga page view wired to ITS bookId
        if (m.profile === "comic") {
          var openManga = function () { self.setState({ mangaOpen: true }); self._loadManga(m); };
          b.act = openManga;
          b.goComp = openManga;
          b.btnLabel = "PAGE VIEW →";
          b.btnCur = "pointer";
          b.btnOp = "1";
        }
        b.facts = (m.fact_count || 0).toLocaleString();
        var prog = m.progress_episode || 0;
        if (b.id === st.book) prog = ch;
        b.posLabel = prog > 0
          ? "CH. " + prog + " OF " + m.episode_count + " · " + Math.round((prog / Math.max(1, m.episode_count)) * 100) +
            "% · " + ((self._theoryCount || 0) + st.userTheories.length) + " THEORIES PINNED"
          : "NOT STARTED · " + m.episode_count + " CHAPTERS AHEAD OF YOU";
      });
      v.delConfirm = function () {
        var slug = self.state.delModal;
        var m = self._bySlug[slug];
        if (!m) { self.setState({ delModal: null }); return; }
        V.call("delete_book", { id: m.id }).then(function () {
          self.setState({ delModal: null });
          self._toast("BOOK BURNED — LEDGER, THEORIES AND CHATS WENT WITH IT");
          self._hydrate();
        }).catch(function (e) { self.setState({ delModal: null }); self._up(e); });
      };
    }
    // BROWSE FILES (import) and READ THE BRANCH share the template's {{ noop }}
    v.noop = function (e) {
      var t = (e && e.target && e.target.textContent) || "";
      if (/BROWSE FILES/i.test(t)) self._importPrompt();
      else if (/READ THE BRANCH/i.test(t)) self._toast("WHAT-IF BRANCHES AREN’T WIRED TO THE ENGINE YET");
    };

    // --- chat starter chips: book-agnostic (the design's are Dracula demo
    // lines) + a memory recap now that turns carry conversation history
    v.chips = [
      { label: "REMIND ME — WHERE DO THINGS STAND?", send: function () { self._send("Remind me — where do things stand right now?"); } },
      { label: "WHAT DID WE TALK ABOUT LAST TIME?", send: function () { self._send("What did we talk about last time?"); } }
    ];

    // --- comics: real CBZ pages when a comic is on the shelf, demo otherwise
    v.mangaOpenFn = function () { self.setState({ mangaOpen: true }); self._loadManga(); };
    if (this._manga) {
      var MG = this._manga;
      var mgPage = Math.max(1, Math.min(st.mangaPage, MG.count));
      v.mangaPageN = mgPage;
      v.mangaPageLbl = mgPage + " / " + MG.count;
      v.mangaPrev = function () { self.setState({ mangaPage: Math.max(1, mgPage - 1) }); };
      v.mangaNext = function () { self.setState({ mangaPage: Math.min(MG.count, mgPage + 1) }); };
      v.mangaStrip = [];
      for (var mgi = 0; mgi < Math.max(1, Math.min(4, MG.count - mgPage + 1)); mgi++) v.mangaStrip.push({ n: mgPage + mgi });
    }

    // --- app bar: honest model badge
    if (this._ai) {
      var badge = this._ai.ready
        ? (this._ai.mode === "cloud" ? "RELAY (CLOUD)" : String(this._ai.model || "LOCAL").toUpperCase() + " LOCAL")
        : "NO MODEL — SET ⚙";
      v.modelBadge = badge;
      if (v.leakCtx && v.leakCtx[2]) v.leakCtx[2].v = badge;
    }

    // --- theories
    v.theoryMeta = ((this._theoryCount || 0) + st.userTheories.length) + " PINNED";
    var pinReal = function () {
      var t = self.state.newTheory.trim();
      if (!t) return;
      self.setState({ newTheory: "" });
      self._pinTheory(t);
    };
    v.addTheory = pinReal;
    v.theoryKey = function (e) { if (e.key === "Enter") pinReal(); };
    v.msgs = v.msgs.map(function (m) {
      if (!m.bot) return m;
      return Object.assign({}, m, {
        pin: function () {
          var q = m.text.length > 90 ? m.text.slice(0, 90) + "…" : m.text;
          self._pinTheory(v.charShort + " is hiding something: “" + q + "”");
        },
      });
    });

    // --- progress: mark-read / read-ahead / TOC jumps / horizon dial
    var total = ab ? ab.total : 27;
    var advance = function (prefix) {
      if (ch >= total) { self._toast("THAT WAS THE LAST PAGE."); return; }
      var next = ch + 1;
      var newlyMet = self.fullCast.find(function (c) { return c.metCh === next; });
      self._setCh(next, prefix + " → CH." + next +
        (newlyMet ? " · SOMEONE NEW STEPS OUT OF THE INK" : " · THE CAST HAS CAUGHT UP TO YOU"));
    };
    v.markRead = function () { advance("HORIZON MOVED"); };
    v.readAhead = function () { self._toast("READING AHEAD — THE STREAK PAUSES, NOTHING IS LOST"); advance("HORIZON"); };
    v.chUp = function () { if (ch < total) self._setCh(ch + 1, "HORIZON MOVED → CH." + (ch + 1)); };
    v.chDown = function () { if (ch > 1) self._setCh(ch - 1, "HORIZON PULLED BACK → CH." + (ch - 1) + " · THE CAST FORGETS"); };
    v.toc = v.toc.map(function (row, i) {
      var n = i + 1;
      return Object.assign({}, row, {
        jump: function () {
          if (n === ch) { self.setState({ tocOpen: false }); return; }
          self._setCh(n, n < ch ? "RE-SEALED TO CH." + n : "HORIZON → CH." + n);
        },
      });
    });
    if (this._episode) {
      v.statsLine = "≈ " + (this._episode.est_minutes != null ? this._episode.est_minutes : "?") +
        " MIN IN CHAPTER · " + (this._episode.scene_count || 1) + " SCENES IN THE LEDGER" +
        (st.readerMode === "paged" ? " · PAGE " + st.pageN + "/18" : "");
    }

    // --- who-links in the typeset page → real character ids
    var legacyWho = function (key) {
      return function (e) {
        if (e && e.stopPropagation) e.stopPropagation();
        var id = self._legacyIds && self._legacyIds[key];
        if (id) self.setState({ whoId: id });
      };
    };
    if (this._legacyIds && Object.keys(this._legacyIds).length) {
      v.whoVH = legacyWho("vh");
      v.whoLucy = legacyWho("lucy");
      var vhId = this._legacyIds.vh || st.char;
      v.askPassage = function () {
        self.setState({ chatOpen: true, char: vhId, input: "Why do you insist on the garlic? What is it for?" });
      };
      v.selAsk = function () {
        self.setState({ selOpen: false, dictOpen: false, chatOpen: true, char: vhId,
          input: "About this line: “" + (self.state.selText || "") + "” — what should I make of it?" });
      };
    }

    // --- honest toasts for absent capabilities (never fake a translation)
    var noTranslate = function () {
      self.setState({ langOpen: false, transOpen: false });
      self._toast("TRANSLATION NEEDS A LANGUAGE MODEL — NOT WIRED IN THIS BUILD");
    };
    v.transOpts = v.transOpts.map(function (o, i) {
      if (i === 0) return o; // OFF stays local
      return Object.assign({}, o, { pick: noTranslate });
    });
    v.selTrans = noTranslate;
    v.langChips = v.langChips.map(function (l, i) {
      var code = ["fr", "ja", "de", "es"][i];
      return Object.assign({}, l, {
        pick: function () {
          V.call("set_setting", { key: "target_language", value: LANG_NAME[code] || "French" })
            .then(function () { self.setState({ targetLang: code }); })
            .catch(function (e) { self._up(e); });
        },
      });
    });

    // --- wiki: consent-gated unseal/reseal + lazy page loads
    v.wikiOpen = function () { self._unseal(); };
    v.wikiReseal = function () { self._reseal(); };
    if (v.archOpen && ab && this.wiki[ab.id]) {
      var wd = this.wiki[ab.id];
      var cs = wd.sections.find(function (x) { return x.id === st.wikiSection; }) || wd.sections[0];
      if (cs && cs._pending && !cs._loading) {
        setTimeout(function () { self._loadWikiSection(ab, cs); }, 0);
      }
      if (!v.wikiLead) v.wikiLead = "Everything the ledger holds — full spoilers, nothing softened.";
      v.wikiCats = [
        { label: (meta && meta.profile ? meta.profile.toUpperCase() : "PROSE") },
        { label: (meta && meta.license ? meta.license.toUpperCase() : "") },
        { label: "FULL SPOILERS" },
      ].filter(function (c) { return c.label; });
      v.wikiFooterMeta = "WRITTEN BY THE LEDGER · " + (meta ? (meta.fact_count || 0) + " FACTS" : "") +
        " · CONTENT DRAWN ONLY FROM THE BOOK · NOTHING LEAVES THIS PHONE";
    }

    // --- Test the Gate → run_probes(12), taxonomy counts from leak_kind
    v.gateTest = function () { self._runProbes(); };
    if (this._gateResult) v.gateResult = this._gateResult;

    // --- leak report → report_leak
    v.leakSubmit = function () {
      var bookId = self._bookId();
      if (bookId == null) { self.setState({ leakOpen: false, leakMsg: null }); return; }
      var reason = ({ future: "future_event", character: "unmet_character", tone: "tone_implies_ending", other: "other" })[self.state.leakReason] || "other";
      V.call("report_leak", { bookId: bookId, reason: reason, excerpt: self.state.leakMsg || "", comment: "" })
        .then(function () {
          self._buzz();
          self.setState({ leakOpen: false, leakMsg: null });
          self._toast("LEAK FILED — THE GATE ADDS A RULE FOR THIS BOOK");
        })
        .catch(function (e) { self.setState({ leakOpen: false, leakMsg: null }); self._up(e); });
    };

    // --- model tiers: real catalog, real installs, real download
    if (this._settings && this._settings.tiers && this._settings.tiers.length) {
      var s = this._settings;
      var brandToId = {};
      s.tiers.forEach(function (t) { brandToId[t.brand] = t.id; });
      var installedId = s.local_ready ? brandToId[s.local_model] : null;
      if (st.dl.status === "done" && !installedId) installedId = st.dl.tier;
      v.models = s.tiers.map(function (t) {
        var blocked = t.id === "arch";
        // install state is the plausible weights on disk (backend-checked) —
        // stale flags and dead downloads never show INSTALLED
        var installed = !!t.installed;
        var partial = !!t.partial && !installed;
        // ACTIVE requires the engine to actually answer (get_ai_status probes)
        var aiUp = !!(self._ai && self._ai.ready);
        var active = installed && st.model === t.id && aiUp;
        var offline = installed && st.model === t.id && !aiUp;
        var downloading = st.dl.status === "downloading" && st.dl.tier === t.id;
        return {
          id: t.id, chip: t.chip, name: t.brand,
          size: t.size_gb.toFixed(1) + " GB" + (offline ? " · ENGINE OFFLINE"
            : installed ? " · INSTALLED"
              : partial ? " · PARTIAL — RESUME OR DELETE"
                : blocked ? " · NEEDS " + t.min_ram_gb + " GB" : ""),
          desc: MODEL_DESCS[t.id] || "",
          installed: installed, blocked: blocked, active: active,
          op: blocked ? ".45" : "1",
          shdw: active ? "4px 4px 0 var(--cyan)" : "3px 3px 0 var(--shdw)",
          cur: installed && !active ? "pointer" : "default",
          chipBg: active ? "var(--cyan)" : "var(--ink)", chipCol: "var(--inv)",
          downloading: downloading,
          dlW: (downloading ? st.dl.pct : 0) + "%", dlPct: downloading ? st.dl.pct : 0,
          btnLabel: blocked ? "TOO BIG" : active ? "ACTIVE" : offline ? "START SERVER →"
            : installed ? "ACTIVATE"
              : downloading ? (st.dl.pct >= 99 ? "VERIFYING…" : "STOP ✕")
                : partial ? "RESUME ↓" : "DOWNLOAD",
          btnBg: active ? "var(--cyan)" : "transparent",
          btnCol: active ? "var(--inv)" : blocked ? "var(--mut2)" : "var(--ink)",
          btnCur: blocked ? "default" : "pointer",
          pick: function () { if (installed && !active) self._activateLocal(t.id, t.brand); },
          btnAct: function (e) {
            if (e && e.stopPropagation) e.stopPropagation();
            if (blocked || active) return;
            if (offline) {
              self._toast("WEIGHTS ON DISK, NO RUNTIME — RUN ./scripts/serve-local.sh OR SWITCH TO CLOUD RELAY");
              return;
            }
            if (installed) { self._activateLocal(t.id, t.brand); return; }
            if (downloading) {
              // STOP keeps the .part — the same button then offers RESUME
              if (st.dl.pct < 99) V.call("cancel_model_download", { kind: "chat", tier: t.id }).catch(function () {});
              return;
            }
            self._startDl(t.id);
          },
        };
      });
    }

    // --- paint models: the REAL tier catalog (paint_tiers) + real downloads
    if (this._paintTiers && this._paintTiers.length) {
      var pDl = st.paintDl || {};
      var pDescs = {
        sketch: "Stable Diffusion 1.5 — quick covers and portraits on this device.",
        easel: "SDXL base — richer paint, heavier download.",
      };
      v.paintModels = this._paintTiers.map(function (t) {
        var installed = !!t.installed;
        var partial = !!t.partial && !installed;
        var downloading = pDl.status === "downloading" && pDl.tier === t.id;
        return {
          id: "paint-" + t.id,
          chip: t.id === "sketch" ? "1.5" : "XL",
          name: t.brand,
          size: t.size_gb.toFixed(1) + " GB" + (installed ? " · INSTALLED" : partial ? " · PARTIAL — RESUME OR DELETE" : ""),
          desc: pDescs[t.id] || "",
          active: false, op: "1",
          shdw: installed ? "3px 3px 0 var(--cyan)" : "2px 2px 0 var(--shdw)",
          chipBg: installed ? "var(--cyan)" : "var(--ink)", chipCol: "var(--inv)",
          downloading: downloading,
          dlW: (downloading ? pDl.pct | 0 : 0) + "%", dlPct: downloading ? pDl.pct | 0 : 0,
          btnLabel: installed ? "INSTALLED ✓"
            : downloading ? ((pDl.pct | 0) >= 99 ? "VERIFYING…" : "STOP ✕")
              : partial ? "RESUME ↓" : "DOWNLOAD",
          btnAria: t.brand + (installed ? " — installed" : " — download"),
          btnBg: "transparent", btnCol: installed ? "var(--cyan)" : "var(--ink)",
          btnCur: installed ? "default" : "pointer",
          btnAct: function () {
            if (installed) return;
            if (downloading) {
              if ((pDl.pct | 0) < 99) V.call("cancel_model_download", { kind: "paint", tier: t.id }).catch(function () {});
              return;
            }
            self._startPaintDl(t.id);
          },
        };
      });
      // an api image endpoint (via the relay) still shows as an honest status row
      var imgSt = this._imageStatus;
      if (imgSt && imgSt.tier === "api") {
        v.paintModels.push({
          id: "paint-api", chip: "API",
          name: String(imgSt.model || "IMAGE ENDPOINT").toUpperCase(),
          size: "VIA RELAY",
          desc: "Covers and portraits arrive from the configured image endpoint — stamped ✦ AI.",
          active: true, op: "1", shdw: "3px 3px 0 var(--cyan)",
          chipBg: "var(--cyan)", chipCol: "var(--inv)",
          downloading: false, dlW: "0%", dlPct: 0,
          btnLabel: "ACTIVE", btnAria: "Image endpoint — active",
          btnBg: "transparent", btnCol: "var(--cyan)", btnCur: "default",
          btnAct: function () {},
        });
      }
    } else if (this._imageStatus && this._imageStatus.tier === "none") {
      var paintToast = function () { self._toast("LOCAL PAINT MODELS AREN’T WIRED — PORTRAITS ARRIVE VIA THE RELAY"); };
      v.paintModels = v.paintModels.map(function (m) {
        return Object.assign({}, m, {
          active: false,
          downloading: false, dlW: "0%", dlPct: 0,
          shdw: "2px 2px 0 var(--shdw)",
          size: m.size.replace(" · INSTALLED", ""),
          btnLabel: m.blocked ? "TOO BIG" : "NOT WIRED",
          btnAria: m.name + (m.blocked ? " — too big for this phone" : " — not wired in this build"),
          btnBg: "transparent", btnCol: "var(--mut2)", btnCur: "default",
          btnAct: m.blocked ? function () {} : paintToast,
        });
      });
    }
    // --- dictionary packs: no store backend for packs — honest toast, no fake bars
    v.packs = v.packs.map(function (p) {
      return Object.assign({}, p, {
        isIdle: true, isDl: false, isDone: false, pctW: "0%", pctLbl: "0%",
        get: function () { self._toast("DICTIONARY PACKS AREN’T WIRED TO A STORE YET — WORDNET STAYS BUILT-IN"); },
      });
    });

    // --- relay: real chat-mode switch, config, model list and gate test
    v.relayToggle = function () {
      var on = !self.state.relay;
      V.call("set_chat_mode", { mode: on ? "cloud" : "local" }).then(function () {
        self.setState({ relay: on });
        self._toast(on ? "RELAY ON — THE GATE RUNS LOCALLY BEFORE ANYTHING LEAVES THE PHONE" : "RELAY OFF — FULLY LOCAL AGAIN");
        self._refreshAi();
      }).catch(function (e) { self._up(e); });
    };
    var saveRelayCfg = function () {
      return V.call("set_api_config", {
        baseUrl: self.state.relayUrl || "",
        apiKey: self.state.relayKey || "",
        model: self.state.relayModel || "",
      });
    };
    v.relayFetch = function () {
      if (self.state.relayFetchSt === "run") return;
      self.setState({ relayFetchSt: "run" });
      saveRelayCfg()
        .then(function () { return V.call("list_relay_models"); })
        .then(function (ms) { self.setState({ relayFetchSt: "done", relayModels: ms || [] }); })
        .catch(function (e) { self.setState({ relayFetchSt: "idle", relayModels: [] }); self._up(e); });
    };
    v.relayTest = function () {
      if (self.state.relayTestSt === "run") return;
      self.setState({ relayTestSt: "run" });
      saveRelayCfg()
        .then(function () { return V.call("test_relay"); })
        .then(function (r) {
          self._relayMsg = String(r.message || "").toUpperCase() ||
            ("ROUND-TRIP " + (r.latency_ms || 0) + " MS · GATE " + (r.gate_verified ? "VERIFIED" : "NOT VERIFIED"));
          if (r.ok) { self._buzz(); self.setState({ relayTestSt: "done", relayLatency: r.latency_ms || 0 }); self._refreshAi(); }
          else { self.setState({ relayTestSt: "idle" }); self._toast(self._relayMsg); }
        })
        .catch(function (e) { self.setState({ relayTestSt: "idle" }); self._up(e); });
    };
    if (this._relayMsg && st.relayTestSt === "done") v.relayTestResult = this._relayMsg;

    // --- one-tap relay presets: pick a provider (pre-fills base+model), paste a
    // key (or none for localhost), CONNECT runs configure_relay (fill+persist+TEST
    // in one call). The manual base/key/model fields stay as the "custom" fallback.
    var presets = this._relayPresets || [];
    if (presets.length) {
      var manualRelayTest = v.relayTest;
      var provList = presets.map(function (p) {
        return { k: p.id, label: String(p.name).toUpperCase(), url: p.base_url, model: p.default_model };
      }).concat([{ k: "custom", label: "CUSTOM", url: "" }]);
      v.provChips = provList.map(function (p) {
        return {
          label: p.label,
          pressed: st.relayProv === p.k ? "true" : "false",
          bg: st.relayProv === p.k ? "var(--ink)" : "transparent",
          col: st.relayProv === p.k ? "var(--inv)" : "var(--ink)",
          pick: function () {
            self.setState({ relayProv: p.k, relayUrl: p.url || st.relayUrl, relayModel: p.model || st.relayModel, relayModels: [], relayFetchSt: "idle", relayTestSt: "idle" });
          },
        };
      });
      v.relayTestLabel = st.relayTestSt === "run" ? "CONNECTING…" : "CONNECT";
      v.relayTest = function () {
        var pr = provList.find(function (p) { return p.k === st.relayProv; });
        if (!pr || pr.k === "custom") { manualRelayTest(); return; }
        if (self.state.relayTestSt === "run") return;
        self.setState({ relayTestSt: "run" });
        V.call("configure_relay", { provider: pr.k, apiKey: self.state.relayKey || "", model: (self.state.relayModel || "").trim() })
          .then(function (r) {
            self._relayMsg = r.gate_verified
              ? "GATE VERIFIED · " + (r.latency_ms || 0) + " MS"
              : (String(r.message || "").toUpperCase() || "ROUND-TRIP " + (r.latency_ms || 0) + " MS");
            if (r.ok) { self._buzz(); self.setState({ relayTestSt: "done", relayLatency: r.latency_ms || 0 }); self._refreshAi(); }
            else { self.setState({ relayTestSt: "idle" }); self._toast(self._relayMsg); }
          })
          .catch(function (e) { self.setState({ relayTestSt: "idle" }); self._up(e); });
      };
    }

    // --- streaming forge: companion usable through forgedThrough while forging
    if (ab && ab.status === "forging") {
      var ft = (this._forgeState && ab.nid != null && this._forgeState[ab.nid] && this._forgeState[ab.nid].forgedThrough) ||
        (typeof st.forgedThrough === "number" ? st.forgedThrough : 0);
      var fcount = meta ? (meta.fact_count || 0) : 0;
      if (ft >= ch || fcount > 0) {
        v.compReady = true;
        v.compForging = false;
        var thru = ft > 0 ? "CH." + this.roman(ft) : "THE OPENING";
        if (v.compAuthorUp != null) v.compAuthorUp = v.compAuthorUp + " · READY THROUGH " + thru + ", STILL FORGING";
      }
    }

    // --- settings → set_setting (persisted by the engine)
    var setKV = function (key, value) {
      V.call("set_setting", { key: key, value: value }).catch(function (e) { self._up(e); });
    };
    v.strictOpts = ["strict", "standard", "relaxed"].map(function (k, i) {
      return Object.assign({}, v.strictOpts[i], {
        pick: function () { self.setState({ strict: k }); setKV("gate_mode", k); },
      });
    });
    var tglKeys = { "SHOW THE ENGINE STAMPS": ["tglStamps", "show_engine_stamps"], "GUARD CHARACTER FATES": ["tglFates", "guard_fates"] };
    v.engineToggles = v.engineToggles.map(function (t) {
      var pair = tglKeys[t.label];
      if (!pair) return t; // silhouettes / art are local presentation prefs
      return Object.assign({}, t, {
        flip: function () {
          var nv = !self.state[pair[0]];
          self.setState((function (o) { var u = {}; u[pair[0]] = nv; return u; })());
          setKV(pair[1], nv ? "1" : "0");
        },
      });
    });
    v.resealTgl = function () {
      var nv = !self.state.tglReseal;
      self.setState({ tglReseal: nv });
      setKV("reseal_on_reread", nv ? "1" : "0");
    };
    // serial pacing has no engine behind it — the toggle refuses honestly
    // instead of showing a fabricated countdown/streak in the reader
    v.serialTgl = function () { self._toast("SERIAL PACING ISN’T WIRED IN THIS BUILD — EVERY CHAPTER IS OPEN"); };
    var origTheme = v.themeToggle;
    v.themeToggle = function () {
      origTheme();
      var order = ["light", "sepia", "dark", "oled"];
      var next = order[(order.indexOf(v.themeName) + 1) % order.length];
      setKV("theme", next);
    };

    // --- positioning: lead with ownership + privacy (bound placeholders,
    // safely overridden here as TEXT only — layout untouched). Spoiler-safety
    // stays as a supporting proof-point ("no names ahead of your bookmark").
    if (typeof v.shelfMeta === "string") {
      v.shelfMeta = v.shelfMeta.replace("EVERYTHING ON THIS PHONE", "YOUR BOOKS, YOUR PHONE, NOTHING LEAVES IT");
    }
    if (typeof v.castMeta === "string") {
      v.castMeta = v.castMeta.replace(" AHEAD", " STILL INK") + " — YOUR BOOK, NO NAMES AHEAD OF YOUR BOOKMARK";
    }
    if (!ab) {
      v.readerEmptyBody = "Bring your own book — import an EPUB and it opens on your shelf. Every page stays on this phone.";
    }

    return v;
  };
})();
