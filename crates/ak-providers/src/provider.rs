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
    pub max_tokens: u32,
    /// Reasoning budget in tokens; `None` disables thinking.
    pub thinking_budget: Option<u32>,
}

/// Incremental output of a streaming completion.
///
/// Ordering contract for adapters:
/// - `ThinkingDelta`s append to the open thinking block; `ThinkingSignature` closes it.
/// - Tool calls are keyed by `index`: one `ToolCallStart`, then argument fragments
///   whose concatenation is a JSON object (or empty, meaning `{}`).
/// - `Usage` events are deltas; their sum is the request's total.
/// - The stream ends with exactly one `Done`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ThinkingDelta(String),
    /// Opaque replay data for the open thinking block (signature or redacted payload).
    ThinkingSignature(String),
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: usize,
        arguments_fragment: String,
    },
    Usage(Usage),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: Option<String>,
}

pub type EventStream = BoxStream<'static, Result<StreamEvent, ProviderError>>;

/// An LLM backend. Object-safe so the provider can be chosen at runtime.
pub trait Provider: Send + Sync {
    /// Stable id, also used as [`ak_types::chat::Thinking::origin`].
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
}
