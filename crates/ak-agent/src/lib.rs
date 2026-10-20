//! Agent loop, tool definitions, permission policy and context compaction.

#![forbid(unsafe_code)]

mod agent;
pub mod lock;
pub mod permission;
pub mod scripted;
pub mod tool;
pub mod tools;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub use agent::{Agent, AgentConfig, Checkpointer, NoCheckpoints, TurnSummary};

/// Unique enough for ids within one installation: time + process-wide counter.
pub(crate) fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!(
        "{prefix}-{nanos:x}-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
