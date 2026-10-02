use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::domain::{Filter, Role, SearchHit, Session, Tool, Turn};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("unknown role: {0}")]
    UnknownRole(String),
    #[error("session id {0} is ambiguous, matches: {1}")]
    AmbiguousSession(String, String),
}

const INIT_SQL: &str = "
CREATE TABLE IF NOT EXISTS session (
  id          TEXT PRIMARY KEY,
  tool        TEXT NOT NULL,
  title       TEXT,
  project     TEXT,
  model       TEXT,
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER,
  tokens_in   INTEGER NOT NULL DEFAULT 0,
  tokens_out  INTEGER NOT NULL DEFAULT 0,
  cost_usd    REAL,
  indexed_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS turn (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id  TEXT    NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  seq         INTEGER NOT NULL,
  role        TEXT    NOT NULL,
  tool_name   TEXT,
  tool_input  TEXT,
  content     TEXT    NOT NULL DEFAULT '',
  ts          INTEGER,
  tokens      INTEGER NOT NULL DEFAULT 0,
  UNIQUE(session_id, seq)
);

CREATE INDEX IF NOT EXISTS turn_session ON turn(session_id, seq);
CREATE INDEX IF NOT EXISTS turn_tool    ON turn(tool_name) WHERE tool_name IS NOT NULL;

CREATE TABLE IF NOT EXISTS turn_chunk (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT    NOT NULL,
  turn_seq   INTEGER NOT NULL,
  chunk_seq  INTEGER NOT NULL,
  content    TEXT    NOT NULL,
  UNIQUE(session_id, turn_seq, chunk_seq)
);

CREATE INDEX IF NOT EXISTS chunk_turn ON turn_chunk(session_id, turn_seq);

CREATE VIRTUAL TABLE IF NOT EXISTS turn_fts USING fts5(
  content,
  content=turn_chunk,
  content_rowid=id,
  tokenize='unicode61 remove_diacritics 1'
);

CREATE TRIGGER IF NOT EXISTS chunk_ai AFTER INSERT ON turn_chunk BEGIN
  INSERT INTO turn_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS chunk_ad AFTER DELETE ON turn_chunk BEGIN
  INSERT INTO turn_fts(turn_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;

CREATE TRIGGER IF NOT EXISTS chunk_au AFTER UPDATE ON turn_chunk BEGIN
  INSERT INTO turn_fts(turn_fts, rowid, content) VALUES ('delete', old.id, old.content);
  INSERT INTO turn_fts(rowid, content) VALUES (new.id, new.content);
END;
";

const CHUNK_THRESHOLD: usize = 800;
const CHUNK_SIZE: usize = 700;
const CHUNK_OVERLAP: usize = 150;

fn chunk_content(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    let total = chars.len();
    if total <= CHUNK_THRESHOLD {
        return vec![content.to_owned()];
    }
    let step = CHUNK_SIZE - CHUNK_OVERLAP;
    (0..)
        .map(|i| i * step)
        .take_while(|&start| start < total)
        .map(|start| chars[start..(start + CHUNK_SIZE).min(total)].iter().collect())
        .collect()
}

fn migrate_v2(conn: &Connection) -> Result<(), DbError> {
    let chunk_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_chunk'",
        [],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0;
    if chunk_exists {
        return Ok(());
    }
    let session_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='session'",
        [],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0;
    conn.execute_batch("
        DROP TRIGGER IF EXISTS turn_ai;
        DROP TRIGGER IF EXISTS turn_ad;
        DROP TRIGGER IF EXISTS turn_au;
        DROP TABLE IF EXISTS turn_fts;
    ")?;
    if session_exists {
        conn.execute_batch("UPDATE session SET indexed_at = 0")?;
    }
    Ok(())
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &str) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.query_row("PRAGMA mmap_size=536870912", [], |_| Ok(()))?;
        migrate_v2(&conn)?;
        conn.execute_batch(INIT_SQL)?;
        Ok(Db { conn })
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(INIT_SQL)?;
        Ok(Db { conn })
    }

    pub fn upsert_session(&self, s: &Session) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO session
             (id, tool, title, project, model, started_at, ended_at, tokens_in, tokens_out, cost_usd, indexed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                s.id, s.tool.as_str(), s.title, s.project, s.model,
                s.started_at, s.ended_at, s.tokens_input, s.tokens_output,
                s.cost_usd, now_ms()
            ],
        )?;
        Ok(())
    }

    pub fn upsert_turns(&mut self, turns: &[Turn]) -> Result<(), DbError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt_turn = tx.prepare_cached(
                "INSERT OR REPLACE INTO turn (session_id,seq,role,tool_name,tool_input,content,ts,tokens)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )?;
            let mut stmt_del = tx.prepare_cached(
                "DELETE FROM turn_chunk WHERE session_id=?1 AND turn_seq=?2",
            )?;
            let mut stmt_chunk = tx.prepare_cached(
                "INSERT INTO turn_chunk (session_id, turn_seq, chunk_seq, content) VALUES (?1,?2,?3,?4)",
            )?;
            turns.iter().try_for_each(|t| -> Result<(), DbError> {
                stmt_turn.execute(params![
                    t.session_id, t.seq, t.role.as_str(),
                    t.tool_name, t.tool_input, t.content, t.ts, t.tokens
                ])?;
                stmt_del.execute(params![t.session_id, t.seq])?;
                chunk_content(&t.content)
                    .into_iter()
                    .enumerate()
                    .try_for_each(|(i, chunk)| -> Result<(), DbError> {
                        stmt_chunk.execute(params![t.session_id, t.seq, i as i32, chunk])?;
                        Ok(())
                    })
            })?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn sessions(&self, filter: &Filter) -> Result<Vec<Session>, DbError> {
        let limit = filter.limit.unwrap_or(50) as i64;
        let tool = filter.tool.as_ref().map(|t| t.as_str());
        let mut stmt = self.conn.prepare(
            "SELECT id,tool,title,project,model,started_at,ended_at,tokens_in,tokens_out,cost_usd
             FROM session
             WHERE (?2 IS NULL OR tool = ?2)
               AND (?3 IS NULL OR started_at >= ?3)
             ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit, tool, filter.since_ms], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?, row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?, row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?, row.get::<_, Option<f64>>(9)?))
        })?;

        rows.map(|r| {
            let (id, tool_str, title, project, model, started_at, ended_at, tokens_input, tokens_output, cost_usd) = r?;
            let tool = Tool::from_str(&tool_str)
                .ok_or_else(|| rusqlite::Error::InvalidParameterName(tool_str.clone()))?;
            Ok(Session { id, tool, title, project, model, started_at, ended_at, tokens_input, tokens_output, cost_usd })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::Sqlite)
    }

    pub fn resolve_session_id(&self, id: &str) -> Result<String, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM session
             WHERE id = ?1 OR substr(id, 1, length(?1)) = ?1
                OR substr(id, instr(id, ':') + 1, length(?1)) = ?1
             ORDER BY (id = ?1) DESC, id LIMIT 6",
        )?;
        let ids = stmt
            .query_map(params![id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        match ids.as_slice() {
            [] => Ok(id.to_owned()),
            [only] => Ok(only.clone()),
            [first, ..] if first == id => Ok(first.clone()),
            many => Err(DbError::AmbiguousSession(id.to_owned(), many.join(", "))),
        }
    }

    pub fn turns(&self, session_id: &str) -> Result<Vec<Turn>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id,session_id,seq,role,tool_name,tool_input,content,ts,tokens
             FROM turn WHERE session_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i32>(2)?,
                row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?, row.get::<_, String>(6)?,
                row.get::<_, Option<i64>>(7)?, row.get::<_, i64>(8)?))
        })?;

        rows.map(|r| {
            let (id, session_id, seq, role_str, tool_name, tool_input, content, ts, tokens) = r?;
            let role = Role::from_str(&role_str)
                .ok_or_else(|| rusqlite::Error::InvalidParameterName(role_str.clone()))?;
            Ok(Turn { id, session_id, seq, role, tool_name, tool_input, content, ts, tokens })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::Sqlite)
    }

    pub fn search(&self, query: &str, filter: &Filter) -> Result<Vec<SearchHit>, DbError> {
        let limit = filter.limit.unwrap_or(20) as i64;
        let tool = filter.tool.as_ref().map(|t| t.as_str());

        let mut stmt = self.conn.prepare(
            "SELECT tc.session_id, s.tool, tc.turn_seq, t.role, t.tool_name,
                    snippet(turn_fts, 0, '[', ']', '…', 24) AS snippet,
                    turn_fts.rank,
                    s.started_at, s.project
             FROM turn_fts
             JOIN turn_chunk tc ON tc.id = turn_fts.rowid
             JOIN turn t        ON t.session_id = tc.session_id AND t.seq = tc.turn_seq
             JOIN session s     ON s.id = tc.session_id
             WHERE turn_fts MATCH ?1
               AND (?3 IS NULL OR s.tool = ?3)
               AND (?4 IS NULL OR s.started_at >= ?4)
             ORDER BY turn_fts.rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit, tool, filter.since_ms], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, i32>(2)?, row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?,
                row.get::<_, f64>(6)?, row.get::<_, i64>(7)?,
                row.get::<_, Option<String>>(8)?))
        })?;

        rows.map(|r| {
            let (session_id, tool_str, seq, role_str, tool_name, snippet, rank, started_at, project) = r?;
            let tool = Tool::from_str(&tool_str)
                .ok_or_else(|| rusqlite::Error::InvalidParameterName(tool_str.clone()))?;
            let role = Role::from_str(&role_str)
                .ok_or_else(|| rusqlite::Error::InvalidParameterName(role_str.clone()))?;
            Ok(SearchHit { session_id, tool, seq, role, tool_name, snippet, rank, started_at, project })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(DbError::Sqlite)
    }

    pub fn has_session(&self, id: &str, since: i64) -> Result<bool, DbError> {
        let result: Option<i64> = self.conn.query_row(
            "SELECT indexed_at FROM session WHERE id = ?1",
            params![id],
            |row| row.get(0),
        ).optional()?;
        Ok(result.map(|t| t >= since).unwrap_or(false))
    }

    pub fn stats(&self, session_id: Option<&str>) -> Result<Vec<(Tool, i64, i64, i64, f64)>, DbError> {
        match session_id {
            Some(id) => {
                let mut stmt = self.conn.prepare(
                    "SELECT tool, 1, tokens_in, tokens_out, COALESCE(cost_usd,0)
                     FROM session WHERE id = ?1",
                )?;
                let rows = stmt.query_map(params![id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, f64>(4)?))
                })?;
                collect_stats(rows)
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT tool, COUNT(*), SUM(tokens_in), SUM(tokens_out), SUM(COALESCE(cost_usd,0))
                     FROM session GROUP BY tool ORDER BY tool",
                )?;
                let rows = stmt.query_map(params![], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, f64>(4)?))
                })?;
                collect_stats(rows)
            }
        }
    }
}

fn collect_stats(
    rows: impl Iterator<Item = Result<(String, i64, i64, i64, f64), rusqlite::Error>>,
) -> Result<Vec<(Tool, i64, i64, i64, f64)>, DbError> {
    rows.map(|r| {
        let (tool_str, sessions, tin, tout, cost) = r?;
        let tool = Tool::from_str(&tool_str)
            .ok_or_else(|| rusqlite::Error::InvalidParameterName(tool_str.clone()))?;
        Ok((tool, sessions, tin, tout, cost))
    })
    .collect::<Result<Vec<_>, _>>()
    .map_err(DbError::Sqlite)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str, tool: Tool) -> Session {
        Session {
            id: id.to_string(),
            tool,
            title: Some("Test session".to_string()),
            project: Some("/Users/test/proj".to_string()),
            model: Some("test-model".to_string()),
            started_at: 1_000_000,
            ended_at: Some(1_001_000),
            tokens_input: 100,
            tokens_output: 50,
            cost_usd: Some(0.001),
        }
    }

    fn make_turn(session_id: &str, seq: i32, role: Role, content: &str) -> Turn {
        Turn {
            id: 0,
            session_id: session_id.to_string(),
            seq,
            role,
            tool_name: None,
            tool_input: None,
            content: content.to_string(),
            ts: Some(1_000_000 + seq as i64 * 1000),
            tokens: 10,
        }
    }

    fn long_content(n: usize) -> String {
        "word ".repeat(n)
    }

    #[test]
    fn open_in_memory_succeeds() {
        Db::open_in_memory().unwrap();
    }

    #[test]
    fn upsert_and_retrieve_session() {
        let db = Db::open_in_memory().unwrap();
        let s = make_session("sess-1", Tool::Claude);
        db.upsert_session(&s).unwrap();

        let sessions = db.sessions(&Filter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "sess-1");
        assert_eq!(sessions[0].tool, Tool::Claude);
        assert_eq!(sessions[0].tokens_input, 100);
    }

    #[test]
    fn upsert_session_replaces_on_conflict() {
        let db = Db::open_in_memory().unwrap();
        let s1 = make_session("sess-1", Tool::Claude);
        db.upsert_session(&s1).unwrap();

        let mut s2 = make_session("sess-1", Tool::Claude);
        s2.tokens_input = 999;
        db.upsert_session(&s2).unwrap();

        let sessions = db.sessions(&Filter::default()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].tokens_input, 999);
    }

    #[test]
    fn sessions_filter_by_tool() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("s1", Tool::Claude)).unwrap();
        db.upsert_session(&make_session("s2", Tool::Codex)).unwrap();

        let filter = Filter { tool: Some(Tool::Claude), ..Default::default() };
        let sessions = db.sessions(&filter).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
    }

    #[test]
    fn sessions_filter_by_since() {
        let db = Db::open_in_memory().unwrap();
        let mut s1 = make_session("s1", Tool::Claude);
        s1.started_at = 500;
        let mut s2 = make_session("s2", Tool::Claude);
        s2.started_at = 2_000_000;
        db.upsert_session(&s1).unwrap();
        db.upsert_session(&s2).unwrap();

        let filter = Filter { since_ms: Some(1_000_000), ..Default::default() };
        let sessions = db.sessions(&filter).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s2");
    }

    #[test]
    fn upsert_and_retrieve_turns() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("sess-1", Tool::Claude)).unwrap();

        let turns = vec![
            make_turn("sess-1", 0, Role::User, "hello world"),
            make_turn("sess-1", 1, Role::Assistant, "hi there"),
        ];
        db.upsert_turns(&turns).unwrap();

        let retrieved = db.turns("sess-1").unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].role, Role::User);
        assert_eq!(retrieved[0].content, "hello world");
        assert_eq!(retrieved[1].role, Role::Assistant);
    }

    #[test]
    fn resolve_session_id_accepts_prefixes_with_or_without_tool() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("claude:220f7528-c2e6", Tool::Claude)).unwrap();
        db.upsert_session(&make_session("claude:47f31aaa-0000", Tool::Claude)).unwrap();
        assert_eq!(db.resolve_session_id("claude:220f7").unwrap(), "claude:220f7528-c2e6");
        assert_eq!(db.resolve_session_id("220f7528").unwrap(), "claude:220f7528-c2e6");
        assert_eq!(db.resolve_session_id("claude:220f7528-c2e6").unwrap(), "claude:220f7528-c2e6");
        assert_eq!(db.resolve_session_id("nope").unwrap(), "nope");
        assert!(matches!(db.resolve_session_id("claude:"), Err(DbError::AmbiguousSession(..))));
    }

    #[test]
    fn turns_are_ordered_by_seq() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("sess-1", Tool::Claude)).unwrap();

        let turns = vec![
            make_turn("sess-1", 2, Role::Assistant, "second"),
            make_turn("sess-1", 0, Role::User, "first"),
            make_turn("sess-1", 1, Role::ToolUse, "middle"),
        ];
        db.upsert_turns(&turns).unwrap();

        let retrieved = db.turns("sess-1").unwrap();
        assert_eq!(retrieved[0].content, "first");
        assert_eq!(retrieved[1].content, "middle");
        assert_eq!(retrieved[2].content, "second");
    }

    #[test]
    fn fts_search_finds_content() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("sess-1", Tool::Claude)).unwrap();
        db.upsert_turns(&[make_turn("sess-1", 0, Role::User, "the authentication module is broken")]).unwrap();
        db.upsert_turns(&[make_turn("sess-1", 1, Role::Assistant, "let me check the database connection")]).unwrap();

        let hits = db.search("authentication", &Filter::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("authentication") || hits[0].snippet.contains("[authentication]"));
    }

    #[test]
    fn fts_search_empty_query_returns_nothing_panicky() {
        let db = Db::open_in_memory().unwrap();
        let result = db.search("nonexistent_xyz_term_12345", &Filter::default()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn fts_search_filters_by_tool() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("s-claude", Tool::Claude)).unwrap();
        db.upsert_session(&make_session("s-codex", Tool::Codex)).unwrap();
        db.upsert_turns(&[make_turn("s-claude", 0, Role::User, "refactor the login function")]).unwrap();
        db.upsert_turns(&[make_turn("s-codex", 0, Role::User, "refactor the login function")]).unwrap();

        let filter = Filter { tool: Some(Tool::Codex), ..Default::default() };
        let hits = db.search("login", &filter).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].tool, Tool::Codex);
    }

    #[test]
    fn has_session_returns_false_for_unknown() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.has_session("unknown", 0).unwrap());
    }

    #[test]
    fn has_session_returns_true_after_insert() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("sess-1", Tool::Claude)).unwrap();
        assert!(db.has_session("sess-1", 0).unwrap());
    }

    #[test]
    fn stats_aggregates_by_tool() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("s1", Tool::Claude)).unwrap();
        db.upsert_session(&make_session("s2", Tool::Claude)).unwrap();
        db.upsert_session(&make_session("s3", Tool::Codex)).unwrap();

        let stats = db.stats(None).unwrap();
        assert_eq!(stats.len(), 2);
        let claude_stat = stats.iter().find(|(t, ..)| *t == Tool::Claude).unwrap();
        assert_eq!(claude_stat.1, 2);
        assert_eq!(claude_stat.2, 200);
    }

    #[test]
    fn cascade_delete_removes_turns() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("sess-1", Tool::Claude)).unwrap();
        db.upsert_turns(&[make_turn("sess-1", 0, Role::User, "test")]).unwrap();

        db.conn.execute("DELETE FROM session WHERE id = 'sess-1'", []).unwrap();

        let turns = db.turns("sess-1").unwrap();
        assert!(turns.is_empty());
    }

    #[test]
    fn short_turn_produces_single_chunk() {
        let content = "short content";
        let chunks = chunk_content(content);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], content);
    }

    #[test]
    fn long_turn_produces_multiple_overlapping_chunks() {
        let content = long_content(300);
        let chunks = chunk_content(&content);
        assert!(chunks.len() > 1);
        let step = CHUNK_SIZE - CHUNK_OVERLAP;
        let chars: Vec<char> = content.chars().collect();
        let expected_first: String = chars[..CHUNK_SIZE].iter().collect();
        let expected_second: String = chars[step..step + CHUNK_SIZE].iter().collect();
        assert_eq!(chunks[0], expected_first);
        assert_eq!(chunks[1], expected_second);
    }

    #[test]
    fn chunked_fts_finds_match_in_long_turn() {
        let mut db = Db::open_in_memory().unwrap();
        db.upsert_session(&make_session("sess-1", Tool::Claude)).unwrap();
        let prefix = "a ".repeat(400);
        let content = format!("{prefix}crystallize endpoint {}", "b ".repeat(200));
        db.upsert_turns(&[make_turn("sess-1", 0, Role::Assistant, &content)]).unwrap();

        let hits = db.search("crystallize", &Filter::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("crystallize") || hits[0].snippet.contains("[crystallize]"));
    }

    #[test]
    fn chunk_content_boundary_turn_is_single_chunk() {
        let content = "x".repeat(CHUNK_THRESHOLD);
        let chunks = chunk_content(&content);
        assert_eq!(chunks.len(), 1);
    }
}
