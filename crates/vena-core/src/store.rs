//! SQLite store (§5, Appendix A). Owns the **gate** — the deterministic SQL that
//! decides what the model may ever see. `schema.sql` is the single source of truth,
//! compiled in with include_str!.

use crate::error::{Result, VenaError};
use crate::model::*;
use crate::verify;
use rusqlite::{params, Connection, OptionalExtension};

pub const SCHEMA_SQL: &str = include_str!("../../../schema.sql");

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Store { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ---------- story ----------

    pub fn insert_story(
        &self,
        slug: &str,
        title: &str,
        author: Option<&str>,
        license: &str,
        source: Option<&str>,
        cover: Option<&str>,
        meta_json: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO story (slug,title,author,license,source,cover,meta_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![slug, title, author, license, source, cover, meta_json],
        )?;
        let id = self.conn.last_insert_rowid();
        // Every story starts at progress 0 (nothing read).
        self.conn.execute(
            "INSERT OR IGNORE INTO progress (story_id, episode_seq, scene_seq) VALUES (?1,0,0)",
            params![id],
        )?;
        Ok(id)
    }

    pub fn list_books(&self) -> Result<Vec<BookMeta>> {
        let ids: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM story ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        ids.into_iter().map(|id| self.get_book(id)).collect()
    }

    pub fn get_book(&self, id: i64) -> Result<BookMeta> {
        let (slug, title, author, license, source, cover, meta_json): (
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            String,
        ) = self
            .conn
            .query_row(
                "SELECT slug,title,author,license,source,cover,meta_json FROM story WHERE id=?1",
                params![id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| VenaError::NotFound(format!("story {id}")))?;

        let meta: serde_json::Value = serde_json::from_str(&meta_json).unwrap_or_default();
        let episode_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM episode WHERE story_id=?1",
            params![id],
            |r| r.get(0),
        )?;
        let fact_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM fact WHERE story_id=?1",
            params![id],
            |r| r.get(0),
        )?;
        let progress_episode: i64 = self
            .conn
            .query_row(
                "SELECT episode_seq FROM progress WHERE story_id=?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);

        let forge_state = match meta.get("forge_state").and_then(|v| v.as_str()) {
            Some("forging") => ForgeState::Forging,
            Some("sealed") => ForgeState::Sealed,
            _ => {
                if fact_count > 0 {
                    ForgeState::Sealed
                } else {
                    ForgeState::Raw
                }
            }
        };

        Ok(BookMeta {
            id,
            slug,
            title,
            author,
            license,
            source,
            cover,
            episode_count,
            progress_episode,
            ledger_coverage: meta
                .get("ledger_coverage")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32,
            fact_count,
            package_sha: meta
                .get("package_sha")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            profile: meta
                .get("profile")
                .and_then(|v| v.as_str())
                .unwrap_or("prose")
                .to_string(),
            forge_state,
        })
    }

    /// "Burn this book's data" (§11.4a): hard-delete everything for one story.
    pub fn burn_book(&self, id: i64) -> Result<()> {
        let tx = &self.conn;
        tx.execute("DELETE FROM message WHERE conversation_id IN (SELECT id FROM conversation WHERE story_id=?1)", params![id])?;
        tx.execute("DELETE FROM chat_memory WHERE conversation_id IN (SELECT id FROM conversation WHERE story_id=?1)", params![id])?;
        tx.execute("DELETE FROM conversation WHERE story_id=?1", params![id])?;
        // Story graph (v2.0): edges cite facts, so they burn before facts do.
        tx.execute("DELETE FROM edge WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM entity WHERE story_id=?1", params![id])?;
        tx.execute(
            "DELETE FROM scene WHERE episode_id IN (SELECT id FROM episode WHERE story_id=?1)",
            params![id],
        )?;
        tx.execute("DELETE FROM fact WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM theory WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM branch WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM character WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM episode WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM progress WHERE story_id=?1", params![id])?;
        tx.execute("DELETE FROM story WHERE id=?1", params![id])?;
        Ok(())
    }

    /// Set the cover asset path (covers are generated/replaced; canon is untouched).
    pub fn conn_execute_set_cover(&self, id: i64, cover: &str) -> Result<()> {
        self.conn
            .execute("UPDATE story SET cover=?2 WHERE id=?1", params![id, cover])?;
        Ok(())
    }

    pub fn set_book_meta(&self, id: i64, meta_json: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE story SET meta_json=?2 WHERE id=?1",
            params![id, meta_json],
        )?;
        Ok(())
    }

    pub fn book_meta_value(&self, id: i64) -> Result<serde_json::Value> {
        let s: String = self.conn.query_row(
            "SELECT meta_json FROM story WHERE id=?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&s).unwrap_or_default())
    }

    // ---------- episode / scene (canon — immutable, no UPDATE path) ----------

    pub fn insert_episode(
        &self,
        story_id: i64,
        seq: i64,
        title: Option<&str>,
        est_minutes: Option<i64>,
        content_html: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO episode (story_id,seq,title,est_minutes,content_html)
             VALUES (?1,?2,?3,?4,?5)",
            params![story_id, seq, title, est_minutes, content_html],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_episode(&self, story_id: i64, seq: i64) -> Result<EpisodeHtml> {
        let (id, title, est, html): (i64, Option<String>, Option<i64>, String) = self
            .conn
            .query_row(
                "SELECT id,title,est_minutes,content_html FROM episode WHERE story_id=?1 AND seq=?2",
                params![story_id, seq],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| VenaError::NotFound(format!("episode {story_id}/{seq}")))?;
        let scene_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM scene WHERE episode_id=?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(EpisodeHtml {
            seq,
            title,
            est_minutes: est,
            content_html: html,
            scene_count,
        })
    }

    pub fn insert_scene(&self, episode_id: i64, seq: i64, summary: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO scene (episode_id,seq,summary) VALUES (?1,?2,?3)",
            params![episode_id, seq, summary],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    // ---------- characters ----------

    pub fn insert_character(
        &self,
        story_id: i64,
        name: &str,
        aliases: &[String],
        voice: &VoiceCard,
        first_appearance_chapter: i64,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO character (story_id,name,aliases_json,voice_card_json,first_appearance_chapter)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                story_id,
                name,
                serde_json::to_string(aliases)?,
                serde_json::to_string(voice)?,
                first_appearance_chapter
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn row_to_character(r: &rusqlite::Row, progress: i64) -> rusqlite::Result<Character> {
        let id: i64 = r.get(0)?;
        let story_id: i64 = r.get(1)?;
        let name: String = r.get(2)?;
        let aliases_json: String = r.get(3)?;
        let voice_json: String = r.get(4)?;
        let first: i64 = r.get(5)?;
        Ok(Character {
            id,
            story_id,
            name,
            aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
            voice_card: serde_json::from_str(&voice_json).unwrap_or_default(),
            first_appearance_chapter: first,
            met: first <= progress,
        })
    }

    /// §11.2 `list_characters`: ONLY first_appearance ≤ progress are "met"; unmet
    /// are returned too but flagged `met=false` so the UI can silhouette them.
    pub fn list_characters(&self, story_id: i64) -> Result<Vec<Character>> {
        let progress = self.get_progress(story_id)?.0;
        let mut stmt = self.conn.prepare(
            "SELECT id,story_id,name,aliases_json,voice_card_json,first_appearance_chapter
             FROM character WHERE story_id=?1 ORDER BY first_appearance_chapter, name",
        )?;
        let rows = stmt
            .query_map(params![story_id], |r| Self::row_to_character(r, progress))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_character(&self, story_id: i64, character_id: i64) -> Result<Character> {
        let progress = self.get_progress(story_id)?.0;
        self.conn
            .query_row(
                "SELECT id,story_id,name,aliases_json,voice_card_json,first_appearance_chapter
                 FROM character WHERE id=?1 AND story_id=?2",
                params![character_id, story_id],
                |r| Self::row_to_character(r, progress),
            )
            .optional()?
            .ok_or_else(|| VenaError::NotFound(format!("character {character_id}")))
    }

    /// Names (incl. aliases) of characters not yet met — for the unmet_character check.
    pub fn unmet_character_names(&self, story_id: i64) -> Result<Vec<String>> {
        let progress = self.get_progress(story_id)?.0;
        let mut stmt = self.conn.prepare(
            "SELECT name,aliases_json FROM character WHERE story_id=?1 AND first_appearance_chapter > ?2",
        )?;
        let mut out = Vec::new();
        let rows = stmt.query_map(params![story_id, progress], |r| {
            let name: String = r.get(0)?;
            let aliases: String = r.get(1)?;
            Ok((name, aliases))
        })?;
        for row in rows {
            let (name, aliases_json) = row?;
            out.push(name);
            if let Ok(aliases) = serde_json::from_str::<Vec<String>>(&aliases_json) {
                out.extend(aliases);
            }
        }
        Ok(out)
    }

    // ---------- facts / the gate ----------

    pub fn insert_fact(&self, f: &Fact) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO fact (story_id,chapter_seq,subject_char_id,kind,text,known_by_json,spoiler_weight)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                f.story_id,
                f.chapter_seq,
                f.subject_char_id,
                f.kind.as_str(),
                f.text,
                serde_json::to_string(&f.known_by)?,
                f.spoiler_weight
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn load_facts(&self, story_id: i64) -> Result<Vec<Fact>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,story_id,chapter_seq,subject_char_id,kind,text,known_by_json,spoiler_weight
             FROM fact WHERE story_id=?1 ORDER BY chapter_seq, id",
        )?;
        let rows = stmt
            .query_map(params![story_id], |r| {
                let known_by_json: String = r.get(6)?;
                let kind: String = r.get(4)?;
                Ok(Fact {
                    id: r.get(0)?,
                    story_id: r.get(1)?,
                    chapter_seq: r.get(2)?,
                    subject_char_id: r.get(3)?,
                    kind: FactKind::parse(&kind),
                    text: r.get(5)?,
                    known_by: serde_json::from_str(&known_by_json).unwrap_or_default(),
                    spoiler_weight: r.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// STAGE 1 — THE GATE. Facts the model may see: `chapter_seq ≤ progress`, and
    /// (character chat) the character must know it per `known_by`. Then hybrid
    /// retrieval ranks by relevance to the user message and takes top-k.
    ///
    /// This is the whole ballgame: the weak on-device model never *decides* what it
    /// knows — this SQL does, before the model is invoked.
    pub fn gated_facts(
        &self,
        story_id: i64,
        progress: i64,
        character_id: Option<i64>,
        query: &str,
        k: usize,
    ) -> Result<Vec<Fact>> {
        let all = self.load_facts(story_id)?;
        let mut visible: Vec<Fact> = all
            .into_iter()
            .filter(|f| f.chapter_seq <= progress)
            .filter(|f| match character_id {
                None => true, // narrator: everything the reader has read
                Some(cid) => f
                    .known_by
                    .iter()
                    .any(|kb| kb.character_id == cid && kb.learned_at_chapter <= progress),
            })
            .collect();

        // Hybrid retrieval: rank by lexical relevance to the message; keep top-k.
        if !query.trim().is_empty() {
            visible.sort_by(|a, b| {
                verify::similarity(query, &b.text)
                    .partial_cmp(&verify::similarity(query, &a.text))
                    .unwrap()
            });
        }
        visible.truncate(k);
        Ok(visible)
    }

    /// Facts that must NEVER reach the model for this turn — the verifier checks
    /// generated claims against these. = future facts (chapter > progress) plus,
    /// in character mode, facts the reader knows but this character does not yet.
    pub fn forbidden_facts(
        &self,
        story_id: i64,
        progress: i64,
        character_id: Option<i64>,
    ) -> Result<Vec<Fact>> {
        let all = self.load_facts(story_id)?;
        Ok(all
            .into_iter()
            .filter(|f| {
                if f.chapter_seq > progress {
                    return true;
                }
                match character_id {
                    None => false,
                    Some(cid) => !f
                        .known_by
                        .iter()
                        .any(|kb| kb.character_id == cid && kb.learned_at_chapter <= progress),
                }
            })
            .collect())
    }

    pub fn facts_at_or_before(&self, story_id: i64, progress: i64) -> Result<Vec<Fact>> {
        Ok(self
            .load_facts(story_id)?
            .into_iter()
            .filter(|f| f.chapter_seq <= progress)
            .collect())
    }

    /// Future weight≥2 facts, used by "Test the Gate — RUN 12 PROBES" (§11.4a).
    pub fn future_probe_facts(&self, story_id: i64, progress: i64) -> Result<Vec<Fact>> {
        Ok(self
            .load_facts(story_id)?
            .into_iter()
            .filter(|f| f.chapter_seq > progress && f.spoiler_weight >= 2)
            .collect())
    }

    // ---------- progress ----------

    pub fn get_progress(&self, story_id: i64) -> Result<(i64, i64)> {
        Ok(self
            .conn
            .query_row(
                "SELECT episode_seq,scene_seq FROM progress WHERE story_id=?1",
                params![story_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0)))
    }

    /// Set progress. Returns whether this was a REWIND (new < old), which triggers
    /// re-seal-on-re-read handling in the engine layer.
    pub fn set_progress(&self, story_id: i64, episode_seq: i64, scene_seq: i64) -> Result<bool> {
        let (old, _) = self.get_progress(story_id)?;
        self.conn.execute(
            "INSERT INTO progress (story_id,episode_seq,scene_seq,updated_at)
             VALUES (?1,?2,?3,datetime('now'))
             ON CONFLICT(story_id) DO UPDATE SET episode_seq=?2, scene_seq=?3, updated_at=datetime('now')",
            params![story_id, episode_seq, scene_seq],
        )?;
        Ok(episode_seq < old)
    }

    // ---------- conversations / messages ----------

    /// The most recent non-archived conversation for a (book, character), if any.
    pub fn find_active_conversation(
        &self,
        story_id: i64,
        character_id: Option<i64>,
    ) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM conversation WHERE story_id=?1 AND archived=0
                 AND ((character_id IS NULL AND ?2 IS NULL) OR character_id=?2)
                 ORDER BY id DESC LIMIT 1",
                params![story_id, character_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn slug_exists(&self, slug: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM story WHERE slug=?1", params![slug], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
            .is_some())
    }

    pub fn create_conversation(&self, story_id: i64, character_id: Option<i64>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO conversation (story_id,character_id) VALUES (?1,?2)",
            params![story_id, character_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_message(
        &self,
        conversation_id: i64,
        role: &str,
        text: &str,
        pinned_progress: i64,
        verify_json: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO message (conversation_id,role,text,pinned_progress,verify_json)
             VALUES (?1,?2,?3,?4,?5)",
            params![conversation_id, role, text, pinned_progress, verify_json],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Re-seal on re-read (§11.4a): archive conversations/messages stamped after a
    /// rewind point (never delete). Restored when progress passes them again.
    pub fn reseal_after(&self, story_id: i64, progress: i64) -> Result<()> {
        // Archive conversations whose messages are all beyond the new position.
        self.conn.execute(
            "UPDATE conversation SET archived=1
             WHERE story_id=?1 AND id IN (
               SELECT conversation_id FROM message
               GROUP BY conversation_id HAVING MIN(pinned_progress) > ?2)",
            params![story_id, progress],
        )?;
        // Restore any that are back in range.
        self.conn.execute(
            "UPDATE conversation SET archived=0
             WHERE story_id=?1 AND id IN (
               SELECT conversation_id FROM message
               GROUP BY conversation_id HAVING MIN(pinned_progress) <= ?2)",
            params![story_id, progress],
        )?;
        Ok(())
    }

    // ---------- theories (resolution gated to progress) ----------

    pub fn add_theory(&self, story_id: i64, text: &str, logged_at_chapter: i64) -> Result<Theory> {
        self.conn.execute(
            "INSERT INTO theory (story_id,text,logged_at_chapter) VALUES (?1,?2,?3)",
            params![story_id, text, logged_at_chapter],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Theory {
            id,
            text: text.to_string(),
            logged_at_chapter,
            resolved_status: None,
            resolved_at_chapter: None,
        })
    }

    pub fn list_theories(&self, story_id: i64) -> Result<Vec<Theory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,text,logged_at_chapter,resolved_status,resolved_at_chapter
             FROM theory WHERE story_id=?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![story_id], |r| {
                Ok(Theory {
                    id: r.get(0)?,
                    text: r.get(1)?,
                    logged_at_chapter: r.get(2)?,
                    resolved_status: r.get(3)?,
                    resolved_at_chapter: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_theory_resolution(
        &self,
        id: i64,
        status: &str,
        resolved_at_chapter: i64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE theory SET resolved_status=?2, resolved_at_chapter=?3 WHERE id=?1",
            params![id, status, resolved_at_chapter],
        )?;
        Ok(())
    }

    /// Re-seal wipes resolutions stamped after a rewind point (theory re-opens).
    pub fn reopen_theories_after(&self, story_id: i64, progress: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE theory SET resolved_status=NULL, resolved_at_chapter=NULL
             WHERE story_id=?1 AND resolved_at_chapter > ?2",
            params![story_id, progress],
        )?;
        Ok(())
    }

    // ---------- branches (what-ifs, always AI-labeled) ----------

    pub fn add_branch(
        &self,
        story_id: i64,
        forked_at_episode: i64,
        title: &str,
        content_html: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO branch (story_id,forked_at_episode,title,content_html,ai_label)
             VALUES (?1,?2,?3,?4,1)",
            params![story_id, forked_at_episode, title, content_html],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_branches(&self, story_id: i64) -> Result<Vec<(i64, i64, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,forked_at_episode,title,content_html FROM branch WHERE story_id=?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![story_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---------- settings ----------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM setting WHERE key=?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO setting (key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=?2",
            params![key, value],
        )?;
        Ok(())
    }
}
