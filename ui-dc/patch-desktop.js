/* patch-desktop.js — appended INSIDE the dc-script, after `class Component`.
 *
 * Two jobs:
 *  1. The canonical desktop design is truncated at 256 KiB, mid-renderVals().
 *     build.mjs closes the class and calls `this._venaTail(locals)`; this file
 *     defines that tail — every template binding the lost tail used to provide.
 *  2. Wire the design to the REAL backend through window.VENA (vena-bridge.js):
 *     real books, cast, theories, wiki, episodes, probes, relay, settings.
 *     Where the backend has no capability (translate, vision forge, dictionary
 *     packs, paint models) the UI stays but actions toast honestly — no fakes.
 */
(function () {
  'use strict';
  if (typeof Component === 'undefined') return;
  const P = Component.prototype;
  const V = (typeof window !== 'undefined' && window.VENA) || null;

  /* ---------------- helpers ---------------- */
  const UP = s => String(s == null ? '' : s).toUpperCase();
  const cleanTxt = s => String(s || '').replace(/_/g, '').replace(/--/g, '—');
  const shaShort = sha => sha ? UP(sha.slice(0, 4)) + '…' + UP(sha.slice(-2)) : '—';
  const initialsOf = name => {
    const w = String(name || '?').split(/\s+/).filter(x => /^[A-Za-z]/.test(x));
    return (w.length > 1 ? w[0][0] + w[w.length - 1][0] : (w[0] || '?').slice(0, 1)).toUpperCase();
  };
  const lastWord = n => String(n || '').split(' ').slice(-1)[0];
  // map a real character name onto the design's art/bust keys
  const keyOf = name => {
    const n = String(name || '').toLowerCase();
    if (n.includes('dracula') || n.includes('count')) return 'dr';
    if (n.includes('mina')) return 'mina';
    if (n.includes('lucy')) return 'lucy';
    if (n.includes('helsing')) return 'vh';
    if (n.includes('seward')) return 'js';
    if (n.includes('harker')) return 'jh';
    if (n.includes('holmwood') || n === 'arthur') return 'ah';
    if (n.includes('quincey') || n.includes('morris')) return 'qm';
    return null;
  };
  const FALLBACK_COVERS = [
    'radial-gradient(circle at 50% 30%, rgba(240,198,106,.5) 0 14%, transparent 34%), linear-gradient(168deg,#3c4452,#161a20)',
    'radial-gradient(circle at 40% 24%, rgba(42,223,223,.4) 0 12%, transparent 36%), linear-gradient(168deg,#4a3040,#1d1219)',
    'radial-gradient(circle at 60% 30%, rgba(224,69,60,.45) 0 12%, transparent 36%), linear-gradient(168deg,#2f4038,#131a16)'
  ];
  const DRACULA_COVER = 'radial-gradient(circle at 50% 26%, #e0453c 0 21%, rgba(224,69,60,.3) 22%, transparent 40%), conic-gradient(from 205deg at 24% 74%, #0a070d 0 52deg, transparent 52deg), conic-gradient(from 215deg at 52% 66%, #0a070d 0 46deg, transparent 46deg), conic-gradient(from 210deg at 78% 72%, #0a070d 0 50deg, transparent 50deg), linear-gradient(180deg, transparent 70%, #07050a 70.5%), linear-gradient(170deg, #241129, #10090f)';
  const coverFor = slug => slug === 'dracula' ? DRACULA_COVER
    : FALLBACK_COVERS[Math.abs(String(slug).split('').reduce((a, c) => (a * 31 + c.charCodeAt(0)) | 0, 7)) % FALLBACK_COVERS.length];
  const chapterSpan = facts => {
    const ns = [];
    (facts || []).forEach(f => { const m = /\(Ch\.\s*(\d+)\)/.exec(f); if (m) ns.push(+m[1]); });
    if (!ns.length) return '';
    const lo = Math.min.apply(null, ns), hi = Math.max.apply(null, ns);
    return 'CH.' + P.roman(lo) + (hi > lo ? '–' + P.roman(hi) : '');
  };
  const stripStamp = f => String(f || '').replace(/^\(Ch\.\s*\d+\)\s*/, '');

  /* ---------------- error surfacing (honest, never fake) ---------------- */
  P._honest = function (prefix, e) {
    const code = e && e.code;
    if (code === 'NoBackend') {
      this._toast('NO AI CONFIGURED — ADD CLOUD RELAY OR A LOCAL MODEL IN SETTINGS');
    } else if (code === 'SpoilerConsentRequired') {
      this._toast('THE ARCHIVE STAYS SEALED UNTIL YOU CONSENT');
    } else {
      const msg = (e && e.message) ? String(e.message) : String(e || 'FAILED');
      this._toast(UP(prefix + ' — ' + msg).slice(0, 88));
    }
  };

  P._curBookMeta = function () {
    const defs = this.bookDefs || [];
    const def = defs.find(b => b.id === this.state.book) || defs[0];
    return def && def.meta ? def.meta : null;
  };

  /* ---------------- boot: real data in the design's exact shapes ---------------- */
  const _mount = P.componentDidMount;
  P.componentDidMount = function () {
    _mount.call(this);
    // stop the demo "forging" ticker — forge progress now comes from forge:progress events
    if (this._forge) { clearInterval(this._forge); this._forge = null; }
    this._venaInit();
  };

  const _unmount = P.componentWillUnmount;
  P.componentWillUnmount = function () {
    this._venaUnsub && this._venaUnsub();
    _unmount && _unmount.call(this);
  };

  const _didUpdate = P.componentDidUpdate;
  P.componentDidUpdate = function (pp) {
    _didUpdate && _didUpdate.call(this, pp);
    this._venaReaderText();
    this._venaDesignFacts();
    this._venaDataPrivacy();
    this._venaMangaDom();
  };

  P._venaInit = function () {
    // blank the demo data immediately — nothing fake renders while we load
    this.bookDefs = [];
    this.fullCast = [];
    this.baseTheories = [];
    this.wiki = {};
    this.whoPeople = []; this.whoPlaces = []; this.whoTerms = [];
    this.corpus = [];
    this.storeFeatured = []; this.storeSE = []; this.storeGut = []; this.catalogBooks = [];
    this._vena = {
      books: [], theories: [], settings: null, ai: null, image: null,
      wikiIdx: null, episode: null, forge: {}, storeOffline: false, relayPresets: [],
      paintTiers: []
    };
    this._manga = null;
    this._turnSeq = 0;
    // demo-state honesty: no fake serial countdown (no pacing engine exists),
    // no prefilled margin note from a chapter the reader never opened
    this.setState({ book: '', catalogs: [], tglSerial: false, notes: [] });
    if (!V) { this._toast('VENA BRIDGE MISSING — NO BACKEND ON THIS PAGE'); return; }
    this._venaUnsub = V.onEvent(e => this._venaEvent(e));

    V.call('list_books').then(books => {
      this._applyBooks(books);
      const first = (books || [])[0];
      if (first) {
        this.setState({ book: first.slug, chOverride: Math.max(1, first.progress_episode || 1) });
        this._loadBook(first);
      }
    }).catch(e => this._honest('SHELF UNREACHABLE', e));

    this._loadSettings();
    // one-tap relay presets (§ configure_relay) — pre-fill base+model per provider
    V.call('relay_presets').then(ps => { this._vena.relayPresets = ps || []; this.setState({}); }).catch(() => {});
    // the store's network sources are probed on first visit to THE STORE —
    // real calls that may honestly fail, so they are not fired at boot
    this._storeLoaded = false;
  };

  P._loadSettings = function () {
    V.call('get_settings').then(s => {
      this._vena.settings = s;
      this.setState({
        strict: s.gate_mode || 'standard',
        tglFates: !!s.guard_fates,
        tglStamps: !!s.show_engine_stamps,
        tglReseal: !!s.reseal_on_reread,
        relay: !!(s.cloud_base_url && s.default_chat_mode === 'cloud'),
        relayUrl: s.cloud_base_url || this.state.relayUrl,
        relayModel: s.cloud_model || this.state.relayModel
      });
    }).catch(e => this._honest('SETTINGS', e));
    V.call('get_ai_status').then(a => { this._vena.ai = a; this.setState({}); }).catch(() => {});
    this._loadPaint();
  };

  // local paint tiers (paint_tiers) + the image endpoint status (get_image_status)
  P._loadPaint = function () {
    V.call('get_image_status').then(i => { this._vena.image = i; this.setState({}); }).catch(() => {});
    V.call('paint_tiers').then(ts => { this._vena.paintTiers = ts || []; this.setState({}); }).catch(() => {});
  };

  P._applyBooks = function (books) {
    this._vena.books = books || [];
    this.bookDefs = this._vena.books.map(b => {
      const sealed = b.forge_state === 'sealed';
      return {
        id: b.slug,
        title: UP(b.title), author: UP(b.author || 'UNKNOWN'),
        cover: coverFor(b.slug),
        status: sealed ? (b.progress_episode > 0 ? 'forged' : 'fresh')
          : (b.forge_state === 'forging' ? 'forging' : 'raw'),
        total: b.episode_count,
        stats: b.fact_count + ' FACTS · COVERAGE ' + Math.round((b.ledger_coverage || 0) * 100) + '% · SHA ' + shaShort(b.package_sha),
        meta: b
      };
    });
    this.setState({});
  };

  P._refreshBooks = function () {
    return V.call('list_books').then(bs => {
      this._applyBooks(bs);
      if (!bs.find(b => b.slug === this.state.book)) {
        const first = bs[0];
        this.setState({ book: first ? first.slug : '' });
        if (first) this._loadBook(first);
      }
    }).catch(e => this._honest('SHELF REFRESH', e));
  };

  P._loadBook = function (meta) {
    this._loadCast(meta);
    this._loadTheories(meta);
    this._loadWho(meta);
    this._loadEpisode(meta, Math.max(1, meta.progress_episode || 1));
  };

  P._loadCast = function (meta) {
    return V.call('list_characters', { bookId: meta.id }).then(chars => {
      const cast = (chars || []).slice()
        .sort((a, b) => a.first_appearance_chapter - b.first_appearance_chapter)
        .map(c => ({
          id: c.id, name: c.name,
          short: lastWord(c.name), init: initialsOf(c.name),
          role: (String((c.voice_card && c.voice_card.temperament) || '').split(';')[0].split('.')[0]) || 'Of the story',
          bio: (c.voice_card && c.voice_card.worldview) || '',
          metCh: c.first_appearance_chapter, met: c.met,
          hint: 'Keep reading — a name not yet spoken'
        }));
      this.fullCast = cast;
      const ch = this.state.chOverride || meta.progress_episode || 1;
      const metNow = cast.filter(c => c.metCh <= ch);
      const cur = cast.find(c => c.id === this.state.char);
      if (!cur && metNow.length) this.setState({ char: metNow[metNow.length - 1].id });
      else this.setState({});
    }).catch(e => this._honest('CAST', e));
  };

  P._loadTheories = function (meta) {
    return V.call('list_theories', { bookId: meta.id }).then(ts => {
      this._vena.theories = ts || [];
      this.setState({});
    }).catch(e => this._honest('THEORY BOARD', e));
  };

  // who's-who from the synced (spoiler-safe) wiki index + one page per entity
  P._loadWho = function (meta) {
    return V.call('get_wiki_index', { bookId: meta.id, mode: 'synced' }).then(idx => {
      this._vena.wikiIdx = idx;
      const rows = { people: [], places: [], terms: [] };
      const jobs = (idx.entries || []).map(e =>
        V.call('get_wiki_page', { bookId: meta.id, entityId: e.id, mode: 'synced' }).then(pg => {
          const facts = [].concat.apply([], (pg.sections || []).map(s => s.facts || []));
          (rows[e.group] || rows.terms).push({
            name: e.name,
            desc: facts.length ? stripStamp(facts[0]) : 'Nothing on the ledger yet — keep reading.',
            seen: chapterSpan(facts)
          });
        }).catch(() => {
          (rows[e.group] || rows.terms).push({ name: e.name, desc: e.fact_count + ' facts on the ledger.', seen: '' });
        }));
      return Promise.all(jobs).then(() => {
        this.whoPeople = rows.people; this.whoPlaces = rows.places; this.whoTerms = rows.terms;
        this.setState({});
      });
    }).catch(e => this._honest('WHO’S WHO', e));
  };

  P._loadEpisode = function (meta, seq) {
    return V.call('get_episode', { bookId: meta.id, seq }).then(ep => {
      let paras = [];
      try {
        const doc = new DOMParser().parseFromString(ep.content_html || '', 'text/html');
        paras = Array.prototype.slice.call(doc.querySelectorAll('p'))
          .map(p => cleanTxt(p.textContent).trim()).filter(Boolean);
      } catch (err) { /* keep [] */ }
      if (!paras.length && ep.content_html) {
        paras = [cleanTxt(ep.content_html.replace(/<[^>]+>/g, ' ')).trim()];
      }
      this._vena.episode = { seq: ep.seq, title: ep.title, est: ep.est_minutes, scenes: ep.scene_count, paras };
      // the reader's search corpus = the real text you have actually opened
      this.corpus = paras.map(t => ({
        ch: 'CH.' + this.roman(ep.seq),
        line: t.length > 120 ? t.slice(0, 120) + '…' : t
      }));
      this.setState({});
    }).catch(e => this._honest('CHAPTER ' + seq, e));
  };

  P._persistProgress = function (n) {
    const meta = this._curBookMeta();
    if (!meta) return;
    V.call('set_progress', { bookId: meta.id, episodeSeq: n, sceneSeq: 0 }).then(() => {
      meta.progress_episode = n;
      this._applyBooks(this._vena.books);
      this._loadCast(meta);
      this._loadWho(meta);
      this._loadTheories(meta); // resolutions come ONLY from the backend
      this._loadEpisode(meta, n);
    }).catch(e => this._honest('PROGRESS NOT SAVED', e));
  };

  P._selectBook = function (def) {
    this.setState({
      book: def.id, wikiArmed: false, resolved: false,
      chOverride: Math.max(1, (def.meta && def.meta.progress_episode) || 1)
    });
    if (def.meta) this._loadBook(def.meta);
  };

  /* ---------------- store: real sources, honest failures ---------------- */
  P._loadStore = function () {
    V.call('store_search', { query: '' }).then(items => {
      this.storeFeatured = (items || []).map(x => this._storeItemDef(x, 'feat'));
      const done = {};
      (items || []).forEach(x => { if (x.on_shelf) done[x.id] = { status: 'done', pct: 100 }; });
      this.setState(s => ({ storeSt: Object.assign({}, s.storeSt, done) }));
    }).catch(e => this._honest('STORE CATALOG', e));

    const tryBrowse = (source, into, src) => V.call('store_browse', { source }).then(items => {
      this[into] = (items || []).map(x => this._storeItemDef(x, src));
      this.setState({});
      return true;
    }).catch(() => {
      this._vena.storeOffline = true;
      this.setState({});
      return false;
    });
    // probe Gutenberg first; if the network is blocked, don't hammer the rest
    tryBrowse('gutenberg', 'storeGut', 'gut').then(ok => {
      if (ok) return tryBrowse('standard-ebooks', 'storeSE', 'se');
      this._toast('GUTENBERG & STANDARD EBOOKS UNREACHABLE — NETWORK IS BLOCKED HERE');
      return false;
    });

    V.call('list_opds_catalogs').then(cats => {
      this.setState({ catalogs: (cats || []).map(c => ({ id: c.id, name: c.name, url: c.url, open: false, books: [], loaded: false })) });
    }).catch(e => this._honest('CATALOGS', e));
  };

  // map a real StoreItem into the design's store-item shape
  P._storeItemDef = function (x, src) {
    const shelf = this._vena.books.find(b => b.slug === x.id || b.title === x.title);
    return {
      id: x.id,
      title: UP(x.title), author: UP(x.author || 'UNKNOWN'),
      year: UP(x.license || 'PUBLIC DOMAIN'),
      size: x.source === 'vena-catalog' ? 'PKG' : 'EPUB',
      cover: coverFor(x.id),
      facts: shelf ? shelf.fact_count + ' FACTS' : '—',
      cov: shelf ? Math.round((shelf.ledger_coverage || 0) * 100) + '%' : '—',
      num: x.id, lic: UP(x.license || 'PUBLIC DOMAIN'),
      src, pre: x.source === 'vena-catalog',
      item: x
    };
  };

  P._browseCatalog = function (cat, i) {
    V.call('store_browse', { source: cat.id }).then(items => {
      const cats = this.state.catalogs.map((c, j) => j === i
        ? Object.assign({}, c, { loaded: true, books: (items || []).map(x => this._storeItemDef(x, 'opds')) }) : c);
      this.setState({ catalogs: cats });
    }).catch(e => {
      const cats = this.state.catalogs.map((c, j) => j === i ? Object.assign({}, c, { loaded: true, books: [] }) : c);
      this.setState({ catalogs: cats });
      this._honest(UP(cat.name) + ' UNREACHABLE', e);
    });
  };

  // REAL download/forge via store_download + store:progress events.
  // Items without a real StoreItem behind them (dictionary packs) stay honest.
  P._storeGet = function (item) {
    if (!item || !item.item) {
      this._toast(String(item && item.id).indexOf('dict-') === 0
        ? 'DICTIONARY PACKS NEED A PACK SERVER — WORDNET STAYS BUILT-IN & OFFLINE'
        : 'THAT SOURCE ISN’T REACHABLE FROM THIS DEVICE');
      return;
    }
    const id = item.id;
    const set = patch => this.setState(s => {
      const o = Object.assign({}, s.storeSt);
      o[id] = Object.assign({}, o[id] || {}, patch);
      return { storeSt: o };
    });
    set({ status: 'dl', pct: 0 });
    V.call('store_download', { item: item.item }).then(() => {
      set({ status: 'done', pct: 100 });
      this._toast(item.title + ' → YOUR SHELF · LEDGER SEALED');
      this._refreshBooks();
    }).catch(e => {
      set({ status: 'idle', pct: 0 });
      this._honest(item.title, e);
    });
  };

  /* ---------------- companion: the real 5-stage engine ---------------- */
  P._send = function (forcedText) {
    const st = this.state;
    if (this._busy) return;
    const text = String(forcedText != null ? forcedText : st.input).trim();
    if (!text) return;
    const meta = this._curBookMeta();
    if (!meta) { this._toast('NO BOOK OPEN'); return; }
    const charId = st.char;
    const chats = Object.assign({}, st.chats);
    const list = (chats[charId] || []).slice();
    list.push({ user: true, text });
    chats[charId] = list;
    this._busy = true;
    const turnId = ++this._turnSeq;
    this._turnId = turnId;
    this.setState({ chats, input: '', phase: 'gate', stream: '' });
    const cid = typeof charId === 'number' ? charId : null;
    V.call('companion_turn', { bookId: meta.id, characterId: cid, message: text, turnId })
      .then(rep => {
        if (this._turnId !== turnId) return;
        const c2 = Object.assign({}, this.state.chats);
        const l2 = (c2[charId] || []).slice();
        l2.push({ bot: true, text: rep.reply, shield: !!(rep.repaired || rep.redacted), redacted: !!rep.redacted });
        c2[charId] = l2;
        this._busy = false;
        this.setState({ chats: c2, phase: null, stream: '' });
        if (rep.redacted) this._toast('A LINE WAS INKED OUT — THE GATE HELD');
      })
      .catch(e => {
        if (this._turnId !== turnId) return;
        this._busy = false;
        this.setState({ phase: null, stream: '' });
        this._honest('THE CAST IS SILENT', e);
      });
  };

  P._pinTheory = function (text) {
    const meta = this._curBookMeta();
    if (!meta || !text) return;
    V.call('add_theory', { bookId: meta.id, text: String(text).slice(0, 240) }).then(th => {
      this._vena.theories = this._vena.theories.concat([th]);
      this.setState({});
      this._toast('PINNED AT CH.' + this.roman(th.logged_at_chapter || 1) + ' — IT TURNS WHEN THE STORY DOES');
    }).catch(e => this._honest('PIN FAILED', e));
  };

  /* ---------------- recap: real get_recap, design typing effect ---------------- */
  P._streamRecap = function () {
    if (this._recapBusy) return;
    const meta = this._curBookMeta();
    if (!meta) return;
    this._recapBusy = true;
    V.call('get_recap', { bookId: meta.id }).then(txt => {
      const words = String(txt || '').split(' ');
      let i = 0;
      const step = () => {
        i += 2;
        this.setState({ recap: words.slice(0, i).join(' ') });
        if (i < words.length) this._later(step, 42);
        else { this._recapBusy = false; this.setState({ recapDone: true }); }
      };
      this._later(step, 300);
    }).catch(e => {
      this._recapBusy = false;
      this.setState({ recapOpen: false, recap: '', recapDone: false });
      this._honest('NO RECAP', e);
    });
  };

  /* ---------------- model downloads: real, driven by model:progress ---------------- */
  P._startDl = function (tier) {
    if (this._dlBusy) return;
    this._dlBusy = true;
    const t = tier || 'quill';
    this.setState({ dl: { status: 'downloading', pct: 0, tier: t } });
    V.call('download_local_model', { tier: t }).then(() => {
      this._dlBusy = false;
      this.setState({ dl: { status: 'done', pct: 100, tier: t } });
      this._toast('MODEL DOWNLOADED — READY TO SPEAK');
      this._loadSettings();
    }).catch(e => {
      this._dlBusy = false;
      this.setState({ dl: { status: 'idle', pct: 0 } });
      this._honest('MODEL DOWNLOAD FAILED', e);
    });
  };

  /* paint weights: real download_paint_model, progress via model:progress kind:'paint' */
  P._startPaintDl = function (tier) {
    if (this._paintDlBusy) return;
    this._paintDlBusy = true;
    this.setState({ paintDl: { status: 'downloading', pct: 0, tier } });
    V.call('download_paint_model', { tier }).then(r => {
      this._paintDlBusy = false;
      this.setState({ paintDl: { status: 'done', pct: 100, tier } });
      this._toast(r && r.engine_present === false
        ? 'WEIGHTS INSTALLED — ALSO INSTALL stable-diffusion.cpp (sd CLI) TO PAINT LOCALLY'
        : ((r && r.brand) || 'PAINT MODEL') + ' INSTALLED — READY TO PAINT');
      this._loadPaint();
    }).catch(e => {
      this._paintDlBusy = false;
      this.setState({ paintDl: { status: 'idle', pct: 0 } });
      this._honest('PAINT MODEL DOWNLOAD FAILED', e);
    });
  };

  /* ---------------- real file import (BROWSE FILES) ----------------
   * Tauri: native dialog → import_book(path). Browser: hidden <input type=file>
   * → base64 → import_book_data. forge:progress events drive the existing bars.
   */
  P._venaBookInput = function () {
    if (this.__venaBookEl) return this.__venaBookEl;
    const inp = document.createElement('input');
    inp.type = 'file';
    inp.accept = '.epub,.txt,.cbz';
    inp.style.display = 'none';
    inp.setAttribute('data-vena-book-input', '1');
    inp.addEventListener('change', () => {
      const f = inp.files && inp.files[0];
      inp.value = '';
      if (!f) return;
      const r = new FileReader();
      r.onload = () => {
        const b64 = String(r.result || '').split(',')[1] || '';
        if (!b64) { this._toast('COULD NOT READ THAT FILE'); return; }
        this._toast('FORGING — THE LEDGER IS BEING WRITTEN');
        V.call('import_book_data', { name: f.name, data: b64 }).then(meta => {
          this._toast('LEDGER FORGED ✓ — ' + UP(meta.title || 'BOOK') + ' IS ON THE SHELF');
          this._refreshBooks();
        }).catch(e => this._honest('IMPORT FAILED', e));
      };
      r.onerror = () => this._toast('COULD NOT READ THAT FILE');
      r.readAsDataURL(f);
    });
    document.body.appendChild(inp);
    this.__venaBookEl = inp;
    return inp;
  };

  P._venaBrowseBook = function () {
    const T = window.__TAURI__;
    if (T) {
      const options = { multiple: false, filters: [{ name: 'Books', extensions: ['epub', 'txt', 'cbz'] }] };
      const opened = (T.dialog && T.dialog.open)
        ? T.dialog.open(options)
        : T.core.invoke('plugin:dialog|open', { options });
      Promise.resolve(opened).then(sel => {
        const path = Array.isArray(sel) ? sel[0] : sel;
        if (!path) return;
        this._toast('FORGING — THE LEDGER IS BEING WRITTEN');
        return V.call('import_book', { path: String(path) }).then(meta => {
          this._toast('LEDGER FORGED ✓ — ' + UP(meta.title || 'BOOK') + ' IS ON THE SHELF');
          this._refreshBooks();
        });
      }).catch(e => this._honest('IMPORT FAILED', e));
      return;
    }
    this._venaBookInput().click();
  };

  /* ---------------- real CBZ pages (comic profile) ----------------
   * get_manga_pages → count; get_manga_page → base64, lazily for the visible
   * spread only, cached per page. Prose books keep the design's demo view.
   */
  P._loadManga = function () {
    const books = (this._vena && this._vena.books) || [];
    const meta = books.find(b => b.profile === 'comic' && b.slug === this.state.book)
      || books.find(b => b.profile === 'comic');
    if (!meta) { this._manga = null; return; }
    if (this._manga && this._manga.bookId === meta.id) return;
    V.call('get_manga_pages', { bookId: meta.id }).then(r => {
      const count = (r && r.count) | 0;
      this._manga = count > 0
        ? { bookId: meta.id, title: UP(meta.title), count, cache: {}, pending: {} }
        : null;
      this.setState(this._manga ? { mangaPage: 1 } : {});
    }).catch(e => { this._manga = null; this._honest('CBZ PAGES', e); });
  };

  P._fetchMangaPage = function (n) {
    const M = this._manga;
    if (!M || M.pending[n] || M.cache[n] || n < 1 || n > M.count) return;
    M.pending[n] = true;
    V.call('get_manga_page', { bookId: M.bookId, page: n }).then(r => {
      M.pending[n] = false;
      if (this._manga !== M || !r || !r.data) return;
      M.cache[n] = 'data:' + (r.mime || 'image/jpeg') + ';base64,' + r.data;
      this.setState({});
    }).catch(e => { M.pending[n] = false; this._honest('PAGE ' + n, e); });
  };

  P._venaMangaDom = function () {
    if (!this.state.mangaOpen) return;
    const M = this._manga;
    const spans = Array.prototype.slice.call(document.querySelectorAll('span, div'));
    spans.forEach(el => {
      if (el.__vMgO == null) el.__vMgO = el.textContent || '';
      // header title + footer status only change for a REAL comic
      if (!M) return;
      if (el.tagName === 'SPAN' && el.__vMgO.indexOf('LITTLE NEMO') === 0 && el.textContent !== M.title) {
        el.textContent = M.title;
      } else if (el.__vMgO.indexOf('PLACEHOLDER PAGES') === 0) {
        const want = 'REAL CBZ · ' + M.count + ' PAGES FROM YOUR FILE · TAP A BUBBLE → DICTIONARY · TRANSLATE · ASK THE CAST';
        if (el.textContent !== want) el.textContent = want;
      }
    });
    if (!M) return; // prose: the design's demo view stays untouched
    // page panels: the stroke "P.N" spans mark each page plate
    spans.forEach(el => {
      if (el.tagName !== 'SPAN' || !/^P\.\d+$/.test(el.textContent || '')) return;
      const n = +((el.textContent || '').slice(2));
      const panel = el.parentElement;
      if (!panel) return;
      let img = panel.querySelector('img[data-vena-manga]');
      if (!img) {
        img = document.createElement('img');
        img.setAttribute('data-vena-manga', '1');
        img.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;object-fit:contain;display:none;image-rendering:pixelated';
        panel.appendChild(img);
      }
      img.alt = 'Comic page ' + n;
      const grid = panel.querySelector('div');
      if (grid) grid.style.visibility = 'hidden'; // demo panel grid never shows on a real comic
      const src = n >= 1 && n <= M.count ? M.cache[n] : null;
      if (src) {
        if (img.getAttribute('src') !== src) img.setAttribute('src', src);
        img.style.display = 'block';
        el.style.visibility = 'hidden';
      } else {
        img.style.display = 'none';
        el.style.visibility = 'visible'; // the page number doubles as the loading mark
        if (n >= 1 && n <= M.count) this._fetchMangaPage(n);
      }
    });
  };

  /* vision forge: no backend command exists — keep the panel, stay honest */
  P._visionRun = function () {
    this._toast('VISION FORGE NEEDS CLOUD RELAY + AN OCR MODEL — NOT WIRED ON THIS DEVICE');
  };

  /* ---------------- full-spoiler archive from the real wiki ---------------- */
  P._loadFullWiki = function (meta, quiet) {
    return V.call('get_wiki_index', { bookId: meta.id, mode: 'full' }).then(idx => {
      const groups = {};
      (idx.entries || []).forEach(e => { (groups[e.group] = groups[e.group] || []).push(e); });
      const labels = { people: 'CHARACTER FATES', places: 'PLACES', terms: 'TERMS & THINGS' };
      let infobox = null;
      const secs = [];
      const jobs = [];
      Object.keys(groups).forEach(g => {
        const sec = { id: g, label: labels[g] || UP(g), entries: [] };
        secs.push(sec);
        groups[g].forEach(e => {
          jobs.push(V.call('get_wiki_page', { bookId: meta.id, entityId: e.id, mode: 'full' }).then(pg => {
            const facts = [].concat.apply([], (pg.sections || []).map(s => s.facts || []));
            sec.entries.push({
              head: UP(e.name),
              body: facts.length ? facts.join(' ') : 'The ledger holds nothing on this page.',
              stamp: chapterSpan(facts) || (e.fact_count + ' FACTS'),
              _n: e.fact_count
            });
            if (!infobox || e.fact_count > infobox._n) {
              infobox = {
                _n: e.fact_count,
                title: UP(e.name) + ' — UNSEALED',
                rows: (pg.sections || []).slice(0, 5).map(s => ({
                  k: UP(s.heading),
                  v: (s.facts || []).map(stripStamp).join(' ')
                }))
              };
            }
          }).catch(() => {}));
        });
      });
      return Promise.all(jobs).then(() => {
        secs.forEach(s => s.entries.sort((a, b) => b._n - a._n));
        this.wiki[meta.slug] = { infobox, sections: secs, _idx: idx };
        const w = Object.assign({}, this.state.wikiUnlocked);
        w[meta.slug] = true;
        this.setState({ wikiUnlocked: w, wikiArmed: false, wikiSection: (secs[0] || {}).id });
        if (!quiet) this._toast('THE ARCHIVE IS OPEN — NOTHING BELOW IS SAFE');
      });
    }).catch(e => {
      if (!quiet) this._honest('ARCHIVE', e);
    });
  };

  /* ---------------- backend events → the design's animations ---------------- */
  P._venaEvent = function (e) {
    const p = e.payload || {};
    switch (e.name) {
      case 'companion:stage': {
        if (!this._busy || p.turnId !== this._turnId) return;
        const map = { gate: 'gate', compose: 'gen', verify: 'verify', repair: 'repair' };
        this.setState({ phase: map[p.stage] || 'gen' });
        return;
      }
      case 'model:progress':
        if (p.kind === 'paint') {
          if ((this.state.paintDl || {}).status === 'downloading') {
            this.setState(s => ({ paintDl: Object.assign({}, s.paintDl, { pct: p.pct | 0 }) }));
          }
        } else if (this.state.dl.status === 'downloading') {
          this.setState(s => ({ dl: Object.assign({}, s.dl, { pct: p.pct | 0 }) }));
        }
        return;
      case 'store:progress': {
        const id = p.jobId;
        if (id == null) return;
        this.setState(s => {
          if ((s.storeSt[id] || {}).status === 'done') return null;
          const o = Object.assign({}, s.storeSt);
          o[id] = { status: p.phase === 'forge' ? 'forge' : 'dl', pct: p.pct | 0 };
          return { storeSt: o };
        });
        return;
      }
      case 'forge:progress': {
        const prev = this._vena.forge[p.bookId] || {};
        const ft = p.forgedThrough != null ? (p.forgedThrough | 0) : (prev.forgedThrough | 0);
        this._vena.forge[p.bookId] = { pct: p.pct | 0, stage: p.stage, forgedThrough: ft };
        this.setState({ forgePct: p.pct | 0, forgedThrough: ft });
        return;
      }
      case 'forge:done':
        delete this._vena.forge[p.bookId];
        this._refreshBooks();
        return;
      default:
        return;
    }
  };

  /* ---------------- reader: real chapter text into the fixed paragraphs ----------------
   * The template's five reader <p> elements are verbatim design HTML (no
   * bindings), so the real episode text is written into their TEXT NODES after
   * each render. Element children are never added or removed (React keeps
   * owning them); inline demo name-chips are hidden, absolutely-positioned
   * controls (the ✎ margin-note button) are kept.
   */
  P._venaReaderText = function () {
    if (this.state.screen !== 'reader') return;
    const ep = this._vena && this._vena.episode;
    if (!ep || !ep.paras.length) return;
    const root = document.querySelector('.vhub');
    if (!root) return;
    const ps = Array.prototype.slice.call(root.querySelectorAll('p'));
    if (!ps.length) return;
    ps.forEach((p, i) => {
      const txt = i < ps.length - 1
        ? (ep.paras[i] || '')
        : ep.paras.slice(ps.length - 1).join('  ').slice(0, 1600);
      if (!txt || p.__venaTxt === txt) return;
      p.__venaTxt = txt;
      let first = true;
      let dropcap = null;
      Array.prototype.forEach.call(p.childNodes, node => {
        if (node.nodeType === 3) {
          node.nodeValue = first ? (i === 0 ? txt.slice(1) : txt) : '';
          first = false;
        } else if (node.nodeType === 1) {
          const cs = node.getAttribute('style') || '';
          if (i === 0 && !dropcap && /float:\s*left/.test(cs)) {
            dropcap = node;
            node.textContent = txt.slice(0, 1);
          } else if (cs.indexOf('position:absolute') === -1) {
            node.style.display = 'none'; // inline demo name-chips
          }
        }
      });
    });
  };

  /* ---------------- static demo copy → real ledger data ----------------
   * A few plates in the canonical template are hardcoded showcase HTML with
   * no bindings (the theory flip card, the DATA & PRIVACY ledger line, the
   * WHO'S WHO tally, the reader kicker). Their text nodes are rewritten from
   * REAL data after each render — same reconciliation-safe technique as the
   * reader text; the design's markup and styling are never altered.
   */
  P._venaDesignFacts = function () {
    const D = this._vena;
    if (!D) return;
    const meta = this._curBookMeta();
    const els = Array.prototype.slice.call(document.querySelectorAll('span, div'));
    const orig = el => (el.__vOrig != null ? el.__vOrig : (el.__vOrig = el.textContent));
    const put = (el, txt) => { if (el.__vTxt !== txt) { el.__vTxt = txt; el.textContent = txt; } };

    els.forEach(el => {
      /* reader kicker (has an <em> child) → the real episode's own heading */
      if (el.children.length) {
        const f = el.firstChild;
        if (el.__vKick || (f && f.nodeType === 3 && f.nodeValue.indexOf('DR. SEWARD') === 0)) {
          el.__vKick = true;
          const ep = D.episode;
          if (ep && ep.title && f && f.nodeType === 3) {
            const want = UP(ep.title) + ' — ';
            if (f.nodeValue !== want) f.nodeValue = want;
          }
        }
        return;
      }
      const o = orig(el);

      /* theory flip card → the first ledger-CONFIRMED theory, or hidden */
      if (o === 'REVEAL REACHED!!' && el.tagName === 'SPAN') {
        let card = el.parentElement;
        while (card && (card.getAttribute('style') || '').indexOf('perspective') === -1) card = card.parentElement;
        if (!card) return;
        const th = (D.theories || []).find(t => t.resolved_status === 'confirmed');
        card.style.display = th ? '' : 'none';
        if (!th) return;
        Array.prototype.forEach.call(card.querySelectorAll('span, div'), c => {
          if (c.children.length) return;
          const co = orig(c);
          if (/^PINNED CH\./.test(co)) put(c, 'PINNED CH.' + this.roman(th.logged_at_chapter || 1));
          else if (/ran aground are connected/.test(co)) put(c, '“' + th.text + '”');
          else if (/^RESOLVED · CHAPTER/.test(co)) put(c, 'RESOLVED · CHAPTER ' + this.roman(th.resolved_at_chapter || th.logged_at_chapter || 1));
          else if (/^The pieces met in Chapter/.test(co)) put(c, 'The story caught up at Chapter ' + this.roman(th.resolved_at_chapter || th.logged_at_chapter || 1) + ' — confirmed by the ledger, never by guesswork.');
        });
        return;
      }

      /* DATA & PRIVACY ledger line → the real book's numbers */
      if (/^LEDGER SHA A3F2/.test(o)) {
        put(el, meta
          ? 'LEDGER SHA ' + shaShort(meta.package_sha) + ' · ' + meta.fact_count + ' FACTS · COVERAGE ' + Math.round((meta.ledger_coverage || 0) * 100) + '%'
          : 'NO BOOK OPEN');
        return;
      }

      /* WHO'S WHO tally → the real synced index */
      if (o === '13 ENTRIES · 4 SEALED') {
        const idx = D.wikiIdx;
        if (idx) put(el, (idx.entries || []).length + ' ENTRIES · ' + (idx.sealed_total || 0) + ' SEALED');
      }
    });
  };

  /* ---------------- portable-data layer (SETTINGS ▸ DATA & PRIVACY) --------
   * The canonical DATA & PRIVACY plate is static markup with only exportData /
   * wipeBook anchors. We add the portable-data actions (sync export/import,
   * per-book theory share, forget conversations) by injecting buttons into the
   * design's own button row after render — reusing the plate's exact button
   * style so it matches the house style on this platform. Idempotent + self-
   * healing (re-injects if the framework ever re-renders the row). The template
   * is never edited; no new template anchors are introduced.
   */
  P._venaDownload = function (filename, obj) {
    try {
      const blob = new Blob([JSON.stringify(obj, null, 2)], { type: 'application/json' });
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      setTimeout(() => { try { URL.revokeObjectURL(a.href); a.remove(); } catch (e) {} }, 0);
      return true;
    } catch (err) { return false; }
  };

  P._venaExportSync = function () {
    if (!V) { this._toast('VENA BRIDGE MISSING — NO BACKEND ON THIS PAGE'); return; }
    V.call('export_bundle', { scope: 'sync' }).then(bundle => {
      const n = ((bundle && bundle.books) || []).length;
      if (this._venaDownload('vena-sync.json', bundle)) {
        this._toast('EXPORTED · ' + n + ' BOOK' + (n === 1 ? '' : 'S') + ' · YOUR DATA, YOUR FILE');
      } else this._toast('EXPORT FAILED — COULD NOT WRITE THE FILE');
    }).catch(e => this._honest('EXPORT FAILED', e));
  };

  P._venaImportInput = function () {
    if (this.__venaImportEl) return this.__venaImportEl;
    const inp = document.createElement('input');
    inp.type = 'file';
    inp.accept = '.json,application/json';
    inp.style.display = 'none';
    inp.addEventListener('change', () => {
      const f = inp.files && inp.files[0];
      inp.value = '';
      if (!f) return;
      const r = new FileReader();
      r.onload = () => this._venaImportText(String(r.result || ''));
      r.onerror = () => this._toast('COULD NOT READ THAT FILE');
      r.readAsText(f);
    });
    document.body.appendChild(inp);
    this.__venaImportEl = inp;
    return inp;
  };
  P._venaImportPick = function () { this._venaImportInput().click(); };
  P._venaImportText = function (text) {
    if (!V) { this._toast('VENA BRIDGE MISSING — NO BACKEND ON THIS PAGE'); return; }
    V.call('import_bundle', { json: text }).then(rep => {
      rep = rep || {};
      const parts = ['SYNCED'];
      const mb = rep.matched_books || 0;
      parts.push(mb + ' BOOK' + (mb === 1 ? '' : 'S'));
      if (rep.progress_updated) parts.push(rep.progress_updated + ' PROGRESS');
      const ta = rep.theories_added || 0;
      parts.push(ta + ' THEOR' + (ta === 1 ? 'Y' : 'IES') + ' ADDED');
      const skipped = rep.skipped_not_on_shelf || [];
      if (skipped.length) parts.push(skipped.length + ' NOT ON SHELF');
      this._toast(parts.join(' · ').slice(0, 88));
      this._refreshBooks();
      const meta = this._curBookMeta();
      if (meta) this._loadTheories(meta);
    }).catch(e => this._honest('IMPORT FAILED', e));
  };

  P._venaShareTheories = function () {
    const meta = this._curBookMeta();
    if (!meta) { this._toast('NO BOOK OPEN — NOTHING TO SHARE'); return; }
    if (!V) { this._toast('VENA BRIDGE MISSING — NO BACKEND ON THIS PAGE'); return; }
    V.call('export_bundle', { bookId: meta.id, scope: 'theories' }).then(bundle => {
      const b = ((bundle && bundle.books) || [])[0] || {};
      const n = (b.theories || []).length;
      if (this._venaDownload('vena-theories-' + meta.slug + '.json', bundle)) {
        this._toast('SHARED · ' + n + ' THEOR' + (n === 1 ? 'Y' : 'IES') + ' · PASS IT ROUND THE BOOK CLUB');
      } else this._toast('SHARE FAILED — COULD NOT WRITE THE FILE');
    }).catch(e => this._honest('SHARE FAILED', e));
  };

  P._venaForgetChats = function () {
    const meta = this._curBookMeta();
    if (!meta) { this._toast('NO BOOK OPEN'); return; }
    if (!V) { this._toast('VENA BRIDGE MISSING — NO BACKEND ON THIS PAGE'); return; }
    if (this.__venaForgetArmed !== meta.id) {
      this.__venaForgetArmed = meta.id;
      this._toast('TAP AGAIN TO FORGET EVERY CONVERSATION FOR THIS BOOK');
      setTimeout(() => { if (this.__venaForgetArmed === meta.id) this.__venaForgetArmed = null; }, 4000);
      return;
    }
    this.__venaForgetArmed = null;
    V.call('forget_conversations', { bookId: meta.id }).then(() => {
      this._toast('CONVERSATIONS FORGOTTEN — THE BOOK, LEDGER & THEORIES REMAIN');
    }).catch(e => this._honest('FORGET FAILED', e));
  };

  P._venaDataPrivacy = function () {
    const btns = Array.prototype.slice.call(document.querySelectorAll('button'));
    let exportBtn = null, burnBtn = null;
    for (let i = 0; i < btns.length; i++) {
      const t = (btns[i].textContent || '').trim();
      if (!exportBtn && t.indexOf('EXPORT THEORIES') === 0) exportBtn = btns[i];
      if (!burnBtn && t === "BURN THIS BOOK'S DATA") burnBtn = btns[i];
    }
    const anchor = exportBtn || burnBtn;
    if (!anchor) return;
    const row = anchor.parentElement;
    if (!row) return;
    const meta = this._curBookMeta();

    if (!row.querySelector('[data-vena-dp="share"]')) {
      const inkStyle = (exportBtn || burnBtn).getAttribute('style') || '';
      const redStyle = (burnBtn || exportBtn).getAttribute('style') || '';
      const cyanStyle = inkStyle.replace(/var\(--ink\)/g, 'var(--cyan)');
      const mk = (label, style, key, onClick) => {
        const b = document.createElement('button');
        b.textContent = label;
        b.setAttribute('style', style);
        b.setAttribute('data-vena-dp', key);
        b.addEventListener('click', onClick);
        row.appendChild(b);
        return b;
      };
      mk('EXPORT MY DATA', inkStyle, 'exportSync', () => this._venaExportSync());
      mk('IMPORT', inkStyle, 'import', () => this._venaImportPick());
      mk('SHARE THEORIES', cyanStyle, 'share', () => this._venaShareTheories());
      mk('FORGET OUR CONVERSATIONS', redStyle, 'forget', () => this._venaForgetChats());
    }
    // per-book actions follow the current book; dim when the shelf is empty
    const has = !!meta;
    ['share', 'forget'].forEach(key => {
      const b = row.querySelector('[data-vena-dp="' + key + '"]');
      if (!b) return;
      b.disabled = !has;
      b.style.opacity = has ? '1' : '.4';
      b.style.cursor = has ? 'pointer' : 'not-allowed';
    });
  };

  /* =================================================================
   *  _venaTail — the renderVals tail (everything past the truncation),
   *  rebuilt in the design's exact shapes, on REAL data.
   * ================================================================= */
  P._venaTail = function (L) {
    const st = L.st, ch = L.ch, chRoman = L.chRoman, go = L.go;
    const D = this._vena || { books: [], theories: [], forge: {} };
    const S = D.settings;
    const ab = L.ab;
    const meta = ab && ab.meta ? ab.meta : null;
    const total = ab ? ab.total : 27;
    const compReadyR = !!ab && ab.status === 'forged';
    const compFreshR = !!ab && ab.status === 'fresh';
    const compForgingR = !!ab && (ab.status === 'forging' || ab.status === 'raw');
    const compEmptyR = !ab;
    // streaming forge: while a book forges chapter-by-chapter, the companion is
    // usable for chapters already committed (<= forgedThrough) or once any facts
    // exist — driven by the real forgedThrough from forge:progress.
    const activeForge = ab ? D.forge[ab.id] : null;
    const forgedThrough = activeForge && activeForge.forgedThrough != null
      ? (activeForge.forgedThrough | 0)
      : (typeof st.forgedThrough === 'number' ? st.forgedThrough : 0);
    const partialReadyR = compForgingR && !compReadyR
      && (forgedThrough >= ch || !!(meta && (meta.fact_count | 0) > 0));
    const unlocked = !!(ab && st.wikiUnlocked[ab.id]);
    const wikiData = ab ? this.wiki[ab.id] : null;
    const curSec = wikiData ? (wikiData.sections.find(x => x.id === st.wikiSection) || wikiData.sections[0]) : null;
    const artMap = this.artMap || {}, bustMap = this.bustMap || {};
    const aiBadge = D.ai ? (D.ai.ready ? UP(D.ai.model) : 'NO ENGINE') : '…';
    const noop = () => {};

    /* ---- real library shelf ---- */
    const defs = (this.bookDefs || []).filter(b => !st.deleted[b.id]);
    const mkTicksN = (readCh, n) => Array.from({ length: Math.max(1, n) }, (_, i) => {
      const k = i + 1, read = k <= readCh, now = k === readCh && readCh > 0;
      return {
        tip: 'Chapter ' + k,
        bg: now ? 'var(--cyan)' : read ? 'var(--red)' : 'var(--mut2)',
        op: now ? '1' : read ? '.85' : '.25',
        anim: 'none'
      };
    });
    const books = defs.map(b => {
      const m = b.meta || {};
      const fsx = D.forge[m.id];
      const forging = b.status === 'forging' || !!fsx;
      const prog = b.id === st.book ? ch : (m.progress_episode || 0);
      const openComp = () => { this._selectBook(b); this.setState({ screen: 'companion', chatOpen: false }); };
      const forgeIt = () => {
        this._toast('FORGING — EVERY FACT GETS ITS CHAPTER STAMP');
        this._vena.forge[m.id] = { pct: 0 };
        this.setState({});
        V.call('forge_ledger', { bookId: m.id }).then(() => this._refreshBooks())
          .catch(err => { delete this._vena.forge[m.id]; this.setState({}); this._honest('FORGE FAILED', err); });
      };
      const sealed = b.status === 'forged' || b.status === 'fresh';
      return {
        id: b.id, title: b.title, author: b.author, cover: b.cover,
        badge: sealed ? 'LEDGER FORGED ✓' : forging ? 'FORGING…' : 'RAW EPUB',
        badgeBg: forging ? 'var(--red)' : 'transparent',
        badgeCol: sealed ? 'var(--cyan)' : forging ? '#fff' : 'var(--mut)',
        badgeBrd: sealed ? 'var(--cyan)' : forging ? 'var(--red)' : 'var(--mut2)',
        isForged: sealed && !forging, isForging: forging,
        stats: b.stats,
        ticks: mkTicksN(prog, b.total),
        posLabel: prog > 0
          ? 'CH. ' + prog + ' OF ' + b.total + ' · ' + Math.round(prog / b.total * 100) + '% · ' + D.theories.length + ' THEORIES PINNED'
          : 'NOT STARTED · ' + b.total + ' CHAPTERS AHEAD OF YOU',
        facts: fsx ? UP(fsx.stage || '…') : '…',
        forgeW: (fsx ? fsx.pct : 0) + '%', forgePct: fsx ? fsx.pct : 0,
        btnLabel: forging ? 'FORGING…' : sealed ? (prog > 0 ? 'RESUME →' : 'BEGIN →') : 'FORGE THE LEDGER',
        btnBg: forging ? 'transparent' : prog > 0 ? 'var(--ink)' : 'var(--red)',
        btnCol: forging ? 'var(--mut2)' : prog > 0 ? 'var(--inv)' : '#fff',
        btnShdw: forging ? 'none' : prog > 0 ? '4px 4px 0 var(--red)' : '4px 4px 0 var(--ink)',
        btnCur: forging ? 'default' : 'pointer', btnOp: forging ? '.6' : '1',
        act: forging ? (() => this._toast('STILL FORGING — THE COMPANION WAKES WHEN THE LEDGER SEALS'))
          : sealed ? openComp : forgeIt,
        goComp: openComp,
        del: () => this.setState({ delModal: b.id })
      };
    });

    /* ---- cast / silhouettes (met = the real ledger's word) ---- */
    const met = L.met, unmet = L.unmet;
    const charObj = L.charObj || { id: null, name: 'The Narrator', short: 'the narrator', init: 'N', role: 'Voice of the ledger', bio: '' };
    const cKey = keyOf(charObj.name);
    const cast = met.slice(0, 8).map(c => ({
      open: () => this.setState({ chatOpen: true, char: c.id, input: '' }),
      bg: artMap[keyOf(c.name)] || 'radial-gradient(circle at 45% 22%, rgba(226,120,60,.35), transparent 55%), linear-gradient(162deg, #33261c, #120c08)',
      gen: false, plain: true, slotId: '', slotPh: '',
      init: c.init, nameUp: UP(c.name), metR: this.roman(c.metCh)
    }));

    /* ---- theories: resolution ONLY from backend fields ---- */
    const rots = ['rotate(.8deg)', 'rotate(-.9deg)', 'rotate(.5deg)', 'rotate(-.6deg)', 'rotate(1deg)'];
    const ths = (D.theories || []).map((t, i) => {
      const open = !t.resolved_status;
      const conf = t.resolved_status === 'confirmed';
      return {
        text: t.text, rot: rots[i % rots.length],
        stamped: !open, stamp: conf ? 'CALLED IT' : 'BUSTED',
        sc: conf ? 'var(--cyan)' : 'var(--red)',
        tw: open ? '100%' : '72%',
        head: open ? 'PINNED CH.' + this.roman(t.logged_at_chapter || 1)
          : 'RESOLVED CH.' + this.roman(t.resolved_at_chapter || t.logged_at_chapter || 1),
        foot: open ? 'turns when the story does — no hints' : 'by the ledger'
      };
    });
    const addTheoryFn = () => {
      const t = String(st.newTheory || '').trim();
      if (!t) { this._toast('WRITE THE THEORY FIRST'); return; }
      if (!meta) return;
      V.call('add_theory', { bookId: meta.id, text: t }).then(th => {
        this._vena.theories = this._vena.theories.concat([th]);
        this.setState({ newTheory: '' });
        this._toast('PINNED AT CH.' + this.roman(th.logged_at_chapter || ch) + ' — NO HINTS UNTIL THE STORY TURNS');
      }).catch(e => this._honest('PIN FAILED', e));
    };

    /* ---- chat ---- */
    const chatList = st.chats[st.char] || [];
    const msgs = chatList.map(m => ({
      user: !!m.user, bot: !!m.bot, text: m.text, shield: !!m.shield,
      pin: () => this._pinTheory(m.text),
      report: () => this.setState({ leakOpen: true, leakMsg: m, leakReason: 'future_event' })
    }));
    const chips = [
      'Remind me — where do things stand right now?',
      'What do you fear most, as of this chapter?'
    ].map(q => ({ label: q, send: () => this._send(q) }));

    const byName = frag => (this.fullCast || []).find(c => String(c.name).toLowerCase().includes(frag));
    const whoObj = L.whoObj;
    const whoKey = whoObj ? keyOf(whoObj.name) : null;

    /* ---- settings: models (real tiers), gate, relay ---- */
    const tierDescs = {
      ink: 'Fast and sure-footed. Chat, recaps and theory checks on this device.',
      quill: 'Richer, more period-true voices. Roughly twice the wait — worth it for chat.',
      arch: 'The scholar. Too heavy for this device — shown so you know it exists.'
    };
    const tiers = (S && S.tiers) || [];
    const models = tiers.map(t => {
      const installed = !!(S && S.local_ready && S.local_model === t.brand);
      const blocked = t.min_ram_gb > 8;
      const active = installed && S && S.default_chat_mode === 'local';
      const downloading = st.dl.status === 'downloading' && st.dl.tier === t.id;
      const activate = () => {
        V.call('set_chat_mode', { mode: 'local' }).then(() => {
          this._loadSettings();
          this._toast(t.brand + ' NOW ANSWERS FOR THE CAST');
        }).catch(e => this._honest('ACTIVATE', e));
      };
      return {
        id: t.id, chip: t.chip, name: t.brand,
        size: t.size_gb.toFixed(1) + ' GB' + (installed ? ' · INSTALLED' : blocked ? ' · NEEDS ' + t.min_ram_gb + ' GB RAM' : ''),
        desc: tierDescs[t.id] || '',
        active,
        op: blocked ? '.45' : '1',
        shdw: active ? '5px 5px 0 var(--cyan)' : '3px 3px 0 var(--shdw)',
        cur: installed && !active ? 'pointer' : 'default',
        chipBg: active ? 'var(--cyan)' : 'var(--ink)', chipCol: 'var(--inv)',
        downloading, dlW: (st.dl.pct | 0) + '%', dlPct: st.dl.pct | 0,
        btnLabel: blocked ? 'TOO BIG' : active ? 'ACTIVE' : installed ? 'ACTIVATE' : downloading ? 'DOWNLOADING…' : 'DOWNLOAD',
        btnBg: active ? 'var(--cyan)' : 'transparent',
        btnCol: active ? 'var(--inv)' : blocked ? 'var(--mut2)' : 'var(--ink)',
        btnCur: blocked ? 'default' : 'pointer',
        pick: () => { if (installed && !active) activate(); },
        btnAct: e => {
          e && e.stopPropagation && e.stopPropagation();
          if (blocked || active || downloading) return;
          if (installed) activate(); else this._startDl(t.id);
        }
      };
    });

    /* paint tiers: real catalog (paint_tiers), real downloads, real installs.
     * get_image_status reports tier 'desktop' when models/paint merely exists;
     * only trust it once a tier's weights are actually installed. */
    const img = D.image;
    const anyPaintInstalled = (D.paintTiers || []).some(t => t.installed);
    const hasPaint = !!(img && (img.tier === 'api' || (img.tier === 'desktop' && anyPaintInstalled)));
    const pDl = st.paintDl || {};
    const paintDescs = {
      sketch: 'Stable Diffusion 1.5 — quick covers and portraits on this device.',
      easel: 'SDXL base — richer paint, heavier download. Worth it on a desktop.'
    };
    const paintModels = (D.paintTiers || []).map(t => {
      const installed = !!t.installed;
      const downloading = pDl.status === 'downloading' && pDl.tier === t.id;
      return {
        id: 'paint-' + t.id,
        chip: t.id === 'sketch' ? '1.5' : 'XL',
        name: t.brand,
        size: t.size_gb.toFixed(1) + ' GB' + (installed ? ' · INSTALLED' : ''),
        desc: paintDescs[t.id] || '',
        active: false,
        op: '1', cur: 'default',
        shdw: installed ? '3px 3px 0 var(--cyan)' : '3px 3px 0 var(--shdw)',
        chipBg: installed ? 'var(--cyan)' : 'var(--ink)', chipCol: 'var(--inv)',
        downloading, dlW: (downloading ? pDl.pct | 0 : 0) + '%', dlPct: downloading ? pDl.pct | 0 : 0,
        btnLabel: installed ? 'INSTALLED ✓' : downloading ? 'DOWNLOADING…' : 'DOWNLOAD',
        btnAria: t.brand + (installed ? ' — installed' : ' — download'),
        btnBg: 'transparent', btnCol: installed ? 'var(--cyan)' : 'var(--ink)',
        btnCur: installed || downloading ? 'default' : 'pointer',
        pick: noop,
        btnAct: e => {
          e && e.stopPropagation && e.stopPropagation();
          if (installed || downloading) return;
          this._startPaintDl(t.id);
        }
      };
    });
    // the api-endpoint row (CONFIGURE) stays — same relay-config door as before
    paintModels.push({
      id: 'paint-api',
      chip: hasPaint ? UP(img.tier).slice(0, 2) : '—',
      name: hasPaint ? UP(img.model || img.tier) : 'NO PAINT MODEL INSTALLED',
      size: hasPaint ? 'ON DEVICE' : '',
      desc: hasPaint
        ? 'Paints covers and portraits on this device — every image stamped ✦ AI.'
        : 'Covers and portraits stay typographic plates until a paint model or image endpoint is configured.',
      active: hasPaint,
      op: '1', shdw: '3px 3px 0 var(--shdw)', cur: 'default',
      chipBg: hasPaint ? 'var(--cyan)' : 'var(--ink)', chipCol: 'var(--inv)',
      downloading: false, dlW: '0%', dlPct: 0,
      btnLabel: hasPaint ? 'ACTIVE' : 'CONFIGURE',
      btnAria: 'Paint engine', btnBg: 'transparent', btnCol: 'var(--ink)', btnCur: 'pointer',
      pick: noop,
      btnAct: () => this.setState({ relayCfgOpen: true })
    });

    const tgl = (key, label, desc, settingKey) => ({
      label, desc,
      state: st[key] ? 'ON' : 'OFF',
      bg: st[key] ? 'var(--ink)' : 'transparent',
      col: st[key] ? 'var(--inv)' : 'var(--mut2)',
      flip: () => {
        const v = !st[key];
        const patch = {}; patch[key] = v;
        this.setState(patch);
        if (settingKey) {
          V.call('set_setting', { key: settingKey, value: v ? 'true' : 'false' })
            .catch(e => this._honest('SETTING', e));
        }
      }
    });
    const engineToggles = [
      tgl('tglStamps', 'SHOW THE ENGINE STAMPS', 'GATE → COMPOSE → VERIFY while the cast thinks. Honest, and worth the space.', 'show_engine_stamps'),
      tgl('tglFates', 'GUARD CHARACTER FATES', 'Questions like “does she die?” are deflected in voice rather than answered.', 'guard_fates'),
      tgl('tglSilhouettes', 'SILHOUETTE UNMET CAST', 'Even names can spoil. Unmet characters stay inked out until you meet them.', null),
      tgl('tglArt', 'GENERATE MISSING ART', 'Covers and portraits the book doesn’t provide are painted locally — stamped “✦ AI”, never downloaded.', null)
    ];

    const strictOpts = ['strict', 'standard', 'relaxed'].map(k => ({
      label: UP(k),
      bg: st.strict === k ? 'var(--ink)' : 'transparent',
      col: st.strict === k ? 'var(--inv)' : 'var(--ink)',
      pick: () => {
        this.setState({ strict: k });
        V.call('set_setting', { key: 'gate_mode', value: k }).catch(e => this._honest('GATE MODE', e));
      }
    }));

    /* gate probes — the real run_probes(bookId, 12), leak-taxonomy tallied */
    const gateTest = () => {
      if (st.gateState === 'running' || !meta) return;
      this.setState({ gateState: 'running', gateResult: '' });
      const t0 = Date.now();
      V.call('run_probes', { bookId: meta.id, n: 12 }).then(rs => {
        const n = (rs || []).length;
        const leaks = (rs || []).filter(r => r.leaked);
        const kinds = { future_event: 0, unmet_character: 0, tone_implies_ending: 0 };
        leaks.forEach(l => { if (l.leak_kind && kinds[l.leak_kind] != null) kinds[l.leak_kind]++; });
        const avg = n ? ((Date.now() - t0) / n / 1000).toFixed(2) : '0.00';
        this.setState({
          gateState: 'idle',
          gateResult: (n - leaks.length) + '/' + n + ' FUTURE PROBES BLOCKED '
            + (leaks.length ? '· ' + leaks.length + ' LEAKED' : '✓ · 0 LEAKS')
            + ' · FUTURE EVENT ' + kinds.future_event
            + ' · UNMET CHARACTER ' + kinds.unmet_character
            + ' · TONE ' + kinds.tone_implies_ending
            + ' · AVG GATE ' + avg + 'S'
        });
      }).catch(e => {
        this.setState({ gateState: 'idle', gateResult: '' });
        this._honest('PROBES NEED AN AI', e);
      });
    };

    /* relay config — one-tap presets (relay_presets/configure_relay) with the
       manual base/key/model fields kept as the "custom" advanced fallback */
    const presets = D.relayPresets || [];
    const provs = (presets.length
      ? presets.map(p => ({
        k: p.id, label: UP(p.name), url: p.base_url, model: p.default_model,
        localhost: /localhost|127\.0\.0\.1/.test(p.base_url || '')
      }))
      : [
        { k: 'openrouter', label: 'OPENROUTER', url: 'https://openrouter.ai/api/v1' },
        { k: 'openai', label: 'OPENAI', url: 'https://api.openai.com/v1' },
        { k: 'together', label: 'TOGETHER', url: 'https://api.together.xyz/v1' }
      ]).concat([{ k: 'custom', label: 'CUSTOM', url: '' }]);
    const saveRelayCfg = () => V.call('set_api_config', {
      baseUrl: String(st.relayUrl || '').trim(),
      apiKey: String(st.relayKey || ''),
      model: String(st.relayModel || '').trim() || 'gpt-4o-mini'
    });
    const relayFetch = () => {
      if (st.relayFetchSt === 'busy') return;
      if (!String(st.relayUrl || '').trim()) { this._toast('SET THE BASE URL FIRST'); return; }
      this.setState({ relayFetchSt: 'busy' });
      saveRelayCfg().then(() => V.call('list_relay_models')).then(ms => {
        this.setState({ relayFetchSt: 'idle', relayModels: ms || [] });
        if (!(ms || []).length) this._toast('THE PROVIDER RETURNED NO MODELS');
      }).catch(e => {
        this.setState({ relayFetchSt: 'idle', relayModels: [] });
        this._honest('MODEL LIST FAILED', e);
      });
    };
    const relayTest = () => {
      if (st.relayTestSt === 'busy') return;
      if (!String(st.relayUrl || '').trim()) { this._toast('SET THE BASE URL FIRST'); return; }
      this.setState({ relayTestSt: 'busy' });
      saveRelayCfg().then(() => V.call('test_relay')).then(r => {
        if (r && r.ok) {
          this.setState({ relayTestSt: 'done', relayLatency: r.latency_ms, relay: true });
          this._loadSettings();
        } else {
          this.setState({ relayTestSt: 'idle' });
          this._toast(UP('RELAY TEST FAILED — ' + ((r && r.message) || 'NO ANSWER')).slice(0, 88));
        }
      }).catch(e => {
        this.setState({ relayTestSt: 'idle' });
        this._honest('RELAY TEST FAILED', e);
      });
    };
    // one-tap connect: for a chosen preset, configure_relay fills base+model,
    // persists AND tests in a single call; "custom" falls back to the manual flow
    const relayConnect = () => {
      const pr = provs.find(p => p.k === st.relayProv);
      if (!pr || pr.k === 'custom' || !presets.length) { relayTest(); return; }
      if (st.relayTestSt === 'busy') return;
      this.setState({ relayTestSt: 'busy' });
      V.call('configure_relay', {
        provider: pr.k,
        apiKey: String(st.relayKey || ''),
        model: String(st.relayModel || '').trim()
      }).then(r => {
        if (r && r.ok) {
          this.setState({ relayTestSt: 'done', relayLatency: r.latency_ms, relay: true });
          this._loadSettings();
          this._toast(r.gate_verified ? 'RELAY CONNECTED — LEDGER GATE VERIFIED ✓' : 'RELAY CONNECTED ✓');
        } else {
          this.setState({ relayTestSt: 'idle' });
          this._toast(UP('RELAY — ' + ((r && r.message) || 'NO ANSWER')).slice(0, 88));
        }
      }).catch(e => {
        this.setState({ relayTestSt: 'idle' });
        this._honest('RELAY', e);
      });
    };

    /* leak report — the real report_leak */
    const leakLine = st.leakMsg ? String(st.leakMsg.text || '').slice(0, 160)
      : 'Something on this screen told me more than the book has.';
    const leakReasons = [
      { k: 'future_event', label: 'A FUTURE EVENT' },
      { k: 'unmet_character', label: 'AN UNMET CHARACTER' },
      { k: 'tone_implies_ending', label: 'TONE IMPLIES THE ENDING' }
    ].map(r => ({
      label: r.label,
      bg: st.leakReason === r.k ? 'var(--ink)' : 'transparent',
      col: st.leakReason === r.k ? 'var(--inv)' : 'var(--ink)',
      pick: () => this.setState({ leakReason: r.k })
    }));

    /* burn — the real delete_book */
    const delB = L.delB;
    const delConfirm = () => {
      const d = (this.bookDefs || []).find(b => b.id === st.delModal);
      if (!d || !d.meta) { this.setState({ delModal: null }); return; }
      V.call('delete_book', { id: d.meta.id }).then(() => {
        this.setState({ delModal: null });
        this._toast('BOOK BURNED — LEDGER, THEORIES AND CHATS WENT WITH IT');
        this._refreshBooks();
      }).catch(e => {
        this.setState({ delModal: null });
        this._honest('BURN FAILED', e);
      });
    };

    /* reader typography */
    const ffMap = { serif: "'Source Serif 4',serif", sans: "'Oswald',sans-serif", mono: "'IBM Plex Mono',monospace" };
    const lhMap = { s: '1.55', m: '1.75', l: '1.95' };
    const wMap = { s: '560px', m: '680px', l: '820px' };
    const fsMap = L.fsMap || { s: '15px', m: '17px', l: '19px' };

    const ep = D.episode;
    const openTheory = (D.theories || []).find(t => !t.resolved_status);

    const markReadFn = () => {
      if (!compReadyR) return;
      if (ch >= total) { this._toast('THAT WAS THE LAST PAGE.'); return; }
      L.advanceCh();
      this._persistProgress(Math.min(total, ch + 1));
    };
    const jumpTo = n => {
      this.setState({ chOverride: n, tocOpen: false });
      this._toast(n < ch ? 'RE-SEALED TO CH.' + n + ' — THE CAST FORGETS EVERYTHING AFTER' : 'HORIZON → CH.' + n);
      this._persistProgress(n);
    };

    const langNames = { fr: 'French', ja: 'Japanese', de: 'German', es: 'Spanish' };
    const storeStOf = id => st.storeSt[id] || {};
    const dictW = ((st.selText || '').split(/\s+/)[0] || '').replace(/[^a-zA-Z'À-ɏ-]/g, '').toLowerCase();

    const catAddFn = () => {
      const u = String(st.catUrl || '').trim();
      if (!u) { this._toast('PASTE AN OPDS URL FIRST'); return; }
      let name = 'OPDS CATALOG';
      try { name = new URL(u.indexOf('http') === 0 ? u : 'https://' + u).hostname.replace('www.', ''); } catch (err) {}
      V.call('add_opds_catalog', { url: u, name }).then(() => {
        this.setState({ catUrl: '' });
        this._toast('CATALOG ADDED — ' + UP(name));
        return V.call('list_opds_catalogs');
      }).then(cats => {
        if (cats) this.setState({ catalogs: cats.map(c => ({ id: c.id, name: c.name, url: c.url, open: false, books: [], loaded: false })) });
      }).catch(e => this._honest('CATALOG', e));
    };
    const ao3FetchFn = () => {
      const u = String(st.ao3Q || '').trim();
      if (!u) { this._toast('PASTE AN AO3 WORK LINK FIRST'); return; }
      const id = 'ao3-' + Date.now();
      const item = { id, title: 'AO3 WORK — FETCHING…', author: u.length > 42 ? u.slice(0, 42) + '…' : u, year: 'WEB', size: 'EPUB', src: 'ao3', lic: 'AO3 · AUTHOR-PROVIDED' };
      this.setState(s => {
        const o = {}; o[id] = { status: 'dl', pct: 10 };
        return { ao3Items: [...s.ao3Items, item], ao3Q: '', storeSt: Object.assign({}, s.storeSt, o) };
      });
      V.call('import_ao3_link', { url: u }).then(bm => {
        this.setState(s => {
          const o = {}; o[id] = { status: 'done', pct: 100 };
          return {
            ao3Items: s.ao3Items.map(x => x.id === id ? Object.assign({}, x, { title: UP(bm.title), author: UP(bm.author || 'AO3') }) : x),
            storeSt: Object.assign({}, s.storeSt, o)
          };
        });
        this._toast(UP(bm.title) + ' → YOUR SHELF');
        this._refreshBooks();
      }).catch(e => {
        this.setState(s => ({ ao3Items: s.ao3Items.filter(x => x.id !== id) }));
        this._honest('AO3 FETCH FAILED', e);
      });
    };

    /* ---- Gutenberg: real store_browse defaults + topic chips (topic@page) ---- */
    const gutBrowse = topic => {
      this.setState({ gutTopic: topic, gutBusy: true });
      const args = topic
        ? { source: 'gutenberg', cursor: topic.toLowerCase() + '@1' }
        : { source: 'gutenberg' };
      V.call('store_browse', args).then(items => {
        this.storeGut = (items || []).map(x => this._storeItemDef(x, 'gut'));
        this.setState({ gutBusy: false });
      }).catch(e => {
        this.storeGut = [];
        this._vena.storeOffline = true;
        this.setState({ gutBusy: false });
        this._honest('GUTENBERG UNREACHABLE', e);
      });
    };
    const gq = String(st.gutQ || '').trim().toLowerCase();
    const gutRowsReal = (this.storeGut || [])
      .filter(x => !gq || (x.title + ' ' + x.author).toLowerCase().includes(gq))
      .slice(0, 8)
      .map(x => {
        const s = st.storeSt[x.id] || {};
        return Object.assign({}, x, {
          isIdle: s.status !== 'dl' && s.status !== 'forge' && s.status !== 'done',
          isDl: s.status === 'dl', isForge: s.status === 'forge', isDone: s.status === 'done',
          pctW: (s.pct || 0) + '%', pctLbl: (s.pct || 0) + '%',
          get: () => this._storeGet(x),
          goShelf: go('library')
        });
      });

    /* ---- real CBZ paging (bounds & labels from the real page count) ---- */
    const MG = this._manga;
    const mgPage = MG ? Math.max(1, Math.min(st.mangaPage, MG.count)) : st.mangaPage;
    const mgStep = st.mangaSpread ? 2 : 1;
    const mangaReal = MG ? {
      mangaPageLbl: st.mangaSpread && mgPage < MG.count
        ? mgPage + '–' + Math.min(MG.count, mgPage + 1) + ' / ' + MG.count
        : mgPage + ' / ' + MG.count,
      mangaLeftN: st.mangaRtl && st.mangaSpread ? Math.min(MG.count, mgPage + 1) : mgPage,
      mangaRightN: st.mangaRtl && st.mangaSpread ? mgPage : Math.min(MG.count, mgPage + 1),
      mangaPrev: () => this.setState({ mangaPage: Math.max(1, mgPage - mgStep) }),
      mangaNext: () => this.setState({ mangaPage: Math.min(MG.count, mgPage + mgStep) }),
      mangaStrip: Array.from({ length: Math.max(1, Math.min(4, MG.count - mgPage + 1)) }, (_, i) => ({ n: mgPage + i }))
    } : {};

    /* ---------------- the tail itself ---------------- */
    return {
      /* system bar */
      themeName: L.themeName,
      navTabs: L.navTabs.map(n => Object.assign({}, n, {
        go: () => {
          n.go();
          if (n.label === 'STORE' && !this._storeLoaded) { this._storeLoaded = true; this._loadStore(); }
        }
      })),
      telemetry: (meta
        ? 'LEDGER ' + meta.fact_count + ' FACTS · GATE ≤ CH.' + ch + ' · 100% LOCAL'
        : 'NO BOOK OPEN · 100% LOCAL')
        + (partialReadyR
          ? ' · COMPANION READY THROUGH ' + (forgedThrough > 0 ? 'CH.' + this.roman(forgedThrough) : 'THE OPENING') + ' — STILL FORGING THE REST'
          : ''),
      modelBadge: aiBadge,
      themeBtns: [
        { k: 'light', label: '☀ DAY' }, { k: 'sepia', label: '▤ SEPIA' },
        { k: 'dark', label: '☾ NIGHT' }, { k: 'oled', label: '● OLED' }
      ].map(t => ({
        label: t.label,
        bg: L.themeName === t.k ? 'var(--ink)' : 'transparent',
        col: L.themeName === t.k ? 'var(--inv)' : 'var(--ink)',
        pick: () => {
          this.setState({ theme: t.k });
          V.call('set_setting', { key: 'theme', value: t.k }).catch(() => {});
        }
      })),
      // BROWSE FILES (real import) and READ THE BRANCH share the template's {{ noop }}
      noop: e => {
        const t = (e && e.target && e.target.textContent) || '';
        if (/BROWSE FILES/i.test(t)) this._venaBrowseBook();
        else if (/READ THE BRANCH/i.test(t)) this._toast('WHAT-IF BRANCHES AREN’T WIRED TO THE ENGINE YET');
      },

      /* screens */
      scrLibrary: st.screen === 'library',
      scrCompanion: st.screen === 'companion',
      scrReader: st.screen === 'reader',
      scrSettings: st.screen === 'settings',
      scrArchive: st.screen === 'archive',

      /* library */
      books,
      shelfMeta: books.length + ' BOOK' + (books.length === 1 ? '' : 'S') + ' · '
        + books.filter(b => b.isForged).length + ' LEDGERS FORGED · YOUR BOOKS, YOUR DEVICE, NOTHING LEAVES IT',
      visionStart: () => this._visionRun(),

      /* comics: real pages when a comic is on the shelf, demo view otherwise */
      mangaOpenFn: () => { this.setState({ mangaOpen: true }); this._loadManga(); },
      ...mangaReal,

      /* store · gutenberg: real most-downloaded page + topic chips */
      gutRows: gutRowsReal,
      gutChips: ['MYSTERY', 'GOTHIC', 'ADVENTURE'].map(c => ({
        label: c,
        bg: st.gutTopic === c ? 'var(--ink)' : 'transparent',
        col: st.gutTopic === c ? 'var(--inv)' : 'var(--ink)',
        pick: () => gutBrowse(st.gutTopic === c ? null : c)
      })),

      /* companion header + states */
      ch, chRoman,
      pctLabel: L.pct + '%',
      ticks: L.mkTicks(ch, false),
      goReader: () => this.setState({ screen: 'reader', chatOpen: false }),
      goCompanion: () => this.setState({ screen: 'companion', chatOpen: false, whoId: null }),
      compReady: compReadyR || partialReadyR, compFresh: compFreshR, compForging: compForgingR && !partialReadyR, compEmpty: compEmptyR,
      coachOn: st.coach && compReadyR,
      compBooks: defs.map(b => ({
        title: b.title, cover: b.cover, initial: b.title.replace('THE ', '')[0],
        brd: ab && ab.id === b.id ? 'var(--red)' : 'var(--mut2)',
        pick: () => this._selectBook(b)
      })),
      freshBegin: () => {
        this.setState({ chOverride: 1, screen: 'reader' });
        this._persistProgress(1);
      },
      spoiledMe: () => this.setState({ leakOpen: true, leakMsg: null, leakReason: 'future_event' }),

      /* cast */
      cast,
      silhouettes: unmet.slice(0, 3).map(s => ({ hint: s.hint || 'Keep reading' })),
      castMeta: met.length + ' MET · ' + unmet.length + ' STILL INK — CHARACTERS FROM YOUR BOOK, NO NAMES AHEAD OF YOUR BOOKMARK',

      /* recap (real get_recap, typed out) */
      recapIdle: !st.recapOpen,
      recapRolling: st.recapOpen,
      recapStreaming: st.recapOpen && !st.recapDone,
      recapDone: st.recapOpen && st.recapDone,
      recap: st.recap,
      recapPlay: () => { this.setState({ recapOpen: true, recap: '', recapDone: false }); this._streamRecap(); },
      recapAgain: () => { this.setState({ recap: '', recapDone: false }); this._streamRecap(); },

      /* theory board (backend-resolved only) */
      theories: ths,
      theoryMeta: ths.length + ' PINNED · ' + ths.filter(t => t.stamped).length + ' RESOLVED — BY THE LEDGER, NEVER BY GUESSWORK',
      flipT: st.resolved ? 'rotateY(180deg)' : 'none',
      resolve: () => this.setState({ resolved: true }),
      resolved: st.resolved,
      newTheory: st.newTheory,
      theoryInput: e => this.setState({ newTheory: e.target.value }),
      theoryKey: e => { if (e.key === 'Enter') addTheoryFn(); },
      addTheory: addTheoryFn,

      /* what-if (design feature; the branch writer is not a backend command) */
      branches: st.branches.map(b => ({ titleUp: UP(b) })),
      branchDraft: st.branchDraft,
      branchInput: e => this.setState({ branchDraft: e.target.value }),
      branchKey: e => {
        if (e.key !== 'Enter') return;
        const t = st.branchDraft.trim();
        if (t) this.setState({ branches: [...st.branches, t], branchDraft: '' });
      },
      addBranch: () => {
        const t = st.branchDraft.trim();
        if (!t) { this._toast('WRITE THE WHAT-IF FIRST'); return; }
        this.setState({ branches: [...st.branches, t], branchDraft: '' });
        this._toast('QUEUED — THE BRANCH WRITER NEEDS AN AI ENGINE TO RUN');
      },

      /* who's who lists (real synced wiki) */
      whoPeople: (this.whoPeople || []).map(w => ({ nameUp: UP(w.name), desc: w.desc, seen: w.seen })),
      whoPlaces: (this.whoPlaces || []).map(w => ({ nameUp: UP(w.name), desc: w.desc, seen: w.seen })),
      whoTerms: (this.whoTerms || []).map(w => ({ nameUp: UP(w.name), desc: w.desc, seen: w.seen })),

      /* chat splash */
      chatOpen: st.chatOpen && !compEmptyR,
      charBg: artMap[cKey] || 'radial-gradient(circle at 42% 18%, rgba(183,137,63,.4), transparent 55%), linear-gradient(165deg, #26303e, #0f141c)',
      charGen: false, charPlain: true, charSlotId: '', charSlotPh: '',
      charInit: charObj.init,
      charNameUp: UP(charObj.name),
      charRoleUp: UP(charObj.role),
      charLastUp: UP(lastWord(charObj.name)),
      charShort: charObj.short,
      switcher: met.slice(0, 8).map(c => ({
        init: c.init, name: c.name,
        col: c.id === st.char ? 'var(--red)' : 'var(--inv)',
        pick: () => this.setState({ char: c.id })
      })),
      msgs,
      stream: st.stream, streamingHas: !!st.stream,
      thinking: !!st.phase && st.tglStamps,
      steps: L.steps,
      showChips: st.chatOpen && msgs.length === 0 && !st.phase,
      chips,
      input: st.input,
      onInput: e => this.setState({ input: e.target.value }),
      onKey: e => { if (e.key === 'Enter') this._send(); },
      send: () => this._send(),
      chatClose: () => this.setState({ chatOpen: false, phase: null }),

      /* archive (real spoiler consent + full wiki) */
      archForging: compForgingR,
      archGate: !compForgingR && !compEmptyR && !(unlocked && wikiData),
      archOpen: !compForgingR && !compEmptyR && unlocked && !!wikiData,
      archAt: ch, archTotal: total, archAhead: Math.max(0, total - ch),
      wikiOpen: () => {
        if (!meta) return;
        V.call('set_spoiler_consent', { bookId: meta.id, granted: true })
          .then(() => this._loadFullWiki(meta, false))
          .catch(e => this._honest('CONSENT', e));
      },
      wikiReseal: () => {
        if (!meta) return;
        V.call('set_spoiler_consent', { bookId: meta.id, granted: false }).catch(() => {});
        const w = Object.assign({}, st.wikiUnlocked);
        delete w[ab.id];
        delete this.wiki[ab.id];
        this.setState({ wikiUnlocked: w, wikiArmed: false });
        this._toast('RE-SEALED. WHAT WAS READ, THOUGH, WAS READ.');
      },
      wikiLead: curSec
        ? 'Everything the ledger holds under “' + curSec.label.toLowerCase() + '” — through the final page, chapter-stamped, nothing held back.'
        : '',
      wikiInfoHas: !!(wikiData && wikiData.infobox),
      wikiInfoTitle: wikiData && wikiData.infobox ? wikiData.infobox.title : '',
      wikiInfo: wikiData && wikiData.infobox ? wikiData.infobox.rows : [],
      wikiCats: wikiData ? wikiData.sections.map(s => ({ label: s.label })) : [],
      wikiStats1: wikiData && wikiData._idx
        ? wikiData._idx.entries.length + ' PAGES · ' + wikiData._idx.entries.reduce((a, e) => a + e.fact_count, 0) + ' FACTS'
        : '',
      wikiStats2: D.wikiIdx ? D.wikiIdx.sealed_total + ' WERE SEALED AT YOUR BOOKMARK' : '',
      wikiFooterMeta: meta
        ? 'WRITTEN BY THE LEDGER · SHA ' + shaShort(meta.package_sha) + ' · NOTHING LEAVES THIS DEVICE'
        : '',

      /* settings */
      models,
      paintModels,
      strictDesc: L.strictDescs[st.strict] || '',
      strictOpts,
      engineToggles,
      gateBtnLabel: st.gateState === 'running' ? 'PROBING…' : 'RUN 12 PROBES',
      gateTest,
      gateResult: st.gateResult || '',
      gateResultHas: !!st.gateResult,

      relayLabel: st.relay ? 'ON' : 'OFF',
      relayPressed: String(!!st.relay),
      relayBg: st.relay ? 'var(--red)' : 'transparent',
      relayCol: st.relay ? '#fff' : 'var(--mut2)',
      relayToggle: () => {
        if (!st.relay && !(S && S.cloud_base_url)) {
          this.setState({ relayCfgOpen: true });
          this._toast('CONFIGURE THE RELAY FIRST — URL, KEY, MODEL');
          return;
        }
        const on = !st.relay;
        V.call('set_chat_mode', { mode: on ? 'cloud' : 'local' }).then(() => {
          this.setState({ relay: on });
          this._loadSettings();
          this._toast(on ? 'CLOUD RELAY ON — THE LEDGER GATE RUNS LOCALLY BEFORE ANYTHING IS SENT' : 'CLOUD RELAY OFF — FULLY LOCAL AGAIN');
        }).catch(e => this._honest('RELAY', e));
      },
      relayCfgOpen: st.relayCfgOpen,
      relayCfgOpenFn: () => this.setState({ relayCfgOpen: true }),
      relayCfgClose: () => this.setState({ relayCfgOpen: false }),
      provChips: provs.map(p => ({
        label: p.label,
        pressed: String(st.relayProv === p.k),
        bg: st.relayProv === p.k ? 'var(--ink)' : 'transparent',
        col: st.relayProv === p.k ? 'var(--inv)' : 'var(--ink)',
        pick: () => this.setState({
          relayProv: p.k,
          relayUrl: p.url || st.relayUrl,
          relayModel: p.model || st.relayModel,
          relayModels: [], relayFetchSt: 'idle', relayTestSt: 'idle'
        })
      })),
      relayUrl: st.relayUrl,
      relayUrlInput: e => this.setState({ relayUrl: e.target.value }),
      relayKey: st.relayKey,
      relayKeyType: st.relayShowKey ? 'text' : 'password',
      relayKeyInput: e => this.setState({ relayKey: e.target.value }),
      relayKeyTgl: () => this.setState({ relayShowKey: !st.relayShowKey }),
      relayKeyTglLabel: st.relayShowKey ? 'HIDE' : 'SHOW',
      relayKeyPressed: String(!!st.relayShowKey),
      relayKeyPaste: () => {
        (navigator.clipboard && navigator.clipboard.readText
          ? navigator.clipboard.readText()
          : Promise.reject(new Error('no clipboard access')))
          .then(v => this.setState({ relayKey: String(v || '').trim() }))
          .catch(() => this._toast('CLIPBOARD BLOCKED — TYPE THE KEY INTO THE FIELD'));
      },
      relayModel: st.relayModel,
      relayModelInput: e => this.setState({ relayModel: e.target.value }),
      relayFetch,
      relayFetchLabel: st.relayFetchSt === 'busy' ? 'FETCHING…' : 'FETCH MODELS',
      relayModelsHas: (st.relayModels || []).length > 0,
      relayModelChips: (st.relayModels || []).slice(0, 8).map(id => ({
        label: id,
        pressed: String(st.relayModel === id),
        bg: st.relayModel === id ? 'var(--ink)' : 'transparent',
        col: st.relayModel === id ? 'var(--inv)' : 'var(--ink)',
        pick: () => this.setState({ relayModel: id })
      })),
      relayImgEpTgl: () => this.setState({ relayImgEp: !st.relayImgEp }),
      relayImgEpPressed: String(!!st.relayImgEp),
      relayImgEpBg: st.relayImgEp ? 'var(--ink)' : 'transparent',
      relayImgEpCol: st.relayImgEp ? 'var(--inv)' : 'var(--mut2)',
      relayImgEpState: st.relayImgEp ? 'ON' : 'OFF',
      relayTest: relayConnect,
      relayTestLabel: st.relayTestSt === 'busy' ? 'CONNECTING…' : 'CONNECT',
      relayTestRunning: st.relayTestSt === 'busy',
      relayTestDone: st.relayTestSt === 'done',
      relayTestResult: st.relayTestSt === 'done' ? st.relayLatency + ' MS ROUND-TRIP ✓' : '',

      exportData: () => {
        try {
          const blob = new Blob([JSON.stringify({ theories: D.theories, notes: st.notes }, null, 2)], { type: 'application/json' });
          const a = document.createElement('a');
          a.href = URL.createObjectURL(blob);
          a.download = (ab ? ab.id : 'vena') + '-theories-notes.json';
          a.click();
          this._toast('EXPORTED — THEORIES & NOTES, PLAIN JSON');
        } catch (err) { this._toast('EXPORT FAILED'); }
      },
      wipeBook: () => { if (ab) this.setState({ delModal: ab.id }); },

      /* serial pacing has no engine behind it — the toggle refuses honestly
         instead of showing a fabricated countdown/streak in the reader */
      serialTgl: () => this._toast('SERIAL PACING ISN’T WIRED IN THIS BUILD — EVERY CHAPTER IS OPEN'),

      recapTglBg: st.tglAutoRecap ? 'var(--ink)' : 'transparent',
      recapTglCol: st.tglAutoRecap ? 'var(--inv)' : 'var(--mut2)',
      recapTglState: st.tglAutoRecap ? 'ON' : 'OFF',
      recapTgl: () => this.setState({ tglAutoRecap: !st.tglAutoRecap }),
      resealTglBg: st.tglReseal ? 'var(--ink)' : 'transparent',
      resealTglCol: st.tglReseal ? 'var(--inv)' : 'var(--mut2)',
      resealTglState: st.tglReseal ? 'ON' : 'OFF',
      resealTgl: () => {
        const v = !st.tglReseal;
        this.setState({ tglReseal: v });
        V.call('set_setting', { key: 'reseal_on_reread', value: v ? 'true' : 'false' }).catch(e => this._honest('SETTING', e));
      },
      chDown: () => { if (ch > 1) jumpTo(ch - 1); },
      chUp: () => { if (ch < total) jumpTo(ch + 1); },

      /* settings: target language persists */
      langChips: [{ k: 'fr', label: 'FRANÇAIS' }, { k: 'ja', label: '日本語' }, { k: 'de', label: 'DEUTSCH' }, { k: 'es', label: 'ESPAÑOL' }].map(l => ({
        label: l.label,
        bg: st.targetLang === l.k ? 'var(--ink)' : 'transparent',
        col: st.targetLang === l.k ? 'var(--inv)' : 'var(--ink)',
        pick: () => {
          this.setState({ targetLang: l.k });
          V.call('set_setting', { key: 'target_language', value: langNames[l.k] || 'French' }).catch(() => {});
        }
      })),

      /* leak report */
      leakOpen: st.leakOpen,
      leakCtx: [
        { k: 'BOOK', v: ab ? ab.title : '—' },
        { k: 'SPEAKING WITH', v: st.leakMsg ? charObj.name : 'THE COMPANION SCREEN' },
        { k: 'GATE', v: '≤ CH.' + ch + ' · ' + UP(st.strict) },
        { k: 'ENGINE', v: aiBadge }
      ],
      leakLine,
      leakReasons,
      leakSubmit: () => {
        if (!meta) { this.setState({ leakOpen: false, leakMsg: null }); return; }
        V.call('report_leak', { bookId: meta.id, reason: st.leakReason, excerpt: leakLine, comment: '' }).then(() => {
          this.setState({ leakOpen: false, leakMsg: null });
          this._toast('LEAK FILED — THE GATE TIGHTENS FOR THIS BOOK');
        }).catch(e => {
          this.setState({ leakOpen: false, leakMsg: null });
          this._honest('REPORT FAILED', e);
        });
      },
      leakCancel: () => this.setState({ leakOpen: false, leakMsg: null }),

      /* burn modal */
      delModalHas: !!delB,
      delCover: delB ? delB.cover : 'var(--ink)',
      delTitleUp: delB ? delB.title : '',
      delTitle: delB ? delB.title : '',
      delStats: delB ? delB.stats || '' : '',
      delConfirm,
      delCancel: () => this.setState({ delModal: null }),

      /* toast */
      toastHas: !!st.toast, toast: st.toast,

      /* reader */
      readerReady: compReadyR,
      readerEmpty: !compReadyR,
      readerEmptyHead: compEmptyR ? 'THE SHELF IS EMPTY' : compForgingR ? 'THE LEDGER IS STILL FORGING' : 'A FRESH LEDGER',
      readerEmptyBody: compEmptyR
        ? 'Bring your own book — import an EPUB and it opens the moment it is on your shelf. Every page stays on this device.'
        : compForgingR
          ? 'The reader opens when every fact is stamped with the chapter it becomes true.'
          : 'You haven’t opened this book yet. Begin at Chapter I — the companion learns as you read.',
      readerEmptyBtn: compEmptyR ? 'GO TO LIBRARY →' : compForgingR ? 'BACK TO THE SHELF' : 'BEGIN CHAPTER I →',
      readerEmptyAct: compFreshR
        ? () => { this.setState({ chOverride: 1 }); this._persistProgress(1); }
        : go('library'),
      typeOpen: st.typeOpen,
      typeToggle: () => this.setState({ typeOpen: !st.typeOpen }),
      typeBtnBg: st.typeOpen ? 'var(--ink)' : 'transparent',
      typeBtnCol: st.typeOpen ? 'var(--inv)' : 'var(--ink)',
      fsOpts: L.fsOpts,
      ffOpts: L.seg('readerFf', [{ v: 'serif', label: 'SERIF' }, { v: 'sans', label: 'SANS' }, { v: 'mono', label: 'MONO' }]),
      lhOpts: L.seg('readerLh', [{ v: 's', label: 'TIGHT' }, { v: 'm', label: 'BOOK' }, { v: 'l', label: 'AIRY' }]),
      wOpts: L.seg('readerW', [{ v: 's', label: 'NARROW' }, { v: 'm', label: 'BOOK' }, { v: 'l', label: 'WIDE' }]),
      alignOpts: [{ v: false, label: 'LEFT' }, { v: true, label: 'JUST' }].map(o => ({
        label: o.label,
        bg: st.readerJust === o.v ? 'var(--ink)' : 'transparent',
        col: st.readerJust === o.v ? 'var(--inv)' : 'var(--ink)',
        pick: () => this.setState({ readerJust: o.v })
      })),
      readerFs: fsMap[st.fontScale] || '17px',
      readerFfCss: ffMap[st.readerFf] || ffMap.serif,
      readerLhCss: lhMap[st.readerLh] || '1.75',
      readerAlignCss: st.readerJust ? 'justify' : 'left',
      readerWCss: wMap[st.readerW] || '680px',
      markRead: markReadFn,
      readAhead: () => { this._toast('READING AHEAD — THE STREAK PAUSES, NOTHING IS LOST'); markReadFn(); },
      askFab: () => this.setState({ chatOpen: true, screen: 'companion' }),
      askPassage: () => this.setState({
        chatOpen: true, screen: 'companion',
        input: 'About this passage in Chapter ' + chRoman + ' — what should I make of it?'
      }),
      statsLine: ep
        ? '≈ ' + (ep.est || '?') + ' MIN IN THIS CHAPTER · ' + ep.scenes + ' SCENE' + (ep.scenes === 1 ? '' : 'S')
          + ' · CH.' + ch + '/' + total + (st.readerMode === 'paged' ? ' · PAGE ' + st.pageN + ' OF 18' : '')
        : 'FETCHING THE CHAPTER FROM THE LEDGER…',
      factBreak: meta
        ? meta.fact_count + ' FACTS ON THE LEDGER · COVERAGE ' + Math.round((meta.ledger_coverage || 0) * 100) + '%'
        : '',
      theoryNudge: openTheory
        ? 'An open theory is pinned — “' + openTheory.text.slice(0, 64) + (openTheory.text.length > 64 ? '…' : '') + '”'
        : 'Pin a theory — it turns only when your bookmark passes the reveal.',
      toc: Array.from({ length: total }, (_, i) => {
        const n = i + 1, cur = n === ch;
        return {
          label: 'CHAPTER ' + this.roman(n),
          tick: n <= ch ? '✓' : '○',
          facts: n <= ch ? 'BEHIND YOUR HORIZON' : 'SEALED AHEAD',
          bg: cur ? 'var(--ink)' : 'transparent',
          col: cur ? 'var(--inv)' : 'var(--ink)',
          jump: () => jumpTo(n)
        };
      }),
      /* who's-who card on the real cast */
      whoVH: e => { e && e.stopPropagation && e.stopPropagation(); const c = byName('helsing'); if (c) this.setState({ whoId: c.id }); },
      whoLucy: e => { e && e.stopPropagation && e.stopPropagation(); const c = byName('lucy'); if (c) this.setState({ whoId: c.id }); },
      whoBust: whoObj ? bustMap[whoKey] || bustMap.jh || {} : {},
      whoBio: whoObj ? whoObj.bio || whoObj.role || '' : '',
      whoRoleUp: whoObj ? UP(whoObj.role) : '',

      /* translate: no on-device MT model — keep the UI, refuse honestly */
      selTrans: () => this._toast('TRANSLATE NEEDS AN MT MODEL OR CLOUD RELAY — NONE INSTALLED'),
      transOpen: false,
      rollingTr: false,
      rtBg: 'transparent', rtCol: 'var(--ink)',
      transShort: 'OFF',
      transOpts: [
        { k: 'off', label: 'OFF — ENGLISH ONLY' }, { k: 'fr', label: 'FRANÇAIS' },
        { k: 'ja', label: '日本語' }, { k: 'de', label: 'DEUTSCH' }, { k: 'es', label: 'ESPAÑOL' }
      ].map(o => ({
        label: o.label, tick: o.k === 'off' ? '✓' : '',
        bg: o.k === 'off' ? 'var(--ink)' : 'transparent',
        col: o.k === 'off' ? 'var(--inv)' : 'var(--ink)',
        pick: () => {
          this.setState({ langOpen: false });
          if (o.k !== 'off') this._toast('ROLLING TRANSLATE NEEDS AN MT MODEL — NONE ON THIS DEVICE');
        }
      })),

      /* store copy that must stay honest */
      storeResultsMeta: (L.extra.storeResults || []).length + ' RESULT'
        + ((L.extra.storeResults || []).length === 1 ? '' : 'S') + ' · LOCAL CATALOG'
        + (D.storeOffline ? ' · NETWORK SOURCES OFFLINE' : ''),
      catalogs: (st.catalogs || []).map((c, i) => ({
        name: UP(c.name || 'CATALOG'),
        meta: (c.loaded ? c.books.length + ' BOOKS · ' : '')
          + 'OPDS · ' + UP(String(c.url || '').replace(/^https?:\/\//, '').split('/')[0]),
        open: !!c.open, caret: c.open ? '▾' : '▸',
        flip: () => {
          const cats = st.catalogs.map((x, j) => j === i ? Object.assign({}, x, { open: !x.open }) : x);
          this.setState({ catalogs: cats });
          if (!c.open && !c.loaded) this._browseCatalog(c, i);
        },
        remove: () => {
          V.call('remove_opds_catalog', { id: c.id }).then(() => {
            this.setState({ catalogs: st.catalogs.filter((x, j) => j !== i) });
            this._toast('CATALOG REMOVED — ITS BOOKS STAY ON YOUR SHELF');
          }).catch(e => this._honest('REMOVE FAILED', e));
        },
        books: (c.books || []).map(x => Object.assign({}, x, {
          isIdle: !storeStOf(x.id).status || storeStOf(x.id).status === 'idle',
          isDl: storeStOf(x.id).status === 'dl',
          isForge: storeStOf(x.id).status === 'forge',
          isDone: storeStOf(x.id).status === 'done',
          pctW: (storeStOf(x.id).pct || 0) + '%',
          pctLbl: (storeStOf(x.id).pct || 0) + '%',
          get: () => this._storeGet(x),
          goShelf: go('library')
        }))
      })),
      catAdd: catAddFn,
      catKey: e => { if (e.key === 'Enter') catAddFn(); },
      ao3Fetch: ao3FetchFn,
      ao3Key: e => { if (e.key === 'Enter') ao3FetchFn(); },

      /* dictionary: the built-in offline entries are real; misses are honest */
      dictWord: dictW ? UP(dictW) : 'SELECT A WORD',
      dictText: this.dict[dictW]
        || ('“' + dictW + '” — no entry in the built-in WordNet. AI fallback definitions need an engine; add one in Settings.'),
      dictSrc: this.dict[dictW] ? 'WORDNET · BUILT-IN · OFFLINE' : 'NO ENTRY · AI FALLBACK NEEDS AN ENGINE',
      selAsk: () => this.setState({
        selOpen: false, dictOpen: false, chatOpen: true, screen: 'companion',
        input: 'About this line: “' + (st.selText || '') + '” — what should I make of it?'
      })
    };
  };
})();
