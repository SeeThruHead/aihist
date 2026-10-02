use anyhow::Result;
use serde::Serialize;

use crate::db::Db;
use crate::domain::{Filter, Role};

#[derive(Serialize)]
pub struct ToolCallRow {
    pub session_id: String,
    pub seq: i32,
    pub tool_name: String,
    pub tool_input: Option<String>,
    pub ts: Option<i64>,
}

pub fn run(db: &Db, session_id: &str, mcp_only: bool, json: bool) -> Result<()> {
    let turns = db.turns(session_id)?;
    let rows: Vec<ToolCallRow> = turns
        .into_iter()
        .filter(|t| t.role == Role::ToolUse)
        .filter(|t| {
            if mcp_only {
                t.tool_name.as_deref().is_some_and(|n| n.starts_with("mcp__"))
            } else {
                true
            }
        })
        .map(|t| ToolCallRow {
            session_id: t.session_id,
            seq: t.seq,
            tool_name: t.tool_name.unwrap_or_default(),
            tool_input: t.tool_input,
            ts: t.ts,
        })
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        let label = if mcp_only { "MCP tool calls" } else { "tool calls" };
        println!("No {label} in session {session_id}");
        return Ok(());
    }

    for r in &rows {
        println!("#{} {} {}", r.seq, r.tool_name, r.tool_input.as_deref().unwrap_or("{}"));
    }

    Ok(())
}

pub fn run_search(db: &Db, filter: &Filter, mcp_only: bool, json: bool) -> Result<()> {
    let query = if mcp_only { "mcp__*" } else { "*" };
    let hits = db.search(query, filter)?;

    let rows: Vec<ToolCallRow> = hits
        .into_iter()
        .filter(|h| h.role == Role::ToolUse)
        .map(|h| ToolCallRow {
            session_id: h.session_id,
            seq: h.seq,
            tool_name: h.tool_name.unwrap_or_default(),
            tool_input: Some(h.snippet),
            ts: None,
        })
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    for r in &rows {
        println!("{} #{} {}", r.session_id, r.seq, r.tool_name);
    }

    Ok(())
}
