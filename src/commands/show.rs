use anyhow::Result;
use serde::Serialize;

use crate::db::Db;
use crate::domain::Turn;

#[derive(Serialize)]
pub struct TurnRow {
    pub seq: i32,
    pub role: String,
    pub tool_name: Option<String>,
    pub content: String,
    pub ts: Option<i64>,
    pub tokens: i64,
}

impl From<Turn> for TurnRow {
    fn from(t: Turn) -> Self {
        TurnRow {
            seq: t.seq,
            role: t.role.as_str().to_owned(),
            tool_name: t.tool_name,
            content: t.content,
            ts: t.ts,
            tokens: t.tokens,
        }
    }
}

pub fn run(db: &Db, session_id: &str, json: bool) -> Result<()> {
    let turns: Vec<TurnRow> = db.turns(session_id)?.into_iter().map(TurnRow::from).collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&turns)?);
        return Ok(());
    }

    if turns.is_empty() {
        println!("Session not found or has no turns: {session_id}");
        return Ok(());
    }

    for t in &turns {
        let label = match t.role.as_str() {
            "user" => "USER",
            "assistant" => "ASST",
            "tool_use" => "TOOL",
            "tool_result" => " OUT",
            _ => "    ",
        };
        let tool_suffix = t.tool_name.as_deref().map(|n| format!(" [{n}]")).unwrap_or_default();
        println!("── #{} {}{} ──────────────────────", t.seq, label, tool_suffix);
        let preview = if t.content.len() > 500 { format!("{}…", &t.content[..500]) } else { t.content.clone() };
        println!("{preview}");
        println!();
    }

    Ok(())
}
