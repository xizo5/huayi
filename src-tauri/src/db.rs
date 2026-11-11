use crate::models::{ReviewItem, Settings, Word};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS words (
    id            TEXT PRIMARY KEY,
    word          TEXT NOT NULL UNIQUE,
    phonetic      TEXT NOT NULL DEFAULT '',
    pos           TEXT NOT NULL DEFAULT '',
    definition    TEXT NOT NULL DEFAULT '',
    context       TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL,
    familiarity   INTEGER NOT NULL DEFAULT 0,
    reviewed_at   INTEGER
);

CREATE TABLE IF NOT EXISTS questions (
    id            TEXT PRIMARY KEY,
    word          TEXT NOT NULL,
    sentence      TEXT NOT NULL,
    sentence_zh   TEXT NOT NULL DEFAULT '',
    options       TEXT NOT NULL,
    answer_index  INTEGER NOT NULL,
    explanation   TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL,
    chosen        INTEGER,
    correct       INTEGER
);

CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_words_created ON words(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_q_word ON questions(word);
"#;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // ---------- 设置 ----------

    pub fn load_settings(&self) -> Settings {
        let conn = self.conn.lock().unwrap();
        let raw: Option<String> = conn
            .query_row("SELECT value FROM kv WHERE key = 'settings'", [], |r| r.get(0))
            .optional()
            .unwrap_or(None);
        match raw {
            Some(s) => serde_json::from_str(&s).unwrap_or_default(),
            None => Settings::default(),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let json = serde_json::to_string(s)?;
        conn.execute(
            "INSERT INTO kv (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![json],
        )?;
        Ok(())
    }

    // ---------- 一次性标志 ----------

    pub fn get_flag(&self, key: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        let v: Option<String> = conn
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .unwrap_or(None);
        v.as_deref() == Some("1")
    }

    pub fn set_flag(&self, key: &str, val: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, if val { "1" } else { "0" }],
        )?;
        Ok(())
    }

    // ---------- 生词本 ----------

    pub fn add_word(&self, w: &Word) -> Result<Word> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row("SELECT id FROM words WHERE word = ?1", params![w.word], |r| r.get(0))
            .optional()?;

        if let Some(id) = existing {
            conn.execute(
                "UPDATE words SET definition = ?1, phonetic = ?2, pos = ?3, context = ?4
                 WHERE id = ?5",
                params![w.definition, w.phonetic, w.pos, w.context, id],
            )?;
            return Ok(get_word(&conn, &id)?.expect("word must exist after update"));
        }

        conn.execute(
            "INSERT INTO words (id, word, phonetic, pos, definition, context, created_at, familiarity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![w.id, w.word, w.phonetic, w.pos, w.definition, w.context, w.created_at],
        )?;
        Ok(get_word(&conn, &w.id)?.expect("word must exist after insert"))
    }

    pub fn list_words(&self, query: Option<&str>, limit: i64) -> Result<Vec<Word>> {
        let conn = self.conn.lock().unwrap();
        let sql = "SELECT id, word, phonetic, pos, definition, context, created_at, familiarity, reviewed_at
                   FROM words
                   WHERE (?1 = '' OR word LIKE ?2 OR definition LIKE ?2 OR context LIKE ?2)
                   ORDER BY created_at DESC
                   LIMIT ?3";
        let q = query.unwrap_or("").trim();
        let like = format!("%{}%", q);
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![q, like, limit], map_word)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn exists(&self, word: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM words WHERE word = ?1",
            params![word],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn delete_word(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM words WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn word_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))?;
        Ok(n)
    }

    pub fn pending_review_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM questions WHERE correct IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 挑出还没出过题的词，用于预生成
    pub fn words_for_generation(&self, limit: i64) -> Result<Vec<Word>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, word, phonetic, pos, definition, context, created_at, familiarity, reviewed_at
             FROM words
             WHERE word NOT IN (SELECT word FROM questions)
             ORDER BY familiarity ASC, RANDOM()
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], map_word)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 全部词都已出过题时，复用最久没考过的词重新出题
    pub fn words_for_regeneration(&self, limit: i64) -> Result<Vec<Word>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, word, phonetic, pos, definition, context, created_at, familiarity, reviewed_at
             FROM words
             ORDER BY familiarity ASC, RANDOM()
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], map_word)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn insert_questions(&self, items: &[ReviewItem]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        for it in items {
            let opts = serde_json::to_string(&it.options)?;
            conn.execute(
                "INSERT OR REPLACE INTO questions
                 (id, word, sentence, sentence_zh, options, answer_index, explanation, created_at, chosen, correct)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)",
                params![
                    it.id,
                    it.word,
                    it.sentence,
                    it.sentence_zh,
                    opts,
                    it.answer_index,
                    it.explanation,
                    now
                ],
            )?;
        }
        Ok(())
    }

    pub fn pending_questions(&self, limit: i64) -> Result<Vec<ReviewItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, word, sentence, sentence_zh, options, answer_index, explanation,
                    chosen, correct
             FROM questions
             WHERE correct IS NULL
             ORDER BY created_at ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let opts: String = row.get(4)?;
            let chosen: Option<i64> = row.get(7)?;
            let correct: Option<i64> = row.get(8)?;
            Ok(ReviewItem {
                id: row.get(0)?,
                word: row.get(1)?,
                sentence: row.get(2)?,
                sentence_zh: row.get(3)?,
                options: serde_json::from_str(&opts).unwrap_or_default(),
                answer_index: row.get(5)?,
                explanation: row.get(6)?,
                answered: correct.is_some(),
                chosen,
                correct: correct.map(|c| c != 0),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn mastered_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM words WHERE familiarity >= 5",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 答题。返回是否答对，同时更新该词的熟悉度。
    pub fn answer(&self, id: &str, chosen: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();

        let answer_index: i64 = conn.query_row(
            "SELECT answer_index FROM questions WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        let correct = chosen == answer_index;

        conn.execute(
            "UPDATE questions SET chosen = ?1, correct = ?2 WHERE id = ?3",
            params![chosen, if correct { 1 } else { 0 }, id],
        )?;

        let word: Option<String> = conn
            .query_row("SELECT word FROM questions WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?;

        if let Some(w) = word {
            let delta = if correct { 1 } else { -1 };
            conn.execute(
                "UPDATE words
                 SET familiarity = MAX(0, MIN(5, familiarity + ?1)),
                     reviewed_at = ?2
                 WHERE word = ?3",
                params![delta, chrono::Utc::now().timestamp(), w],
            )?;
        }
        Ok(correct)
    }

    /// 清掉昨天以前的已答题，避免表无限增长
    pub fn prune_answered(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let cutoff = chrono::Utc::now().timestamp() - 60 * 60 * 24 * 7;
        conn.execute(
            "DELETE FROM questions WHERE correct IS NOT NULL AND created_at < ?1",
            params![cutoff],
        )?;
        Ok(())
    }
}

fn map_word(row: &rusqlite::Row<'_>) -> rusqlite::Result<Word> {
    Ok(Word {
        id: row.get(0)?,
        word: row.get(1)?,
        phonetic: row.get(2)?,
        pos: row.get(3)?,
        definition: row.get(4)?,
        context: row.get(5)?,
        created_at: row.get(6)?,
        familiarity: row.get(7)?,
        reviewed_at: row.get(8)?,
    })
}

fn get_word(conn: &Connection, id: &str) -> Result<Option<Word>> {
    let w = conn
        .query_row(
            "SELECT id, word, phonetic, pos, definition, context, created_at, familiarity, reviewed_at
             FROM words WHERE id = ?1",
            params![id],
            map_word,
        )
        .optional()?;
    Ok(w)
}
