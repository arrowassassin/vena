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
    this._offVena = V.onEvent(function (ev) { self._onVenaEvent(ev); });
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
      var dl = this.state.dl || {};
      if (dl.status === "downloading" && (!dl.tier || dl.tier === p.tier)) {
        this.setState({ dl: { status: "downloading", pct: Math.max(dl.pct || 0, p.pct | 0), tier: dl.tier || p.tier } });
      }
    } else if (name === "forge:progress") {
      if (p.pct != null) this.setState({ forgePct: p.pct | 0 });
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
      var cur = self.bookDefs.find(function (b) { return b.id === self.state.book; }) || self.bookDefs[0] || null;
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
      var div = document.createElement("div");
      div.innerHTML = ep.content_html || "";
      var ps = Array.prototype.map.call(div.querySelectorAll("p"), function (p) {
        return (p.textContent || "").replace(/\s+/g, " ").trim();
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
      self._up(e);
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
  P._importPrompt = function () {
    var self = this;
    var path = window.prompt("Path to an EPUB or .vena package on this machine:");
    if (!path || !path.trim()) return;
    this._toast("FORGING — THE LEDGER IS BEING WRITTEN");
    V.call("import_book", { path: path.trim() }).then(function (meta) {
      self._toast("LEDGER FORGED ✓ — " + String(meta.title || "BOOK").toUpperCase() + " IS ON THE SHELF");
      self._hydrate();
    }).catch(function (e) { self._up(e); });
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
        var installed = t.id === installedId;
        var active = installed && st.model === t.id;
        var downloading = st.dl.status === "downloading" && st.dl.tier === t.id;
        return {
          id: t.id, chip: t.chip, name: t.brand,
          size: t.size_gb.toFixed(1) + " GB" + (installed ? " · INSTALLED" : blocked ? " · NEEDS " + t.min_ram_gb + " GB" : ""),
          desc: MODEL_DESCS[t.id] || "",
          installed: installed, blocked: blocked, active: active,
          op: blocked ? ".45" : "1",
          shdw: active ? "4px 4px 0 var(--cyan)" : "3px 3px 0 var(--shdw)",
          cur: installed && !active ? "pointer" : "default",
          chipBg: active ? "var(--cyan)" : "var(--ink)", chipCol: "var(--inv)",
          downloading: downloading,
          dlW: (downloading ? st.dl.pct : 0) + "%", dlPct: downloading ? st.dl.pct : 0,
          btnLabel: blocked ? "TOO BIG" : active ? "ACTIVE" : installed ? "ACTIVATE" : downloading ? "GETTING…" : "DOWNLOAD",
          btnBg: active ? "var(--cyan)" : "transparent",
          btnCol: active ? "var(--inv)" : blocked ? "var(--mut2)" : "var(--ink)",
          btnCur: blocked ? "default" : "pointer",
          pick: function () { if (installed && !active) self._activateLocal(t.id, t.brand); },
          btnAct: function (e) {
            if (e && e.stopPropagation) e.stopPropagation();
            if (blocked || active) return;
            if (installed) { self._activateLocal(t.id, t.brand); return; }
            if (downloading) { self._toast("DOWNLOAD RUNNING — LET IT FINISH"); return; }
            self._startDl(t.id);
          },
        };
      });
    }

    // --- paint models: no local diffusion backend exists — never claim installs
    if (this._imageStatus && this._imageStatus.tier === "none") {
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
        self._toast(on ? "RELAY ON — GATED TEXT ONLY LEAVES THE PHONE" : "RELAY OFF — FULLY LOCAL AGAIN");
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
    var origTheme = v.themeToggle;
    v.themeToggle = function () {
      origTheme();
      var order = ["light", "sepia", "dark", "oled"];
      var next = order[(order.indexOf(v.themeName) + 1) % order.length];
      setKV("theme", next);
    };

    return v;
  };
})();
