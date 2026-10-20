//! Agent loop, tool definitions, permission policy and context compaction.

#![forbid(unsafe_code)]

mod agent;
pub mod context;
pub mod lock;
pub mod permission;
pub mod plan;
pub mod policy;
pub mod prompt;
pub mod scripted;
pub mod tool;
pub mod tools;

pub use agent::{
    Agent, AgentConfig, Checkpointer, ConversationStore, NoCheckpoints, NoStore, TurnSummary,
};

pub(crate) use ak_types::ids::new_id;
