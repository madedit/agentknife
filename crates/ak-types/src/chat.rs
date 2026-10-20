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
pub const CONVERSATION_SCHEMA_VERSION: u32 = 1;

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

    /// The messages to send to a model, in order.
    pub fn messages(&self) -> Vec<Message> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Message(m) => Some(m.clone()),
                Entry::Checkpoint { .. } => None,
            })
            .collect()
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
        // Version 1 is the first format; future migrations transform `raw` here.
        serde_json::from_value(raw).map_err(|e| ConversationError::Invalid(e.to_string()))
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
    fn conversation_schema_guard() {
        let newer = r#"{"schema_version": 99, "entries": []}"#;
        assert!(matches!(
            Conversation::from_json(newer),
            Err(ConversationError::TooNew { found: 99, .. })
        ));
        assert!(Conversation::from_json(r#"{"entries": []}"#).is_err());
    }
}
