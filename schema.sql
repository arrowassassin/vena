-- Vena — single source of truth for the profile & .vena package schema.
-- Included via include_str! into vena-core. See system-design.md Appendix A / §5.
-- Vocabulary lock: the ledger is FORGED (parse -> facts -> seal); the Archive is
-- SEALED (synced) or UNSEALED (full-spoiler, behind consent). Canon is IMMUTABLE.

CREATE TABLE IF NOT EXISTS story (
  id INTEGER PRIMARY KEY, slug TEXT NOT NULL UNIQUE, title TEXT NOT NULL,
  author TEXT, license TEXT NOT NULL DEFAULT 'user-owned', source TEXT,
  cover TEXT,
  meta_json TEXT NOT NULL DEFAULT '{}');

-- Canon text. IMMUTABLE by convention: no UPDATE path exists in the app.
CREATE TABLE IF NOT EXISTS episode (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  seq INTEGER NOT NULL, title TEXT, est_minutes INTEGER, content_html TEXT NOT NULL,
  UNIQUE (story_id, seq));

CREATE TABLE IF NOT EXISTS scene (
  id INTEGER PRIMARY KEY, episode_id INTEGER NOT NULL REFERENCES episode(id),
  seq INTEGER NOT NULL, summary TEXT NOT NULL,
  embedding BLOB);

CREATE TABLE IF NOT EXISTS character (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  name TEXT NOT NULL, aliases_json TEXT NOT NULL DEFAULT '[]',
  voice_card_json TEXT NOT NULL DEFAULT '{}',
  first_appearance_chapter INTEGER NOT NULL DEFAULT 1);

-- THE KNOWLEDGE LEDGER. chapter_seq = when the READER learns the fact;
-- known_by_json = [{character_id, learned_at_chapter}] (characters may lag the reader).
CREATE TABLE IF NOT EXISTS fact (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  chapter_seq INTEGER NOT NULL, scene_id INTEGER REFERENCES scene(id),
  subject_char_id INTEGER REFERENCES character(id),
  kind TEXT NOT NULL,              -- event|relationship|secret|world|death|reveal
  text TEXT NOT NULL,              -- atomic, single clause
  known_by_json TEXT NOT NULL DEFAULT '[]',
  spoiler_weight INTEGER NOT NULL DEFAULT 1,  -- 0 ambient..3 twist
  embedding BLOB);
CREATE INDEX IF NOT EXISTS idx_fact_gate ON fact (story_id, chapter_seq);

CREATE TABLE IF NOT EXISTS progress (
  story_id INTEGER PRIMARY KEY REFERENCES story(id),
  episode_seq INTEGER NOT NULL DEFAULT 0, scene_seq INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT (datetime('now')));

CREATE TABLE IF NOT EXISTS conversation (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  character_id INTEGER REFERENCES character(id),  -- NULL = narrator mode
  archived INTEGER NOT NULL DEFAULT 0,            -- re-seal on re-read archives, never deletes
  created_at TEXT NOT NULL DEFAULT (datetime('now')));

CREATE TABLE IF NOT EXISTS message (
  id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id),
  role TEXT NOT NULL, text TEXT NOT NULL,
  pinned_progress INTEGER NOT NULL,          -- reader chapter at send time (audit)
  verify_json TEXT NOT NULL DEFAULT '{}');   -- claims, violations, repairs

-- Progress-stamped per-character relationship memory (§6b). Obeys the same gate.
CREATE TABLE IF NOT EXISTS chat_memory (
  id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id),
  text TEXT NOT NULL, created_at_progress INTEGER NOT NULL);

-- AI content lives ONLY in message/branch — never in episode.
CREATE TABLE IF NOT EXISTS branch (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  forked_at_episode INTEGER NOT NULL, title TEXT NOT NULL, content_html TEXT NOT NULL,
  ai_label INTEGER NOT NULL DEFAULT 1);

-- Theory board: resolved ONLY once reader passes the reveal.
CREATE TABLE IF NOT EXISTS theory (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  text TEXT NOT NULL, logged_at_chapter INTEGER NOT NULL,
  resolved_status TEXT, resolved_at_chapter INTEGER);

-- Story graph (v2.0, §6b) — a DERIVED VIEW of the ledger, never a second source of
-- truth. Non-character entities (places/factions/items); characters stay in
-- `character`. Edges reference entities by string key ("char:<id>" | "place:<id>" | …).
CREATE TABLE IF NOT EXISTS entity (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  kind TEXT NOT NULL,                 -- place | faction | item
  name TEXT NOT NULL, aliases_json TEXT NOT NULL DEFAULT '[]',
  first_appearance_chapter INTEGER NOT NULL DEFAULT 1,
  meta_json TEXT NOT NULL DEFAULT '{}');

-- Chapter-stamped relationship edges. Graph traversal is spoiler-gated BY
-- CONSTRUCTION: queries only walk edges with since_chapter <= progress and
-- (until_chapter IS NULL OR until_chapter > progress). Every edge cites a fact.
CREATE TABLE IF NOT EXISTS edge (
  id INTEGER PRIMARY KEY, story_id INTEGER NOT NULL REFERENCES story(id),
  from_entity TEXT NOT NULL,          -- "char:3" | "place:7" | ...
  to_entity TEXT NOT NULL,
  rel_type TEXT NOT NULL,             -- knows | allied_with | loves | betrays | hunts | at_place | ...
  since_chapter INTEGER NOT NULL,
  until_chapter INTEGER,              -- NULL = still valid
  source_fact_id INTEGER REFERENCES fact(id));
CREATE INDEX IF NOT EXISTS idx_edge_gate ON edge (story_id, since_chapter);

CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
