use anyhow::Result;
use serde::Serialize;

use crate::db::Db;
use crate::domain::{Filter, Session};

#[derive(Serialize)]
pub struct SessionRow {
    pub id: String,
    pub tool: String,
    pub title: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub cost_usd: Option<f64>,
}

impl From<Session> for SessionRow {
    fn from(s: Session) -> Self {
        SessionRow {
            id: s.id,
            tool: s.tool.to_string(),
            title: s.title,
            project: s.project,
            model: s.model,
            started_at: s.started_at,
            ended_at: s.ended_at,
            tokens_input: s.tokens_input,
            tokens_output: s.tokens_output,
            cost_usd: s.cost_usd,
        }
    }
}

pub fn run(db: &Db, filter: &Filter, json: bool) -> Result<()> {
    let sessions: Vec<SessionRow> = db.sessions(filter)?.into_iter().map(SessionRow::from).collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        return Ok(());
    }

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!("{:<12} {:<10} {:<8} {:<8} {}", "id", "tool", "in", "out", "project");
    println!("{}", "-".repeat(70));
    sessions.iter().for_each(|s| {
        let id_short = if s.id.len() > 12 { &s.id[..12] } else { &s.id };
        let project = s.project.as_deref().unwrap_or("-");
        let project_short = if project.len() > 40 { &project[project.len()-40..] } else { project };
        println!("{:<12} {:<10} {:<8} {:<8} {}", id_short, s.tool, s.tokens_input, s.tokens_output, project_short);
    });

    Ok(())
}
