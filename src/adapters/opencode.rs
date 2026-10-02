use rusqlite::{Connection, params};
use serde_json::Value;
use std::path::Path;
use thiserror::Error;

use crate::domain::{Role, Session, Tool, Turn};
use super::IngestedSession;

#[derive(Debug, Error)]
pub enum OpenCodeError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("no sessions found")]
    Empty,
}

pub fn ingest_db(path: &Path) -> Result<Vec<IngestedSession>, OpenCodeError> {
    let conn = Connection::open(path)?;

    let mut stmt = conn.prepare(
        "SELECT id, title, directory, model, tokens_input, tokens_output, cost, time_created, time_updated
         FROM session WHERE time_archived IS NULL ORDER BY time_created",
    )?;

    let sessions: Vec<_> = stmt.query_map(params![], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, Option<f64>>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, Option<i64>>(8)?,
        ))
    })?.filter_map(|r| r.ok()).collect();

    let mut result = Vec::with_capacity(sessions.len());

    for (raw_id, title, directory, model_json, tokens_in, tokens_out, cost, time_created, time_updated) in sessions {
        let model = model_json.as_deref().and_then(|m| {
            serde_json::from_str::<Value>(m).ok()
                .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
        });

        let id = format!("opencode:{raw_id}");
        let session = Session {
            id: id.clone(),
            tool: Tool::OpenCode,
            title,
            project: directory,
            model,
            started_at: time_created,
            ended_at: time_updated,
            tokens_input: tokens_in,
            tokens_output: tokens_out,
            cost_usd: cost,
        };

        let turns = extract_turns(&conn, &raw_id, &id)?;
        result.push(IngestedSession { session, turns, source_mtime_ms: time_updated.unwrap_or(time_created) });
    }

    Ok(result)
}

fn extract_turns(conn: &Connection, raw_session_id: &str, session_id: &str) -> Result<Vec<Turn>, OpenCodeError> {
    let mut stmt = conn.prepare(
        "SELECT data, time_created FROM message WHERE session_id = ?1 ORDER BY time_created",
    )?;

    let rows: Vec<(String, i64)> = stmt
        .query_map(params![raw_session_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut turns = Vec::new();
    let mut seq: i32 = 0;

    for (data_str, ts) in rows {
        let data: Value = match serde_json::from_str(&data_str) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let role_str = data.get("role").and_then(Value::as_str).unwrap_or("");
        let role = match role_str {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => continue,
        };

        let parts = data.get("parts").and_then(Value::as_array);
        if let Some(parts) = parts {
            for part in parts {
                let ptype = part.get("type").and_then(Value::as_str).unwrap_or("");
                match ptype {
                    "text" => {
                        let text = part.get("text").and_then(Value::as_str).unwrap_or("");
                        if text.trim().is_empty() { continue; }
                        turns.push(Turn {
                            id: 0, session_id: session_id.to_owned(), seq,
                            role: role.clone(), tool_name: None, tool_input: None,
                            content: text.to_owned(), ts: Some(ts), tokens: 0,
                        });
                        seq += 1;
                    }
                    "tool-invocation" => {
                        let inv = match part.get("toolInvocation") { Some(v) => v, None => continue };
                        let state = inv.get("state").and_then(Value::as_str).unwrap_or("");
                        let name = inv.get("toolName").and_then(Value::as_str).unwrap_or("unknown").to_owned();
                        let input = inv.get("args")
                            .map(|v| serde_json::to_string(v).unwrap_or_default())
                            .unwrap_or_default();
                        match state {
                            "call" | "partial-call" => {
                                turns.push(Turn {
                                    id: 0, session_id: session_id.to_owned(), seq,
                                    role: Role::ToolUse, tool_name: Some(name), tool_input: Some(input.clone()),
                                    content: input, ts: Some(ts), tokens: 0,
                                });
                                seq += 1;
                            }
                            "result" => {
                                let result_val = inv.get("result").and_then(Value::as_str).unwrap_or("").to_owned();
                                if !result_val.trim().is_empty() {
                                    turns.push(Turn {
                                        id: 0, session_id: session_id.to_owned(), seq,
                                        role: Role::ToolResult, tool_name: Some(name), tool_input: None,
                                        content: result_val, ts: Some(ts), tokens: 0,
                                    });
                                    seq += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        } else {
            let text = data.get("content").and_then(Value::as_str).unwrap_or("").to_owned();
            if !text.trim().is_empty() {
                turns.push(Turn {
                    id: 0, session_id: session_id.to_owned(), seq,
                    role, tool_name: None, tool_input: None,
                    content: text, ts: Some(ts), tokens: 0,
                });
                seq += 1;
            }
        }
    }

    Ok(turns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn make_test_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("
            CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT, directory TEXT, model TEXT,
                tokens_input INTEGER NOT NULL DEFAULT 0, tokens_output INTEGER NOT NULL DEFAULT 0,
                cost REAL, time_created INTEGER NOT NULL, time_updated INTEGER,
                time_archived INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                data TEXT NOT NULL, time_created INTEGER NOT NULL
            );
        ").unwrap();
        (dir, path)
    }

    fn insert_session(conn: &Connection, id: &str, title: &str, dir: &str) {
        conn.execute(
            "INSERT INTO session (id, title, directory, model, tokens_input, tokens_output, time_created, time_updated)
             VALUES (?1, ?2, ?3, '{\"id\":\"gpt-5\"}', 100, 50, 1000000, 1001000)",
            params![id, title, dir],
        ).unwrap();
    }

    fn insert_message(conn: &Connection, session_id: &str, role: &str, content: &str) {
        let data = serde_json::json!({ "role": role, "content": content }).to_string();
        conn.execute(
            "INSERT INTO message (id, session_id, data, time_created) VALUES (?, ?, ?, 1000500)",
            params![format!("msg-{session_id}-{role}"), session_id, data],
        ).unwrap();
    }

    #[test]
    fn parses_empty_db() {
        let (_dir, path) = make_test_db();
        let result = ingest_db(&path).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parses_single_session() {
        let (_dir, path) = make_test_db();
        let conn = Connection::open(&path).unwrap();
        insert_session(&conn, "sess-1", "My Session", "/home/user/project");
        insert_message(&conn, "sess-1", "user", "help me fix this bug");
        insert_message(&conn, "sess-1", "assistant", "I'll look at the code");
        drop(conn);

        let result = ingest_db(&path).unwrap();
        assert_eq!(result.len(), 1);
        let s = &result[0].session;
        assert_eq!(s.tool, Tool::OpenCode);
        assert!(s.id.starts_with("opencode:"));
        assert_eq!(s.title, Some("My Session".to_string()));
        assert_eq!(s.project, Some("/home/user/project".to_string()));
        assert_eq!(s.model, Some("gpt-5".to_string()));
        assert_eq!(s.tokens_input, 100);
        assert_eq!(s.tokens_output, 50);
    }

    #[test]
    fn parses_turns_from_session() {
        let (_dir, path) = make_test_db();
        let conn = Connection::open(&path).unwrap();
        insert_session(&conn, "sess-1", "Test", "/proj");
        insert_message(&conn, "sess-1", "user", "fix the auth bug");
        insert_message(&conn, "sess-1", "assistant", "I'll check the code");
        drop(conn);

        let result = ingest_db(&path).unwrap();
        let turns = &result[0].turns;
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, Role::User);
        assert_eq!(turns[0].content, "fix the auth bug");
        assert_eq!(turns[1].role, Role::Assistant);
    }

    #[test]
    fn skips_system_role_messages() {
        let (_dir, path) = make_test_db();
        let conn = Connection::open(&path).unwrap();
        insert_session(&conn, "sess-1", "Test", "/proj");
        insert_message(&conn, "sess-1", "system", "you are a helpful assistant");
        insert_message(&conn, "sess-1", "user", "hello");
        drop(conn);

        let result = ingest_db(&path).unwrap();
        let turns = &result[0].turns;
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, Role::User);
    }

    #[test]
    fn archived_sessions_excluded() {
        let (_dir, path) = make_test_db();
        let conn = Connection::open(&path).unwrap();
        insert_session(&conn, "sess-1", "Active", "/proj");
        conn.execute(
            "INSERT INTO session (id, title, directory, tokens_input, tokens_output, time_created, time_archived)
             VALUES ('sess-2', 'Archived', '/proj', 0, 0, 999, 1234)",
            [],
        ).unwrap();
        drop(conn);

        let result = ingest_db(&path).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].session.id.contains("sess-1"));
    }

    #[test]
    fn all_turns_have_correct_session_id() {
        let (_dir, path) = make_test_db();
        let conn = Connection::open(&path).unwrap();
        insert_session(&conn, "sess-1", "Test", "/proj");
        insert_message(&conn, "sess-1", "user", "question");
        insert_message(&conn, "sess-1", "assistant", "answer");
        drop(conn);

        let result = ingest_db(&path).unwrap();
        for turn in &result[0].turns {
            assert_eq!(turn.session_id, result[0].session.id);
        }
    }

    #[test]
    fn multiple_sessions_ingested() {
        let (_dir, path) = make_test_db();
        let conn = Connection::open(&path).unwrap();
        insert_session(&conn, "s1", "First", "/a");
        insert_session(&conn, "s2", "Second", "/b");
        drop(conn);

        let result = ingest_db(&path).unwrap();
        assert_eq!(result.len(), 2);
    }
}
