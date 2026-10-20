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
}
