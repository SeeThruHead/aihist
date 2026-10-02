use anyhow::Result;
use serde::Serialize;

use crate::db::Db;

#[derive(Serialize)]
pub struct StatsRow {
    pub tool: String,
    pub sessions: i64,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub tokens_total: i64,
    pub cost_usd: f64,
}

pub fn run(db: &Db, session_id: Option<&str>, json: bool) -> Result<()> {
    let raw = db.stats(session_id)?;
    let rows: Vec<StatsRow> = raw
        .into_iter()
        .map(|(tool, sessions, tin, tout, cost)| StatsRow {
            tool: tool.to_string(),
            sessions,
            tokens_input: tin,
            tokens_output: tout,
            tokens_total: tin + tout,
            cost_usd: cost,
        })
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        println!("No data. Run `aihist index` first.");
        return Ok(());
    }

    println!("{:<12} {:>8} {:>12} {:>12} {:>12} {:>10}", "tool", "sessions", "tokens_in", "tokens_out", "total", "cost_usd");
    println!("{}", "-".repeat(72));
    let mut total_in = 0i64;
    let mut total_out = 0i64;
    let mut total_cost = 0f64;
    for r in &rows {
        println!("{:<12} {:>8} {:>12} {:>12} {:>12} {:>10.4}", r.tool, r.sessions, r.tokens_input, r.tokens_output, r.tokens_total, r.cost_usd);
        total_in += r.tokens_input;
        total_out += r.tokens_output;
        total_cost += r.cost_usd;
    }
    println!("{}", "-".repeat(72));
    println!("{:<12} {:>8} {:>12} {:>12} {:>12} {:>10.4}", "TOTAL", "", total_in, total_out, total_in + total_out, total_cost);

    Ok(())
}
