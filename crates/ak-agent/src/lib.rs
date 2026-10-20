//! Agent loop, tool definitions, permission policy and context compaction.

#![forbid(unsafe_code)]

mod agent;
pub mod lock;
pub mod permission;
pub mod policy;
pub mod scripted;
pub mod tool;
pub mod tools;

pub use agent::{Agent, AgentConfig, Checkpointer, NoCheckpoints, TurnSummary};

pub(crate) use ak_types::ids::new_id;
