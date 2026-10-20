//! Agent loop, tool definitions, permission policy and context compaction.

#![forbid(unsafe_code)]

mod agent;
pub mod lock;
pub mod permission;
pub mod scripted;
pub mod tool;

pub use agent::{Agent, AgentConfig, Checkpointer, NoCheckpoints, TurnSummary};
