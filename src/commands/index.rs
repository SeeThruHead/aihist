use std::path::{Path, PathBuf};
use anyhow::Result;

use crate::adapters::{claude, codex, opencode};
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

pub struct IndexResult {
    pub ingested: usize,
    pub skipped: usize,
    pub errors: usize,
}

pub fn run(opts: &IndexOptions) -> Result<IndexResult> {
    let mut db = Db::open(opts.db_path.to_str().unwrap_or(":memory:"))?;
    let mut ingested = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    if let Some(root) = &opts.claude_root {
        if root.exists() {
            let sessions = claude::scan(root, opts.since_mtime_ms);
            for s in sessions {
                if db.has_session(&s.session.id, s.source_mtime_ms)? {
                    skipped += 1;
                    continue;
                }
                match index_session(&mut db, s.session, s.turns) {
                    Ok(_) => ingested += 1,
                    Err(_) => errors += 1,
                }
            }
        }
    }

    if let Some(root) = &opts.codex_root {
        if root.exists() {
            let sessions = codex::scan(root, opts.since_mtime_ms);
            for s in sessions {
                if db.has_session(&s.session.id, s.source_mtime_ms)? {
                    skipped += 1;
                    continue;
                }
                match index_session(&mut db, s.session, s.turns) {
                    Ok(_) => ingested += 1,
                    Err(_) => errors += 1,
                }
            }
        }
    }

    if let Some(oc_path) = &opts.opencode_db {
        if oc_path.exists() {
            match opencode::ingest_db(oc_path) {
                Ok(sessions) => {
                    for s in sessions {
                        if db.has_session(&s.session.id, s.source_mtime_ms)? {
                            skipped += 1;
                            continue;
                        }
                        match index_session(&mut db, s.session, s.turns) {
                            Ok(_) => ingested += 1,
                            Err(_) => errors += 1,
                        }
                    }
                }
                Err(_) => errors += 1,
            }
        }
    }

    Ok(IndexResult { ingested, skipped, errors })
}

fn index_session(db: &mut Db, session: crate::domain::Session, turns: Vec<crate::domain::Turn>) -> Result<()> {
    db.upsert_session(&session)?;
    db.upsert_turns(&turns)?;
    Ok(())
}

pub fn ensure_db_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}
