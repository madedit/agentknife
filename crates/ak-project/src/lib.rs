//! Projects, .agentknife/ files, git checkpoints and chat history.

#![forbid(unsafe_code)]

mod error;
mod fsutil;
mod project;
mod registry;
mod timeline;

pub use error::ProjectError;
pub use project::{AGENTKNIFE_DIR, LoadedHistory, Project, ProjectMeta};
pub use registry::{ProjectEntry, RecentProject, Registry};
pub use timeline::{CheckpointKind, DEFAULT_KEEP_TURNS, Interruption, Timeline, TimelineEntry};
