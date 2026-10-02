use std::path::PathBuf;
use anyhow::Result;
use clap::{Parser, Subcommand};

use aihist::commands::{index, search, sessions, show, stats, tools};
use aihist::db::Db;
use aihist::domain::{Filter, Tool};

fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/aihist/aihist.db")
}

#[derive(Parser)]
#[command(name = "aihist", version, about = "Unified AI conversation history CLI")]
struct Cli {
    #[arg(long, global = true, help = "Output as JSON")]
    json: bool,

    #[arg(long, global = true, value_name = "PATH", help = "Database path")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Index sessions from all tools into the local database")]
    Index {
        #[arg(long, help = "Only index files modified since this unix timestamp (ms)")]
        since: Option<i64>,
        #[arg(long, help = "Verbose output")]
        verbose: bool,
    },

    #[command(about = "List sessions")]
    Sessions {
        #[arg(long, value_parser = parse_tool, help = "Filter by tool: claude|codex|opencode")]
        tool: Option<Tool>,
        #[arg(long, help = "Only sessions started after this unix timestamp (ms)")]
        since: Option<i64>,
        #[arg(long, default_value = "50")]
        limit: usize,
    },

    #[command(about = "Full-text search across all sessions")]
    Search {
        query: String,
        #[arg(long, value_parser = parse_tool, help = "Filter by tool")]
        tool: Option<Tool>,
        #[arg(long, help = "Only sessions started after this unix timestamp (ms)")]
        since: Option<i64>,
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    #[command(about = "Show all turns in a session")]
    Show {
        session_id: String,
    },

    #[command(about = "Show tool calls in a session")]
    Tools {
        session_id: String,
        #[arg(long, help = "Only show MCP tool calls")]
        mcp: bool,
    },

    #[command(about = "Show MCP calls in a session (alias for tools --mcp)")]
    Mcp {
        session_id: String,
    },

    #[command(about = "Show token usage statistics")]
    Stats {
        #[arg(help = "Narrow to a specific session ID")]
        session_id: Option<String>,
    },
}

fn parse_tool(s: &str) -> Result<Tool, String> {
    Tool::from_str(s).ok_or_else(|| format!("unknown tool '{s}': expected claude, codex, or opencode"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.unwrap_or_else(default_db_path);

    match cli.command {
        Command::Index { since, verbose } => {
            index::ensure_db_dir(&db_path)?;
            let mut opts = index::IndexOptions::with_defaults(db_path);
            opts.since_mtime_ms = since;
            opts.verbose = verbose;
            let result = index::run(&opts)?;
            if cli.json {
                println!("{{\"ingested\":{},\"skipped\":{},\"errors\":{}}}", result.ingested, result.skipped, result.errors);
            } else {
                println!("Indexed: {}  skipped: {}  errors: {}", result.ingested, result.skipped, result.errors);
            }
        }

        Command::Sessions { tool, since, limit } => {
            let db = Db::open(db_path.to_str().unwrap())?;
            let filter = Filter { tool, since_ms: since, limit: Some(limit), ..Default::default() };
            sessions::run(&db, &filter, cli.json)?;
        }

        Command::Search { query, tool, since, limit } => {
            let db = Db::open(db_path.to_str().unwrap())?;
            let filter = Filter { tool, since_ms: since, limit: Some(limit), ..Default::default() };
            search::run(&db, &query, &filter, cli.json)?;
        }

        Command::Show { session_id } => {
            let db = Db::open(db_path.to_str().unwrap())?;
            show::run(&db, &session_id, cli.json)?;
        }

        Command::Tools { session_id, mcp } => {
            let db = Db::open(db_path.to_str().unwrap())?;
            tools::run(&db, &session_id, mcp, cli.json)?;
        }

        Command::Mcp { session_id } => {
            let db = Db::open(db_path.to_str().unwrap())?;
            tools::run(&db, &session_id, true, cli.json)?;
        }

        Command::Stats { session_id } => {
            let db = Db::open(db_path.to_str().unwrap())?;
            stats::run(&db, session_id.as_deref(), cli.json)?;
        }
    }

    Ok(())
}
