use serde_json::Value;
use std::path::Path;
use thiserror::Error;
use walkdir::WalkDir;

use crate::domain::{Role, Session, Tool, Turn};
use super::IngestedSession;

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no usable events in {0}")]
    NoEvents(String),
}

fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

pub fn ingest_file(path: &Path) -> Result<IngestedSession, CodexError> {
    let mtime_ms = path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let content = std::fs::read_to_string(path)?;
    let events: Vec<Value> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut started_at: Option<i64> = None;
    let mut ended_at: Option<i64> = None;
    let mut turns: Vec<Turn> = Vec::new();
    let mut seq: i32 = 0;

    for event in &events {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        let ts_str = event.get("timestamp").and_then(Value::as_str).unwrap_or("");
        let ts = parse_ts(ts_str);

        if let Some(t) = ts {
            if started_at.is_none() { started_at = Some(t); }
            ended_at = Some(t);
        }

        match kind {
            "session_meta" => {
                let payload = match event.get("payload") { Some(p) => p, None => continue };
                if session_id.is_none() {
                    session_id = payload.get("id").and_then(Value::as_str).map(str::to_owned);
                }
                if cwd.is_none() {
                    cwd = payload.get("cwd").and_then(Value::as_str).map(str::to_owned);
                }
                if model.is_none() {
                    if let (Some(provider), Some(name)) = (
                        payload.get("model_provider").and_then(Value::as_str),
                        payload.get("model_name").and_then(Value::as_str),
                    ) {
                        model = Some(format!("{provider}/{name}"));
                    }
                }
            }
            "response_item" => {
                let payload = match event.get("payload") { Some(p) => p, None => continue };
                let role_str = payload.get("role").and_then(Value::as_str).unwrap_or("");
                let role = match role_str {
                    "user" => Role::User,
                    "assistant" | "model" => Role::Assistant,
                    "developer" | "system" => continue,
                    _ => continue,
                };

                let blocks = match payload.get("content").and_then(Value::as_array) {
                    Some(b) => b,
                    None => continue,
                };

                let mut has_tool = false;
                for block in blocks {
                    let btype = block.get("type").and_then(Value::as_str).unwrap_or("");
                    match btype {
                        "input_text" | "output_text" => {
                            let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                            if text.trim().is_empty() { continue; }
                            turns.push(Turn {
                                id: 0, session_id: String::new(), seq,
                                role: role.clone(), tool_name: None, tool_input: None,
                                content: text.to_owned(), ts, tokens: 0,
                            });
                            seq += 1;
                        }
                        "function_call" => {
                            has_tool = true;
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("unknown").to_owned();
                            let input = block.get("arguments")
                                .map(|v| serde_json::to_string(v).unwrap_or_default())
                                .unwrap_or_default();
                            turns.push(Turn {
                                id: 0, session_id: String::new(), seq,
                                role: Role::ToolUse, tool_name: Some(name), tool_input: Some(input.clone()),
                                content: input, ts, tokens: 0,
                            });
                            seq += 1;
                        }
                        "function_call_output" => {
                            let output = block.get("output").and_then(Value::as_str).unwrap_or("").to_owned();
                            if !output.trim().is_empty() {
                                turns.push(Turn {
                                    id: 0, session_id: String::new(), seq,
                                    role: Role::ToolResult, tool_name: None, tool_input: None,
                                    content: output, ts, tokens: 0,
                                });
                                seq += 1;
                            }
                        }
                        _ => {}
                    }
                    let _ = has_tool;
                }
            }
            _ => {}
        }
    }

    let raw_id = session_id.ok_or_else(|| CodexError::NoEvents(path.display().to_string()))?;
    let started = started_at.ok_or_else(|| CodexError::NoEvents(path.display().to_string()))?;

    let id = format!("codex:{raw_id}");
    let session = Session {
        id: id.clone(),
        tool: Tool::Codex,
        title: None,
        project: cwd,
        model,
        started_at: started,
        ended_at,
        tokens_input: 0,
        tokens_output: 0,
        cost_usd: None,
    };
    let turns = turns.into_iter().map(|t| Turn { session_id: id.clone(), ..t }).collect();
    Ok(IngestedSession { session, turns, source_mtime_ms: mtime_ms })
}

pub fn scan(root: &Path, since_mtime_ms: Option<i64>) -> Vec<IngestedSession> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter(|e| {
            if let Some(since) = since_mtime_ms {
                let mtime = e.metadata().ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                mtime >= since
            } else {
                true
            }
        })
        .filter_map(|e| ingest_file(e.path()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/codex");
        p.push(name);
        p
    }

    #[test]
    fn parses_basic_fixture() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let s = &result.session;
        assert_eq!(s.tool, Tool::Codex);
        assert!(s.id.starts_with("codex:"));
        assert_eq!(s.project, Some("/Users/test/otherproject".to_string()));
        assert_eq!(s.model, Some("openai/gpt-5.6".to_string()));
        assert!(s.started_at > 0);
    }

    #[test]
    fn basic_fixture_extracts_user_message() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let user = result.turns.iter().find(|t| t.role == Role::User).unwrap();
        assert_eq!(user.content, "refactor the auth module");
    }

    #[test]
    fn basic_fixture_extracts_assistant_message() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let asst = result.turns.iter().find(|t| t.role == Role::Assistant).unwrap();
        assert!(asst.content.contains("reading the auth module"));
    }

    #[test]
    fn basic_fixture_extracts_tool_use() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let tu = result.turns.iter().find(|t| t.role == Role::ToolUse).unwrap();
        assert_eq!(tu.tool_name.as_deref(), Some("shell"));
    }

    #[test]
    fn basic_fixture_extracts_tool_result() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let tr = result.turns.iter().find(|t| t.role == Role::ToolResult).unwrap();
        assert!(tr.content.contains("login"));
    }

    #[test]
    fn basic_fixture_skips_developer_role() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        for turn in &result.turns {
            assert_ne!(turn.content, "system");
        }
    }

    #[test]
    fn all_turns_have_session_id() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        for turn in &result.turns {
            assert_eq!(turn.session_id, result.session.id);
        }
    }

    #[test]
    fn empty_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("empty.jsonl");
        std::fs::write(&f, "\n").unwrap();
        assert!(ingest_file(&f).is_err());
    }

    #[test]
    fn malformed_json_lines_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("bad.jsonl");
        let content = std::fs::read_to_string(fixture("basic.jsonl")).unwrap();
        std::fs::write(&f, format!("not-json\n{content}")).unwrap();
        let result = ingest_file(&f).unwrap();
        assert!(!result.turns.is_empty());
    }
}
