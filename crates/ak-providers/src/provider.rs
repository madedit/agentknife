use std::ops::AddAssign;

use ak_types::chat::{Message, ToolSpec};
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::ProviderError;

/// One streaming chat completion request, in neutral form.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    /// Upper bound; adapters clamp it to the model's output limit.
    pub max_tokens: u32,
    /// How hard the model should think. `None` uses the model's default. Adapters map
    /// it to whatever the model supports (and step down to the nearest lower level).
    pub effort: Option<Effort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
}

impl Effort {
    pub const ALL: [Effort; 5] = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];

    /// Wire name used by providers (`"xhigh"` etc.).
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

/// Incremental output of a streaming completion.
///
/// Ordering contract for adapters:
/// - `ThinkingDelta`s append to the open thinking block; `ThinkingEnd` closes it.
/// - Tool calls are keyed by `index`: one `ToolCallStart`, then argument fragments
///   whose concatenation is a JSON object (or empty, meaning `{}`).
/// - `Fallback` marks a server-side switch to another model after a refusal: thinking
///   and tool calls received before it are void (text stays).
/// - `Usage` events are deltas; their sum is the request's total.
/// - `Replay` (at most once, before `Done`) carries the provider's exact content
///   for verbatim echo on the next request.
/// - The stream ends with exactly one `Done`.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ThinkingEnd,
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        arguments_fragment: String,
    },
    Fallback {
        from: String,
        to: String,
    },
    Usage(Usage),
    Replay(serde_json::Value),
    Done {
        stop_reason: StopReason,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl AddAssign for Usage {
    fn add_assign(&mut self, rhs: Self) {
        self.input_tokens += rhs.input_tokens;
        self.output_tokens += rhs.output_tokens;
        self.cache_read_tokens += rhs.cache_read_tokens;
        self.cache_write_tokens += rhs.cache_write_tokens;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    /// The model or a safety classifier declined. Partial output must be discarded.
    Refusal {
        category: Option<String>,
    },
    Other(String),
}

/// What a model can do. Probed on connect; agent features degrade from it
/// (no tool calling: warn the user; no vision: DOM summary instead of screenshots).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub tool_calling: bool,
    pub vision: bool,
    pub thinking: bool,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: Option<String>,
}

pub type EventStream = BoxStream<'static, Result<StreamEvent, ProviderError>>;

/// An LLM backend. Object-safe so the provider can be chosen at runtime.
pub trait Provider: Send + Sync {
    /// Stable id, also used as [`ak_types::chat::Thinking::origin`] and
    /// [`ak_types::chat::ProviderReplay::origin`].
    fn id(&self) -> &'static str;

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>>;

    fn capabilities(&self, model: &str) -> BoxFuture<'_, Result<Capabilities, ProviderError>>;

    /// Starts a completion. Connection and HTTP status errors resolve the future;
    /// errors mid-stream arrive as stream items.
    fn stream(&self, request: ChatRequest) -> BoxFuture<'_, Result<EventStream, ProviderError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_is_object_safe_and_shareable() {
        fn assert_dyn(_: Option<std::sync::Arc<dyn Provider>>) {}
        assert_dyn(None);
    }

    #[test]
    fn usage_adds_up() {
        let mut total = Usage::default();
        total += Usage {
            input_tokens: 5,
            ..Usage::default()
        };
        total += Usage {
            output_tokens: 7,
            cache_read_tokens: 2,
            ..Usage::default()
        };
        assert_eq!((total.input_tokens, total.output_tokens), (5, 7));
        assert_eq!(total.cache_read_tokens, 2);
    }

    #[test]
    fn effort_orders_and_serializes() {
        assert!(Effort::Low < Effort::XHigh);
        assert_eq!(serde_json::to_string(&Effort::XHigh).unwrap(), "\"xhigh\"");
        for e in Effort::ALL {
            assert_eq!(
                serde_json::to_string(&e).unwrap(),
                format!("\"{}\"", e.as_str())
            );
        }
    }
}
