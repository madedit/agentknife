//! The project tools the agent works with (P1-AGENT-002).

mod ask;
mod command;
mod files;
mod knowledge;
pub mod path;
pub mod preview;
mod search;

use std::path::Path;
use std::sync::Arc;

use ak_types::stack::KnowledgePack;

pub use ask::{AskUser, Asker, Question};
pub use command::{RunCommand, ToolEnv};
pub use files::{EditFile, ListFiles, ReadFile, WriteFile};
pub use knowledge::{READ_KNOWLEDGE, ReadKnowledge};
pub use preview::{PREVIEW_TOOLS, PreviewAccess, with_preview_tools};
pub use search::SearchFiles;

use crate::tool::{ToolContext, ToolOutput, ToolRegistry};

/// Upper bound for one tool result, so a big file or log cannot flood the context.
pub const MAX_OUTPUT_CHARS: usize = 30_000;

/// Directories never listed or searched.
pub(crate) const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "dist"];

/// Every project tool, ready for [`crate::Agent`]. `knowledge` is the project
/// stack's docs (`StackProfile::knowledge_pack`), served by `read_knowledge`.
pub fn project_tools(
    env: ToolEnv,
    asker: Arc<dyn Asker>,
    knowledge: Option<KnowledgePack>,
) -> ToolRegistry {
    let tools = ToolRegistry::new()
        .with(Arc::new(ReadFile))
        .with(Arc::new(WriteFile))
        .with(Arc::new(EditFile))
        .with(Arc::new(ListFiles))
        .with(Arc::new(SearchFiles))
        .with(Arc::new(RunCommand::new(env)))
        .with(Arc::new(AskUser::new(asker)));
    match knowledge {
        Some(pack) => tools.with(Arc::new(ReadKnowledge::new(pack))),
        None => tools,
    }
}

pub(crate) fn project_root(ctx: &ToolContext) -> Result<&Path, ToolOutput> {
    ctx.project_dir
        .as_deref()
        .ok_or_else(|| ToolOutput::error("No project is open."))
}

/// Keeps the start and the end (errors usually come last) of a long text.
pub(crate) fn cap(text: &str, max: usize) -> String {
    let chars = text.chars().count();
    if chars <= max {
        return text.to_owned();
    }
    let head: String = text.chars().take(max / 4).collect();
    let tail: String = text.chars().skip(chars - (max - max / 4)).collect();
    format!("{head}\n… ({} characters omitted) …\n{tail}", chars - max)
}

pub(crate) fn str_arg<'a>(args: &'a serde_json::Value, name: &str) -> Result<&'a str, ToolOutput> {
    args[name]
        .as_str()
        .ok_or_else(|| ToolOutput::error(format!("`{name}` is required and must be a string.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_keeps_head_and_tail() {
        let text: String = (0..1000).map(|i| format!("{i}\n")).collect();
        let c = cap(&text, 100);
        assert!(c.starts_with("0\n1\n"));
        assert!(c.ends_with("999\n"));
        assert!(c.contains("characters omitted"));
        assert_eq!(cap("short", 100), "short");
    }
}
