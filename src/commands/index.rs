use std::path::{Path, PathBuf};
use anyhow::Result;

use crate::adapters::{claude, codex, opencode, IngestedSession};
use crate::db::Db;

pub struct IndexOptions {
    pub db_path: PathBuf,
    pub claude_root: Option<PathBuf>,
    pub codex_root: Option<PathBuf>,
    pub opencode_db: Option<PathBuf>,
    pub since_mtime_ms: Option<i64>,
    pub verbose: bool,
}

impl IndexOptions {
    pub fn with_defaults(db_path: PathBuf) -> Self {
        let home = dirs_home();
        let claude_root = std::env::var("CLAUDE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".claude/projects"));
        let codex_root = std::env::var("CODEX_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".codex/sessions"));
        let opencode_db = std::env::var("OPENCODE_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".local/share/opencode/opencode.db"));
        IndexOptions {
            db_path,
            claude_root: Some(claude_root),
            codex_root: Some(codex_root),
            opencode_db: Some(opencode_db),
            since_mtime_ms: None,
            verbose: false,
        }
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[derive(Default)]
pub struct IndexResult {
    pub ingested: usize,
    pub skipped: usize,
    pub errors: usize,
}

fn collect_candidates(opts: &IndexOptions) -> (Vec<IngestedSession>, usize) {
    let since = opts.since_mtime_ms;

    let claude_sessions = opts.claude_root.as_deref()
        .filter(|p| p.exists())
        .map(|p| claude::scan(p, since))
        .unwrap_or_default();

    let codex_sessions = opts.codex_root.as_deref()
        .filter(|p| p.exists())
        .map(|p| codex::scan(p, since))
        .unwrap_or_default();

    let (opencode_sessions, opencode_errors) = opts.opencode_db.as_deref()
        .filter(|p| p.exists())
        .map(|p| match opencode::ingest_db(p) {
            Ok(sessions) => (sessions, 0usize),
            Err(_) => (vec![], 1usize),
        })
        .unwrap_or_default();

    let all = claude_sessions.into_iter()
        .chain(codex_sessions)
        .chain(opencode_sessions)
        .collect();

    (all, opencode_errors)
}

pub fn run(opts: &IndexOptions) -> Result<IndexResult> {
    let mut db = Db::open(opts.db_path.to_str().unwrap_or(":memory:"))?;
    let (candidates, adapter_errors) = collect_candidates(opts);

    let result = candidates.into_iter().try_fold(
        IndexResult { errors: adapter_errors, ..Default::default() },
        |mut acc, s| -> Result<IndexResult> {
            let already = db.has_session(&s.session.id, s.source_mtime_ms)?;
            if already {
                acc.skipped += 1;
            } else {
                match index_session(&mut db, s.session, s.turns) {
                    Ok(_) => acc.ingested += 1,
                    Err(_) => acc.errors += 1,
                }
            }
            Ok(acc)
        },
    )?;

    Ok(result)
}

fn index_session(db: &mut Db, session: crate::domain::Session, turns: Vec<crate::domain::Turn>) -> Result<()> {
    db.upsert_session(&session)?;
    db.upsert_turns(&turns)?;
    Ok(())
}

pub fn ensure_db_dir(path: &Path) -> Result<()> {
    path.parent().map(std::fs::create_dir_all).transpose()?;
    Ok(())
}
