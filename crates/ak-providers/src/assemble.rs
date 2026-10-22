use std::collections::BTreeMap;

use ak_types::chat::{Message, ProviderReplay, Thinking, ToolCall};
use futures_util::StreamExt;

use crate::{EventStream, ProviderError, StopReason, StreamEvent, Usage};

/// A finished completion.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembled {
    /// Always [`Message::Assistant`].
    pub message: Message,
    pub usage: Usage,
    pub stop_reason: StopReason,
}

/// Folds [`StreamEvent`]s into one assistant [`Message`]. The UI consumes the same
/// events live; this produces what goes into the chat history.
pub struct MessageAssembler {
    origin: String,
    thinking: Vec<Thinking>,
    open_thinking: Option<String>,
    text: String,
    tool_calls: BTreeMap<usize, (String, String, String)>,
    replay: Option<serde_json::Value>,
    usage: Usage,
    stop_reason: Option<StopReason>,
}

impl MessageAssembler {
    /// `origin` is the provider id recorded on thinking and replay content.
    pub fn new(origin: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            thinking: Vec::new(),
            open_thinking: None,
            text: String::new(),
            tool_calls: BTreeMap::new(),
            replay: None,
            usage: Usage::default(),
            stop_reason: None,
        }
    }

    pub fn push(&mut self, event: StreamEvent) -> Result<(), ProviderError> {
        if self.stop_reason.is_some() {
            return Err(ProviderError::Decode("event after Done".into()));
        }
        match event {
            StreamEvent::TextDelta(t) => self.text.push_str(&t),
            StreamEvent::ThinkingDelta(t) => {
                self.open_thinking
                    .get_or_insert_with(String::new)
                    .push_str(&t);
            }
            StreamEvent::ThinkingEnd => self.close_thinking(),
            StreamEvent::ToolCallStart { index, id, name } => {
                if self
                    .tool_calls
                    .insert(index, (id, name, String::new()))
                    .is_some()
                {
                    return Err(ProviderError::Decode(format!(
                        "tool call {index} started twice"
                    )));
                }
            }
            StreamEvent::ToolCallDelta {
                index,
                arguments_fragment,
            } => match self.tool_calls.get_mut(&index) {
                Some((_, _, args)) => args.push_str(&arguments_fragment),
                None => {
                    return Err(ProviderError::Decode(format!(
                        "arguments for unknown tool call {index}"
                    )));
                }
            },
            StreamEvent::Fallback { .. } => {
                // The declined attempt's reasoning and tool calls must not be kept
                // or executed; its text stays as continuation context.
                self.thinking.clear();
                self.open_thinking = None;
                self.tool_calls.clear();
            }
            StreamEvent::Usage(u) => self.usage += u,
            StreamEvent::Replay(content) => self.replay = Some(content),
            StreamEvent::Done { stop_reason } => self.stop_reason = Some(stop_reason),
        }
        Ok(())
    }

    fn close_thinking(&mut self) {
        let text = self.open_thinking.take().unwrap_or_default();
        self.thinking.push(Thinking {
            text,
            origin: self.origin.clone(),
        });
    }

    pub fn finish(mut self) -> Result<Assembled, ProviderError> {
        let stop_reason = self.stop_reason.take().ok_or(ProviderError::Incomplete)?;
        if self.open_thinking.is_some() {
            self.close_thinking();
        }
        let tool_calls = self
            .tool_calls
            .into_values()
            .map(|(id, name, args)| {
                let (arguments, invalid_arguments) = if args.trim().is_empty() {
                    (serde_json::Value::Object(Default::default()), None)
                } else {
                    match serde_json::from_str(&args) {
                        // Some local models encode the arguments twice: a JSON
                        // string holding the object.
                        Ok(serde_json::Value::String(inner)) => {
                            match serde_json::from_str::<serde_json::Value>(&inner) {
                                Ok(v) if v.is_object() => (v, None),
                                _ => (serde_json::Value::String(inner), None),
                            }
                        }
                        Ok(v) => (v, None),
                        Err(_) => (serde_json::Value::Null, Some(args)),
                    }
                };
                ToolCall {
                    id,
                    name,
                    arguments,
                    invalid_arguments,
                }
            })
            .collect();
        let origin = self.origin;
        Ok(Assembled {
            message: Message::Assistant {
                thinking: self.thinking,
                text: self.text,
                tool_calls,
                replay: self
                    .replay
                    .map(|content| ProviderReplay { origin, content }),
            },
            usage: self.usage,
            stop_reason,
        })
    }
}

/// Drains a stream into an [`Assembled`] message.
pub async fn collect(
    mut stream: EventStream,
    origin: impl Into<String>,
) -> Result<Assembled, ProviderError> {
    let mut asm = MessageAssembler::new(origin);
    while let Some(event) = stream.next().await {
        asm.push(event?)?;
    }
    asm.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(i: u64, o: u64) -> StreamEvent {
        StreamEvent::Usage(Usage {
            input_tokens: i,
            output_tokens: o,
            ..Usage::default()
        })
    }

    fn start(index: usize, id: &str, name: &str) -> StreamEvent {
        StreamEvent::ToolCallStart {
            index,
            id: id.into(),
            name: name.into(),
        }
    }

    fn args(index: usize, s: &str) -> StreamEvent {
        StreamEvent::ToolCallDelta {
            index,
            arguments_fragment: s.into(),
        }
    }

    fn done(stop_reason: StopReason) -> StreamEvent {
        StreamEvent::Done { stop_reason }
    }

    fn assistant(m: Message) -> (Vec<Thinking>, String, Vec<ToolCall>, Option<ProviderReplay>) {
        let Message::Assistant {
            thinking,
            text,
            tool_calls,
            replay,
        } = m
        else {
            panic!("not an assistant message");
        };
        (thinking, text, tool_calls, replay)
    }

    #[test]
    fn assembles_thinking_text_tool_calls_and_replay() {
        let mut a = MessageAssembler::new("anthropic");
        for ev in [
            usage(10, 0),
            StreamEvent::ThinkingDelta("look at ".into()),
            StreamEvent::ThinkingDelta("the file".into()),
            StreamEvent::ThinkingEnd,
            StreamEvent::TextDelta("Reading it.".into()),
            start(1, "c1", "read_file"),
            args(1, "{\"path\":"),
            args(1, "\"a.rs\"}"),
            start(2, "c2", "list_files"),
            usage(0, 42),
            StreamEvent::Replay(serde_json::json!([{ "type": "text" }])),
            done(StopReason::ToolUse),
        ] {
            a.push(ev).unwrap();
        }
        let done = a.finish().unwrap();
        assert_eq!(
            (done.usage.input_tokens, done.usage.output_tokens),
            (10, 42)
        );
        assert_eq!(done.stop_reason, StopReason::ToolUse);
        let (thinking, text, tool_calls, replay) = assistant(done.message);
        assert_eq!(text, "Reading it.");
        assert_eq!(thinking.len(), 1);
        assert_eq!(thinking[0].text, "look at the file");
        assert_eq!(thinking[0].origin, "anthropic");
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].arguments["path"], "a.rs");
        assert_eq!(tool_calls[1].arguments, serde_json::json!({}));
        assert_eq!(replay.unwrap().origin, "anthropic");
    }

    #[test]
    fn missing_done_is_incomplete() {
        let mut a = MessageAssembler::new("x");
        a.push(StreamEvent::TextDelta("hi".into())).unwrap();
        assert_eq!(a.finish(), Err(ProviderError::Incomplete));
    }

    #[test]
    fn invalid_tool_arguments_are_marked_not_fatal() {
        let mut a = MessageAssembler::new("x");
        a.push(start(0, "c", "write_file")).unwrap();
        a.push(args(0, "{\"path\":\"a.rs\",\"content\":\"cut off"))
            .unwrap();
        a.push(done(StopReason::MaxTokens)).unwrap();
        let (_, _, calls, _) = assistant(a.finish().unwrap().message);
        assert_eq!(calls[0].arguments, serde_json::Value::Null);
        assert!(
            calls[0]
                .invalid_arguments
                .as_deref()
                .unwrap()
                .contains("cut off")
        );
    }

    /// Regression: a local model sent `"{\"path\":…}"` now and then, and the
    /// tool answered that `path` was missing.
    #[test]
    fn arguments_encoded_twice_are_unwrapped() {
        let mut a = MessageAssembler::new("x");
        a.push(start(0, "c", "read_file")).unwrap();
        a.push(args(0, r#""{\"path\":\"a.rs\"}""#)).unwrap();
        a.push(done(StopReason::ToolUse)).unwrap();
        let (_, _, calls, _) = assistant(a.finish().unwrap().message);
        assert_eq!(calls[0].arguments, serde_json::json!({ "path": "a.rs" }));
        assert_eq!(calls[0].invalid_arguments, None);
    }

    #[test]
    fn fallback_voids_earlier_thinking_and_tool_calls_but_keeps_text() {
        let mut a = MessageAssembler::new("anthropic");
        for ev in [
            StreamEvent::ThinkingDelta("declined attempt".into()),
            StreamEvent::ThinkingEnd,
            StreamEvent::TextDelta("Partial. ".into()),
            start(0, "c0", "run"),
            StreamEvent::Fallback {
                from: "claude-fable-5-1".into(),
                to: "claude-opus-5".into(),
            },
            StreamEvent::TextDelta("Continued.".into()),
            start(1, "c1", "read_file"),
            done(StopReason::ToolUse),
        ] {
            a.push(ev).unwrap();
        }
        let (thinking, text, calls, _) = assistant(a.finish().unwrap().message);
        assert!(thinking.is_empty());
        assert_eq!(text, "Partial. Continued.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
    }

    #[test]
    fn unclosed_trailing_thinking_is_kept_for_display() {
        let mut a = MessageAssembler::new("openai-compatible");
        a.push(StreamEvent::ThinkingDelta("hmm".into())).unwrap();
        a.push(done(StopReason::EndTurn)).unwrap();
        let (thinking, _, _, replay) = assistant(a.finish().unwrap().message);
        assert_eq!(thinking[0].text, "hmm");
        assert!(replay.is_none());
    }

    #[test]
    fn events_after_done_and_orphan_deltas_fail() {
        let mut a = MessageAssembler::new("x");
        assert!(a.push(args(9, "{}")).is_err());
        a.push(done(StopReason::EndTurn)).unwrap();
        assert!(a.push(StreamEvent::TextDelta("late".into())).is_err());
    }
}
