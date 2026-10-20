//! A scripted fake LLM for tests (AGENTS.md §7): each model call plays the next
//! script. Also used by end-to-end tests in other crates.

use std::collections::VecDeque;
use std::sync::Mutex;

use ak_providers::{
    Capabilities, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, StopReason,
    StreamEvent, Usage,
};
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream;
use serde_json::Value;

pub enum Script {
    /// Stream these events (should end with `Done`).
    Reply(Vec<StreamEvent>),
    /// Fail the call before streaming.
    Fail(ProviderError),
    /// Stream these events, then hang forever (to test cancellation).
    Stall(Vec<StreamEvent>),
}

/// A reply with only text.
pub fn text(reply: &str) -> Script {
    Script::Reply(vec![
        StreamEvent::Usage(Usage {
            input_tokens: 10,
            output_tokens: 5,
            ..Usage::default()
        }),
        StreamEvent::TextDelta(reply.to_owned()),
        StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
        },
    ])
}

/// A reply calling tools: `(id, name, arguments)`; arguments are sent as raw text, so
/// tests can also send invalid JSON.
pub fn tool_calls(calls: &[(&str, &str, &str)]) -> Script {
    let mut events = Vec::new();
    for (index, (id, name, args)) in calls.iter().enumerate() {
        events.push(StreamEvent::ToolCallStart {
            index,
            id: (*id).to_owned(),
            name: (*name).to_owned(),
        });
        events.push(StreamEvent::ToolCallDelta {
            index,
            arguments_fragment: (*args).to_owned(),
        });
    }
    events.push(StreamEvent::Done {
        stop_reason: StopReason::ToolUse,
    });
    Script::Reply(events)
}

/// Arguments JSON for [`tool_calls`].
pub fn args(v: Value) -> String {
    v.to_string()
}

pub struct ScriptedProvider {
    scripts: Mutex<VecDeque<Script>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl ScriptedProvider {
    pub fn new(scripts: Vec<Script>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Every request the agent sent, in order.
    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }

    pub fn remaining(&self) -> usize {
        self.scripts.lock().map(|s| s.len()).unwrap_or_default()
    }
}

impl Provider for ScriptedProvider {
    fn id(&self) -> &'static str {
        "scripted"
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>> {
        Box::pin(async {
            Ok(vec![ModelInfo {
                id: "scripted".into(),
                display_name: None,
            }])
        })
    }

    fn capabilities(&self, _: &str) -> BoxFuture<'_, Result<Capabilities, ProviderError>> {
        Box::pin(async {
            Ok(Capabilities {
                tool_calling: true,
                vision: true,
                thinking: true,
                context_window: Some(200_000),
                max_output_tokens: Some(64_000),
            })
        })
    }

    fn stream(&self, request: ChatRequest) -> BoxFuture<'_, Result<EventStream, ProviderError>> {
        if let Ok(mut r) = self.requests.lock() {
            r.push(request);
        }
        let next = self.scripts.lock().ok().and_then(|mut s| s.pop_front());
        Box::pin(async move {
            match next {
                Some(Script::Reply(events)) => Ok(stream::iter(events.into_iter().map(Ok)).boxed()),
                Some(Script::Stall(events)) => Ok(stream::iter(events.into_iter().map(Ok))
                    .chain(stream::pending())
                    .boxed()),
                Some(Script::Fail(e)) => Err(e),
                None => Err(ProviderError::Transport("script exhausted".into())),
            }
        })
    }
}
