pub mod claude;
pub mod codex;
pub mod opencode;

use crate::domain::{Session, Turn};

#[derive(Debug)]
pub struct IngestedSession {
    pub session: Session,
    pub turns: Vec<Turn>,
    pub source_mtime_ms: i64,
}
