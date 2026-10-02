use anyhow::Result;
use serde::Serialize;

use crate::db::Db;
use crate::domain::{Filter, SearchHit};

#[derive(Serialize)]
pub struct SearchRow {
    pub session_id: String,
    pub tool: String,
    pub seq: i32,
    pub role: String,
    pub tool_name: Option<String>,
    pub snippet: String,
    pub rank: f64,
    pub started_at: i64,
    pub project: Option<String>,
}

impl From<SearchHit> for SearchRow {
    fn from(h: SearchHit) -> Self {
        SearchRow {
            session_id: h.session_id,
            tool: h.tool.to_string(),
            seq: h.seq,
            role: h.role.as_str().to_owned(),
            tool_name: h.tool_name,
            snippet: h.snippet,
            rank: h.rank,
            started_at: h.started_at,
            project: h.project,
        }
    }
}

pub fn run(db: &Db, query: &str, filter: &Filter, json: bool) -> Result<()> {
    let hits: Vec<SearchRow> = db.search(query, filter)?.into_iter().map(SearchRow::from).collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    if hits.is_empty() {
        println!("No results for \"{query}\".");
        return Ok(());
    }

    hits.iter().for_each(|h| {
        let id_short = if h.session_id.len() > 16 { &h.session_id[..16] } else { &h.session_id };
        println!("[{}] {} turn#{} ({})", h.tool, id_short, h.seq, h.role);
        println!("  {}", h.snippet);
        println!();
    });

    Ok(())
}
