//! What the model sees: project notes at session start, size estimates, and the
//! compaction instruction.

use std::path::Path;

use ak_types::chat::{ContentPart, Conversation, Message, ToolSpec};

/// Marks the summarization request (the scripted test provider answers it).
pub const COMPACT_INSTRUCTION: &str = "[AgentKnife: compaction] Summarize the conversation \
so far so that the work can continue from the summary alone. Include: what the user \
wants to build, decisions made, what is finished, the current state of the important \
files, and open problems. Be concise and concrete. Write it in the user's language. \
Do not call tools.";

/// Notes files read at session start, relative to the project root.
const NOTE_FILES: &[&str] = &[
    ".agentknife/PROJECT.md",
    ".agentknife/plan/STATUS.md",
    ".agentknife/plan/tasks.md",
];
const MAX_NOTE_CHARS: usize = 20_000;

/// The project's notes, formatted for a context entry; `None` without any notes.
pub fn project_notes(project_dir: &Path) -> Option<String> {
    let mut out = String::from(
        "Project notes at the start of this session (keep them up to date as you work):\n",
    );
    let mut any = false;
    for rel in NOTE_FILES {
        if let Ok(text) = std::fs::read_to_string(project_dir.join(rel)) {
            any = true;
            let text: String = text.chars().take(MAX_NOTE_CHARS).collect();
            out.push_str(&format!("\n--- {rel} ---\n{}\n", text.trim_end()));
        }
    }
    any.then_some(out)
}

/// Rough token count of what a request sends. Deliberately pessimistic: CJK text
/// is close to one token per character, Latin text about four characters per token.
pub fn estimate_tokens(messages: &[Message]) -> u64 {
    fn text(s: &str) -> u64 {
        s.chars()
            .map(|c| if c.is_ascii() { 1 } else { 4 })
            .sum::<u64>()
            .div_ceil(4)
    }
    fn parts(p: &[ContentPart]) -> u64 {
        p.iter()
            .map(|p| match p {
                ContentPart::Text { text: t } => text(t),
                // A screenshot costs on the order of a thousand tokens or more.
                ContentPart::Image { .. } => 1_600,
            })
            .sum()
    }
    messages
        .iter()
        .map(|m| {
            4 + match m {
                Message::User { content } => parts(content),
                Message::ToolResult { content, .. } => parts(content),
                Message::Assistant {
                    text: t,
                    tool_calls,
                    thinking,
                    ..
                } => {
                    text(t)
                        + tool_calls
                            .iter()
                            .map(|c| text(&c.arguments.to_string()) + text(&c.name))
                            .sum::<u64>()
                        + thinking.iter().map(|th| text(&th.text)).sum::<u64>()
                }
            }
        })
        .sum()
}

/// Rough token count of a whole request: system prompt and tool definitions count
/// too (tool schemas alone can be thousands of tokens).
pub fn estimate_request(system: &str, tools: &[ToolSpec], messages: &[Message]) -> u64 {
    let tools_json: String = tools
        .iter()
        .map(|t| format!("{}{}{}", t.name, t.description, t.input_schema))
        .collect();
    estimate_tokens(&[Message::user_text(format!("{system}{tools_json}"))])
        + estimate_tokens(messages)
}

/// Where to cut for a compaction: the view position of the start of the previous
/// user turn, so the current and the previous turn stay verbatim. `None` when there
/// is nothing older to summarize.
pub fn compaction_cut(conversation: &Conversation) -> Option<usize> {
    let view = conversation.view();
    let starts: Vec<usize> = view
        .iter()
        .enumerate()
        .filter(|(_, (idx, m))| idx.is_some() && matches!(m, Message::User { .. }))
        .map(|(pos, _)| pos)
        .collect();
    let cut = match starts.len() {
        0 => return None,
        1 => starts[0],
        n => starts[n - 2],
    };
    (cut > 0).then_some(cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_cjk_heavier_than_latin() {
        let en = estimate_tokens(&[Message::user_text("a".repeat(400))]);
        let zh = estimate_tokens(&[Message::user_text("字".repeat(400))]);
        assert!((100..=110).contains(&en), "{en}");
        assert!((400..=410).contains(&zh), "{zh}");
    }

    #[test]
    fn notes_read_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(project_notes(dir.path()).is_none());
        std::fs::create_dir_all(dir.path().join(".agentknife/plan")).unwrap();
        std::fs::write(
            dir.path().join(".agentknife/PROJECT.md"),
            "# Todo\nA list app.",
        )
        .unwrap();
        let notes = project_notes(dir.path()).unwrap();
        assert!(notes.contains("--- .agentknife/PROJECT.md ---\n# Todo\nA list app."));
        assert!(!notes.contains("STATUS.md"));
    }

    #[test]
    fn cut_keeps_current_and_previous_turn() {
        let mut c = Conversation::new();
        assert_eq!(compaction_cut(&c), None);
        c.push_message(Message::user_text("t1"));
        assert_eq!(
            compaction_cut(&c),
            None,
            "a single turn cannot be compacted"
        );
        c.push_message(Message::user_text("t2"));
        assert_eq!(compaction_cut(&c), None, "two turns are both kept");
        c.push_message(Message::user_text("t3"));
        assert_eq!(compaction_cut(&c), Some(1));
    }
}
