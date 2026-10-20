//! Provider-neutral conversation messages (AGENTS.md §4, "Chat history format").
//!
//! Provider adapters convert these to and from their wire formats. The neutral fields
//! are what the UI shows and what any provider can rebuild a request from. Exact
//! provider output that must be echoed back unchanged (signed thinking, block order)
//! is kept separately as an origin-tagged [`ProviderReplay`].
//!
//! Histories are **append-only**: never edit, reorder or remove an earlier message.
//! Providers bind reasoning signatures to the exact conversation prefix.

use serde::{Deserialize, Serialize};

/// One message in a conversation, in the order the model sees them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    User {
        content: Vec<ContentPart>,
    },
    Assistant {
        /// Reasoning, shown collapsed in the UI. Display-only: never re-sent from here.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        thinking: Vec<Thinking>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCall>,
        /// The provider's own content, replayed verbatim when the same provider is
        /// asked again; other providers rebuild from the neutral fields above.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay: Option<ProviderReplay>,
    },
    ToolResult {
        call_id: String,
        content: Vec<ContentPart>,
        #[serde(default)]
        is_error: bool,
    },
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Message::User {
            content: vec![ContentPart::Text { text: text.into() }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    /// Base64 image, e.g. a preview screenshot for vision models.
    Image {
        media_type: String,
        data_base64: String,
    },
}

/// Model reasoning as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thinking {
    /// May be empty when the provider withholds reasoning text.
    pub text: String,
    /// Provider id that produced it, e.g. `"anthropic"`.
    pub origin: String,
}

/// Opaque provider content for an assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderReplay {
    /// Provider id ([`Thinking::origin`] uses the same ids).
    pub origin: String,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned id, echoed back in [`Message::ToolResult::call_id`].
    pub id: String,
    pub name: String,
    /// Parsed arguments. `Null` when [`ToolCall::invalid_arguments`] is set.
    pub arguments: serde_json::Value,
    /// Raw argument text that was not valid JSON (e.g. cut off by the token limit).
    /// The agent must answer such a call with an error result, not run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_arguments: Option<String>,
}

/// Current on-disk format of [`Conversation`].
/// - 1: messages and checkpoints.
/// - 2: adds `context` and `compaction` entries (v1 files load unchanged).
pub const CONVERSATION_SCHEMA_VERSION: u32 = 2;

/// Prefix of the text a [`Entry::Context`] is sent to the model with.
pub const CONTEXT_PREFIX: &str = "[AgentKnife note — not written by the user]\n";
/// Prefix of the summary that replaces compacted history in the model's view.
pub const SUMMARY_PREFIX: &str = "[AgentKnife: summary of the earlier conversation]\n";

/// A project's chat history as persisted in `.agentknife/`. Append-only: the only
/// mutations are the `push_*` methods.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    schema_version: u32,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum Entry {
    Message(Message),
    /// A checkpoint created after this point (shown on the timeline; never sent
    /// to a model).
    Checkpoint {
        id: String,
        label: String,
    },
    /// Something AgentKnife tells the model (project notes at session start, resume
    /// instructions). Sent as user text with [`CONTEXT_PREFIX`]; the UI shows it as a
    /// note, not as the user's words.
    Context {
        text: String,
    },
    /// From here on the model sees `summary` instead of the entries before
    /// `keep_from`. The entries themselves stay (append-only): the user still sees
    /// the whole chat.
    Compaction {
        summary: String,
        keep_from: usize,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("conversation file uses schema {found}, newer than supported {supported}")]
    TooNew { found: u32, supported: u32 },
    #[error("invalid conversation file: {0}")]
    Invalid(String),
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            schema_version: CONVERSATION_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The messages to send to a model, in order: after the latest compaction, its
    /// summary followed by the entries it kept.
    pub fn messages(&self) -> Vec<Message> {
        self.view().into_iter().map(|(_, m)| m).collect()
    }

    /// [`Self::messages`] with the entry index each message came from (`None` for a
    /// compaction summary).
    pub fn view(&self) -> Vec<(Option<usize>, Message)> {
        let last = self
            .entries
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, e)| match e {
                Entry::Compaction { summary, keep_from } => Some((i, summary, *keep_from)),
                _ => None,
            });
        let (start, mut out) = match last {
            Some((_, summary, keep_from)) => (
                keep_from,
                vec![(
                    None,
                    Message::user_text(format!("{SUMMARY_PREFIX}{summary}")),
                )],
            ),
            None => (0, Vec::new()),
        };
        for (i, e) in self.entries.iter().enumerate().skip(start) {
            match e {
                Entry::Message(m) => out.push((Some(i), m.clone())),
                Entry::Context { text } => {
                    out.push((
                        Some(i),
                        Message::user_text(format!("{CONTEXT_PREFIX}{text}")),
                    ));
                }
                Entry::Checkpoint { .. } | Entry::Compaction { .. } => {}
            }
        }
        out
    }

    pub fn push_context(&mut self, text: impl Into<String>) {
        self.entries.push(Entry::Context { text: text.into() });
    }

    /// The latest context note, if any.
    pub fn last_context(&self) -> Option<&str> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Context { text } => Some(text.as_str()),
            _ => None,
        })
    }

    /// Records a compaction. `keep_from` must point at an entry that starts a user
    /// turn, so the kept part is a well-formed conversation.
    pub fn push_compaction(&mut self, summary: impl Into<String>, keep_from: usize) {
        self.entries.push(Entry::Compaction {
            summary: summary.into(),
            keep_from: keep_from.min(self.entries.len()),
        });
    }

    pub fn push_message(&mut self, message: Message) {
        self.entries.push(Entry::Message(message));
    }

    pub fn push_checkpoint(&mut self, id: impl Into<String>, label: impl Into<String>) {
        self.entries.push(Entry::Checkpoint {
            id: id.into(),
            label: label.into(),
        });
    }

    pub fn to_json(&self) -> Result<String, ConversationError> {
        serde_json::to_string_pretty(self).map_err(|e| ConversationError::Invalid(e.to_string()))
    }

    /// Loads a saved conversation, migrating older schema versions.
    pub fn from_json(text: &str) -> Result<Self, ConversationError> {
        let raw: serde_json::Value =
            serde_json::from_str(text).map_err(|e| ConversationError::Invalid(e.to_string()))?;
        let found = raw["schema_version"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ConversationError::Invalid("missing schema_version".into()))?;
        if found > CONVERSATION_SCHEMA_VERSION {
            return Err(ConversationError::TooNew {
                found,
                supported: CONVERSATION_SCHEMA_VERSION,
            });
        }
        // v1 -> v2: only new entry kinds were added, so the data is unchanged. Future
        // migrations transform `raw` here, oldest first.
        let mut c: Conversation =
            serde_json::from_value(raw).map_err(|e| ConversationError::Invalid(e.to_string()))?;
        c.schema_version = CONVERSATION_SCHEMA_VERSION;
        Ok(c)
    }
}

/// A tool the model may call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the arguments object.
    pub input_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_through_json() {
        let msgs = vec![
            Message::user_text("make the button blue"),
            Message::Assistant {
                thinking: vec![Thinking {
                    text: "find the button".into(),
                    origin: "anthropic".into(),
                }],
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({ "path": "src/app.rs" }),
                    invalid_arguments: None,
                }],
                replay: Some(ProviderReplay {
                    origin: "anthropic".into(),
                    content: serde_json::json!([{ "type": "thinking", "signature": "s" }]),
                }),
            },
            Message::ToolResult {
                call_id: "call_1".into(),
                content: vec![ContentPart::Text { text: "...".into() }],
                is_error: false,
            },
        ];
        let json = serde_json::to_string(&msgs).unwrap();
        assert!(json.contains(r#""kind":"assistant""#));
        let back: Vec<Message> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msgs);
    }

    #[test]
    fn conversation_round_trip_and_checkpoints_are_not_messages() {
        let mut c = Conversation::new();
        c.push_message(Message::user_text("hi"));
        c.push_checkpoint("cp1", "Added a button");
        c.push_message(Message::user_text("again"));
        let back = Conversation::from_json(&c.to_json().unwrap()).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.messages().len(), 2);
        assert_eq!(back.entries().len(), 3);
    }

    #[test]
    fn compaction_view_and_context_entries() {
        let mut c = Conversation::new();
        c.push_context("PROJECT.md: a todo app");
        c.push_message(Message::user_text("turn 1"));
        c.push_message(Message::user_text("answer 1"));
        c.push_checkpoint("cp", "x");
        let keep = c.entries().len();
        c.push_message(Message::user_text("turn 2"));
        c.push_compaction("They want a todo app; turn 1 done.", keep);
        c.push_message(Message::user_text("turn 3"));

        let view = c.messages();
        let texts: Vec<String> = view
            .iter()
            .map(|m| match m {
                Message::User { content } => match &content[0] {
                    ContentPart::Text { text } => text.clone(),
                    _ => String::new(),
                },
                _ => String::new(),
            })
            .collect();
        assert_eq!(texts.len(), 3);
        assert!(texts[0].starts_with(SUMMARY_PREFIX) && texts[0].contains("todo app; turn 1 done"));
        assert_eq!(texts[1], "turn 2");
        assert_eq!(texts[2], "turn 3");
        // Nothing was removed from the stored history.
        assert_eq!(c.entries().len(), 7);
        assert_eq!(c.last_context(), Some("PROJECT.md: a todo app"));
        let back = Conversation::from_json(&c.to_json().unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn v1_files_are_upgraded_on_load() {
        let v1 = r#"{"schema_version": 1, "entries": [{"entry": "message", "kind": "user", "content": [{"type": "text", "text": "hi"}]}]}"#;
        let c = Conversation::from_json(v1).unwrap();
        assert!(c.to_json().unwrap().contains("\"schema_version\": 2"));
        assert_eq!(c.messages().len(), 1);
    }

    #[test]
    fn conversation_schema_guard() {
        let newer = r#"{"schema_version": 99, "entries": []}"#;
        assert!(matches!(
            Conversation::from_json(newer),
            Err(ConversationError::TooNew { found: 99, .. })
        ));
        assert!(Conversation::from_json(r#"{"entries": []}"#).is_err());
    }
}
