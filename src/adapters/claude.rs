use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use thiserror::Error;
use walkdir::WalkDir;

use crate::domain::{Role, Session, Tool, Turn};
use super::IngestedSession;

#[derive(Debug, Error)]
pub enum ClaudeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no usable events in {0}")]
    NoEvents(String),
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "sessionId", default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    message: Option<Value>,
}

fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn text_from_blocks(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|b| {
            let kind = b.get("type")?.as_str()?;
            match kind {
                "text" => b.get("text")?.as_str().map(str::to_owned),
                "tool_result" => {
                    let c = b.get("content")?;
                    if let Some(s) = c.as_str() {
                        Some(s.to_owned())
                    } else if let Some(arr) = c.as_array() {
                        Some(text_from_blocks(arr))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn ingest_file(path: &Path) -> Result<IngestedSession, ClaudeError> {
    let mtime_ms = path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let content = std::fs::read_to_string(path)?;
    let events: Vec<RawEvent> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let mut session_id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut model: Option<String> = None;
    let mut started_at: Option<i64> = None;
    let mut ended_at: Option<i64> = None;
    let mut tokens_input: i64 = 0;
    let mut tokens_output: i64 = 0;
    let mut turns: Vec<Turn> = Vec::new();
    let mut seq: i32 = 0;

    for event in &events {
        if let Some(ref sid) = event.session_id {
            if session_id.is_none() {
                session_id = Some(sid.clone());
            }
        }
        if let Some(ref c) = event.cwd {
            if cwd.is_none() {
                cwd = Some(c.clone());
            }
        }

        let ts = event.timestamp.as_deref().and_then(parse_ts);
        if let Some(t) = ts {
            if started_at.is_none() { started_at = Some(t); }
            ended_at = Some(t);
        }

        let msg = match &event.message {
            Some(m) => m,
            None => continue,
        };

        match event.kind.as_str() {
            "user" => {
                let content_val = msg.get("content");
                match content_val {
                    Some(Value::String(s)) if !s.trim().is_empty() => {
                        turns.push(Turn {
                            id: 0, session_id: String::new(), seq,
                            role: Role::User, tool_name: None, tool_input: None,
                            content: s.clone(), ts, tokens: 0,
                        });
                        seq += 1;
                    }
                    Some(Value::Array(blocks)) => {
                        for block in blocks {
                            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                                continue;
                            }
                            let c = block.get("content");
                            let text = match c {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Array(arr)) => text_from_blocks(arr),
                                _ => continue,
                            };
                            if text.trim().is_empty() { continue; }
                            turns.push(Turn {
                                id: 0, session_id: String::new(), seq,
                                role: Role::ToolResult, tool_name: None, tool_input: None,
                                content: text, ts, tokens: 0,
                            });
                            seq += 1;
                        }
                    }
                    _ => {}
                }
            }
            "assistant" => {
                if let Some(m_model) = msg.get("model").and_then(Value::as_str) {
                    if model.is_none() { model = Some(m_model.to_owned()); }
                }
                if let Some(usage) = msg.get("usage") {
                    tokens_input += usage.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
                    tokens_output += usage.get("output_tokens").and_then(Value::as_i64).unwrap_or(0);
                }
                let blocks = match msg.get("content").and_then(Value::as_array) {
                    Some(b) => b,
                    None => continue,
                };
                let text_parts: String = blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text")?.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text_parts.trim().is_empty() {
                    turns.push(Turn {
                        id: 0, session_id: String::new(), seq,
                        role: Role::Assistant, tool_name: None, tool_input: None,
                        content: text_parts, ts, tokens: tokens_output,
                    });
                    seq += 1;
                }
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) != Some("tool_use") { continue; }
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("unknown").to_owned();
                    let input = block.get("input")
                        .map(|v| serde_json::to_string(v).unwrap_or_default())
                        .unwrap_or_default();
                    turns.push(Turn {
                        id: 0, session_id: String::new(), seq,
                        role: Role::ToolUse, tool_name: Some(name.clone()),
                        tool_input: Some(input.clone()), content: input, ts, tokens: 0,
                    });
                    seq += 1;
                }
            }
            _ => {}
        }
    }

    let raw_id = session_id.ok_or_else(|| ClaudeError::NoEvents(path.display().to_string()))?;
    let started = started_at.ok_or_else(|| ClaudeError::NoEvents(path.display().to_string()))?;

    let id = format!("claude:{raw_id}");
    let session = Session {
        id: id.clone(),
        tool: Tool::Claude,
        title: None,
        project: cwd,
        model,
        started_at: started,
        ended_at,
        tokens_input,
        tokens_output,
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
        p.push("tests/fixtures/claude");
        p.push(name);
        p
    }

    #[test]
    fn parses_basic_fixture() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let s = &result.session;
        assert_eq!(s.tool, Tool::Claude);
        assert!(s.id.starts_with("claude:"));
        assert_eq!(s.project, Some("/Users/test/myproject".to_string()));
        assert_eq!(s.model, Some("claude-opus-5".to_string()));
        assert_eq!(s.tokens_input, 42 + 80);
        assert_eq!(s.tokens_output, 28 + 12);
        assert!(s.started_at > 0);
        assert!(s.ended_at.is_some());
    }

    #[test]
    fn basic_fixture_turn_count_and_roles() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        assert!(!result.turns.is_empty());

        let user_turns: Vec<_> = result.turns.iter().filter(|t| t.role == Role::User).collect();
        let assistant_turns: Vec<_> = result.turns.iter().filter(|t| t.role == Role::Assistant).collect();
        let tool_use_turns: Vec<_> = result.turns.iter().filter(|t| t.role == Role::ToolUse).collect();
        let tool_result_turns: Vec<_> = result.turns.iter().filter(|t| t.role == Role::ToolResult).collect();

        assert_eq!(user_turns.len(), 1, "one user message");
        assert_eq!(tool_use_turns.len(), 1, "one tool call");
        assert_eq!(tool_result_turns.len(), 1, "one tool result");
        assert!(!assistant_turns.is_empty(), "at least one assistant turn");
    }

    #[test]
    fn basic_fixture_user_content() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let user = result.turns.iter().find(|t| t.role == Role::User).unwrap();
        assert_eq!(user.content, "list the files in this directory");
    }

    #[test]
    fn basic_fixture_tool_use_name() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let tu = result.turns.iter().find(|t| t.role == Role::ToolUse).unwrap();
        assert_eq!(tu.tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn basic_fixture_tool_result_content() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let tr = result.turns.iter().find(|t| t.role == Role::ToolResult).unwrap();
        assert!(tr.content.contains("README.md"));
    }

    #[test]
    fn basic_fixture_turns_have_session_id() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        for turn in &result.turns {
            assert_eq!(turn.session_id, result.session.id);
        }
    }

    #[test]
    fn basic_fixture_turns_ordered_by_seq() {
        let result = ingest_file(&fixture("basic.jsonl")).unwrap();
        let seqs: Vec<i32> = result.turns.iter().map(|t| t.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort();
        assert_eq!(seqs, sorted);
    }

    #[test]
    fn empty_fixture_returns_error() {
        let result = ingest_file(&fixture("empty.jsonl"));
        assert!(result.is_err());
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("test.jsonl");
        std::fs::write(&f, "not json\n{\"type\":\"mode\",\"mode\":\"normal\",\"sessionId\":\"x\"}\n\n").unwrap();
        let result = ingest_file(&f);
        assert!(result.is_err());
    }

    #[test]
    fn scan_finds_files_in_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("project-hash");
        std::fs::create_dir(&sub).unwrap();
        let fixture_content = std::fs::read_to_string(fixture("basic.jsonl")).unwrap();
        std::fs::write(sub.join("sess.jsonl"), fixture_content).unwrap();

        let results = scan(dir.path(), None);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn scan_skips_non_jsonl_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        let results = scan(dir.path(), None);
        assert!(results.is_empty());
    }
}
