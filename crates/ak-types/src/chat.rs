//! Provider-neutral conversation messages (AGENTS.md §4, "Chat history format").
//!
//! Provider adapters convert these to and from their wire formats. Nothing here is
//! specific to one provider; provider-specific data is either dropped or kept as an
//! opaque, origin-tagged blob (see [`Thinking`]).

use serde::{Deserialize, Serialize};

/// One message in a conversation, in the order the model sees them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    User {
        content: Vec<ContentPart>,
    },
    Assistant {
        /// Reasoning shown collapsed in the UI. Replayed only to its origin provider.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        thinking: Vec<Thinking>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCall>,
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

/// Model reasoning.
///
/// Some providers (Anthropic extended thinking with tool use) require signed thinking
/// to be sent back unchanged within the same provider. Adapters replay a block only
/// when `origin` matches themselves, and drop it otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thinking {
    pub text: String,
    /// Provider id that produced this block, e.g. `"anthropic"`.
    pub origin: String,
    /// Opaque provider data (signature, redacted payload) needed for replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned id, echoed back in [`Message::ToolResult::call_id`].
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
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
                    signature: Some("sig".into()),
                }],
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({ "path": "src/app.rs" }),
                }],
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
