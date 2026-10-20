use std::collections::BTreeMap;

use ak_types::chat::{Message, Thinking, ToolCall};
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
    usage: Usage,
    stop_reason: Option<StopReason>,
}

impl MessageAssembler {
    /// `origin` is the provider id recorded on thinking blocks.
    pub fn new(origin: impl Into<String>) -> Self {
        Self {
            origin: origin.into(),
            thinking: Vec::new(),
            open_thinking: None,
            text: String::new(),
            tool_calls: BTreeMap::new(),
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
            StreamEvent::ThinkingSignature(sig) => {
                let text = self.open_thinking.take().unwrap_or_default();
                self.thinking.push(Thinking {
                    text,
                    origin: self.origin.clone(),
                    signature: Some(sig),
                });
            }
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
            StreamEvent::Usage(u) => self.usage += u,
            StreamEvent::Done { stop_reason } => self.stop_reason = Some(stop_reason),
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<Assembled, ProviderError> {
        let stop_reason = self.stop_reason.ok_or(ProviderError::Incomplete)?;
        if let Some(text) = self.open_thinking.take() {
            // Unsigned thinking (e.g. OpenAI-compatible reasoning): display-only.
            self.thinking.push(Thinking {
                text,
                origin: self.origin.clone(),
                signature: None,
            });
        }
        let tool_calls = self
            .tool_calls
            .into_values()
            .map(|(id, name, args)| {
                let arguments = if args.trim().is_empty() {
                    serde_json::Value::Object(Default::default())
                } else {
                    serde_json::from_str(&args).map_err(|e| {
                        ProviderError::Decode(format!("tool `{name}` arguments: {e}"))
                    })?
                };
                Ok(ToolCall {
                    id,
                    name,
                    arguments,
                })
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;
        Ok(Assembled {
            message: Message::Assistant {
                thinking: self.thinking,
                text: self.text,
                tool_calls,
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

    #[test]
    fn assembles_thinking_text_and_tool_calls() {
        let mut a = MessageAssembler::new("anthropic");
        for ev in [
            usage(10, 0),
            StreamEvent::ThinkingDelta("look at ".into()),
            StreamEvent::ThinkingDelta("the file".into()),
            StreamEvent::ThinkingSignature("sig1".into()),
            StreamEvent::TextDelta("Reading it.".into()),
            StreamEvent::ToolCallStart {
                index: 1,
                id: "c1".into(),
                name: "read_file".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 1,
                arguments_fragment: "{\"path\":".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 1,
                arguments_fragment: "\"a.rs\"}".into(),
            },
            StreamEvent::ToolCallStart {
                index: 2,
                id: "c2".into(),
                name: "list_files".into(),
            },
            usage(0, 42),
            StreamEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ] {
            a.push(ev).unwrap();
        }
        let done = a.finish().unwrap();
        assert_eq!(done.usage.input_tokens, 10);
        assert_eq!(done.usage.output_tokens, 42);
        assert_eq!(done.stop_reason, StopReason::ToolUse);
        let Message::Assistant {
            thinking,
            text,
            tool_calls,
        } = done.message
        else {
            panic!("not an assistant message");
        };
        assert_eq!(text, "Reading it.");
        assert_eq!(thinking.len(), 1);
        assert_eq!(thinking[0].text, "look at the file");
        assert_eq!(thinking[0].origin, "anthropic");
        assert_eq!(thinking[0].signature.as_deref(), Some("sig1"));
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0].arguments["path"], "a.rs");
        assert_eq!(tool_calls[1].arguments, serde_json::json!({}));
    }

    #[test]
    fn missing_done_is_incomplete() {
        let mut a = MessageAssembler::new("x");
        a.push(StreamEvent::TextDelta("hi".into())).unwrap();
        assert_eq!(a.finish(), Err(ProviderError::Incomplete));
    }

    #[test]
    fn bad_tool_arguments_fail() {
        let mut a = MessageAssembler::new("x");
        a.push(StreamEvent::ToolCallStart {
            index: 0,
            id: "c".into(),
            name: "t".into(),
        })
        .unwrap();
        a.push(StreamEvent::ToolCallDelta {
            index: 0,
            arguments_fragment: "{oops".into(),
        })
        .unwrap();
        a.push(StreamEvent::Done {
            stop_reason: StopReason::ToolUse,
        })
        .unwrap();
        assert!(matches!(a.finish(), Err(ProviderError::Decode(_))));
    }

    #[test]
    fn unsigned_trailing_thinking_is_kept_for_display() {
        let mut a = MessageAssembler::new("openai-compatible");
        a.push(StreamEvent::ThinkingDelta("hmm".into())).unwrap();
        a.push(StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
        })
        .unwrap();
        let Message::Assistant { thinking, .. } = a.finish().unwrap().message else {
            panic!();
        };
        assert_eq!(thinking[0].signature, None);
    }

    #[test]
    fn events_after_done_and_orphan_deltas_fail() {
        let mut a = MessageAssembler::new("x");
        assert!(
            a.push(StreamEvent::ToolCallDelta {
                index: 9,
                arguments_fragment: "{}".into()
            })
            .is_err()
        );
        a.push(StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
        })
        .unwrap();
        assert!(a.push(StreamEvent::TextDelta("late".into())).is_err());
    }
}
