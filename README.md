# Vena

**Vena is a free, open-source, local-first AI reading companion** (desktop + mobile).
You read a book inside Vena; a character from that book — or the narrator — reads along
with you and will talk with you about it. The one hard promise: **the companion can
never spoil you.** Not "is prompted not to." *Can not.*

## The Gate, in brief

Vena does not trust a language model to keep secrets. It makes secrets unreachable.

Every book is **forged** into a **ledger**: a chapter-stamped database of facts,
characters, voice cards, and a story graph, packaged as a `.vena` file. At chat time a
five-stage engine runs:

```
GATE → COMPOSE → GENERATE → VERIFY → (REPAIR)
```

1. **GATE** — deterministic SQL over the ledger selects *only* facts stamped at or
   before your bookmark. Future facts and characters you have not met **never enter the
   model's context**. What the model never sees, it cannot leak.
2. **COMPOSE** — the gated context is assembled into the prompt (graph-guided
   retrieval, §6b).
3. **GENERATE** — a local model (INK/QUILL/ARCHIVIST tier) or the Cloud Relay answers.
4. **VERIFY** — the reply is checked against the ledger's leak taxonomy
   (`future_event`, `unmet_character`, `tone_implies_ending`).
5. **REPAIR** — a flagged reply is rewritten or **INKED OUT**. A remote backend never
   receives the text of the forbidden facts, even during repair — it gets a neutral
   instruction only.

Invariants the codebase enforces (and tests):

- **Canon is immutable** — episode/scene text has no update path; translation is an overlay.
- **Gate before generate** — always; the eval harness proves containment deterministically.
- **Cloud Relay never sends ungated content** — off-device prompts are built exclusively
  from gate output; repair prompts to remote backends carry no forbidden text.
- **Network allowlist** — outbound HTTP only to fixed book/model sources (Gutendex,
  Project Gutenberg, Standard Ebooks, AO3, Hugging Face) and hosts *you* configure
  (your OPDS catalogs, your BYO API endpoint). Unknown hosts are refused. No telemetry.
- **Full-spoiler Archive requires explicit consent** — per book; revoking consent re-seals.
- **API keys live in the OS keychain** — never in SQLite, settings, or logs.
- **No mocks in runtime** — the scripted test backend is compiled out of shipping builds.

## Workspace layout

```
crates/vena-core      The kernel: schema, the gate (store.rs), 5-stage engine,
                      verify/leak taxonomy, sealed/unsealed Archive, .vena packaging.
crates/vena-forge     Book → ledger. Real EPUB/Gutenberg-txt import, scene
                      segmentation, model-forged or maintainer-curated ledgers,
                      story-graph derivation. CLI: inspect / forge / import.
crates/vena-eval      The Phase-1 eval + permanent regression harness (Appendix C
                      interviews; generative and deterministic gate-audit modes).
crates/vena-app       The app: the full §11.2 IPC surface as a plain library
                      (api.rs), the Tauri 2 binary (`vena`), and `vena-devserver`
                      (the same commands over localhost HTTP for browser dev).
ui-dc/                The canonical UI (desktop + mobile), hydrated from the real
                      engine over the devserver bridge. Fonts and React are bundled;
                      nothing loads from a CDN.
data/packages/        Prebuilt .vena packages (public-domain Dracula ships as demo).
data/eval/            Interview sets for the eval harness.
docs/                 System design + canonical design files.
schema.sql            The ledger schema (Appendix A).
```

## Building

Everything except the Tauri shell builds with plain cargo:

```bash
cargo build            # all crates; the Tauri binary is skipped by default
cargo test             # full suite (23 tests, incl. a real forge round-trip)
```

**The shipped desktop binary** (Tauri 2; needs a desktop toolchain — on Linux
`webkit2gtk-4.1`, `gdk-3.0`, etc.):

```bash
cargo build -p vena-app --features tauri
```

**Development** (browser UI against the real engine — no mocks anywhere):

```bash
cargo run -p vena-app --bin vena-devserver
# then open http://127.0.0.1:5714  (VENA_DEV_PORT to change; VENA_DATA_DIR for the
# profile dir; VENA_PACKAGES_DIR to auto-import .vena packages; VENA_UI_DIST=ui-dc)
```

**Forging a book:**

```bash
cargo run -p vena-forge -- inspect --input book.epub
cargo run -p vena-forge -- forge --input book.epub --out book.vena          # model-forged
cargo run -p vena-forge -- forge --input book.txt --curated ledger.json --out book.vena
```

## Running the eval

The eval is the product's conscience; it is kept forever as the regression suite and
ships in-app as **Test the Gate**.

```bash
# Deterministic gate-audit (no backend needed) — proves containment:
cargo run -p vena-eval -- --vena data/packages/dracula.vena --interviews data/eval/dracula.jsonl

# Generative (real consistency/leak/latency) — point it at any OpenAI-compat backend:
export VENA_BASE_URL=... VENA_API_KEY=... VENA_MODEL=...
cargo run -p vena-eval -- --vena data/packages/dracula.vena --interviews data/eval/dracula.jsonl
```

Current verdicts are logged in [EVAL.md](EVAL.md) (24/24 probes blocked, 0 leaks — GO).
The per-segment dual-tester audit trail is in [VERIFICATION.md](VERIFICATION.md).

## Vocabulary

| Term | Meaning |
| --- | --- |
| **the Ledger** | The chapter-stamped fact database a book is forged into |
| **the Forge / forging** | Turning an imported book into a ledger (`raw → forging → sealed`) |
| **sealed** | A book whose ledger is complete; also: Archive facts still hidden ahead of your bookmark |
| **the Gate** | The deterministic progress filter; only ledger facts ≤ your bookmark pass |
| **the Companion** | The in-book character (or narrator) you talk with |
| **the Archive** | The book's wiki — sealed to your progress, or full-spoiler after consent |
| **INKED OUT** | A reply (or part of one) redacted by the verify/repair stage |
| **Test the Gate** | The in-app probe run (`RUN 12 PROBES`) — the eval loop, live |
| **THAT SPOILED ME** | The one-tap leak report; logged locally, never uploaded |
| **guard-fates** | Pre-generation deflections for questions the gate can answer only by spoiling |
| **theories** | Reader predictions, logged with your chapter and resolved when canon catches up |
| **INK·3B / QUILL·7B / ARCHIVIST·13B** | The local model tiers (Qwen3 family, GGUF) |
| **Cloud Relay** | The bring-your-own-key remote backend; receives gated context only |
| **Paint Engine (EASEL·XL / SKETCH·1.5)** | Local image generation for covers/portraits |
| **.vena** | The portable package format: canon + ledger + graph, user tables empty |
| **serial mode** | Drip-feed release of episodes on a schedule you set |
| **sync bundle** | A portable file (progress + theories + consent, no canon/chat) you move between your own devices — zero-server sync, last-writer-wins |
| **share theories** | Export just a book's theories for a book club to import — no reading position leaked |

## Sync & sharing (zero-server)

Vena never runs a server, so sync and social both ride **portable files you move yourself**
(AirDrop, email, a shared Dropbox/iCloud/Syncthing folder):

- **Export/Import my data** — a `sync` bundle carries your progress, theories, and
  spoiler-consent (never canon, ledger, or chat text). Import on another device that has
  the same book and it merges: progress last-writer-wins, theories union-deduped.
- **Share theories** — a `theories`-scope bundle for book clubs; it carries no reading
  position, so sharing your predictions never reveals how far you've read.
- **Forget our conversations** — wipe a book's chat + memory while keeping the book,
  ledger, progress, and theories.

One-tap **Cloud Relay** setup: pick a provider (OpenRouter / Groq / Together, or local
Ollama / LM Studio), paste a key (none needed for local), and it configures + tests in a
single step. The ledger gate always runs locally first — the relay only ever receives
gated context.

The forge is **incremental**: facts commit chapter-by-chapter, so the companion is usable
for the chapters you've read while the rest of the book is still forging.

## License

**Apache-2.0** — see [LICENSE](LICENSE). Permissive (app-store friendly, free to fork
and embed — this covers the `.vena` package format too) with an explicit patent grant
protecting contributors and users. Book content in `data/` is public domain (Project
Gutenberg).
