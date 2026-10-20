//! Chat Completions SSE chunks -> neutral [`StreamEvent`]s.
//!
//! Servers differ: reasoning arrives as `delta.reasoning` (Ollama, gpt-oss),
//! `delta.reasoning_content` (LM Studio, others), `delta.reasoning_details`
//! (OpenRouter) or inline `<think>` tags; tool calls arrive whole (Ollama) or as
//! argument fragments (OpenAI, LM Studio). Unknown fields are ignored.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::Flavor;
use super::think_tags::{Piece, ThinkSplitter};
use crate::sse::SseEvent;
use crate::{ProviderError, StopReason, StreamEvent, Usage};

#[derive(Default)]
struct ToolCallState {
    id: String,
    name: String,
    args: String,
    started: bool,
}

pub struct Decoder {
    flavor: Flavor,
    splitter: ThinkSplitter,
    thinking_open: bool,
    text: String,
    refusal: String,
    /// `delta.reasoning` text, echoed back to Ollama.
    reasoning: String,
    /// OpenRouter `reasoning_details`, echoed back unmodified.
    reasoning_details: Vec<Value>,
    tools: BTreeMap<usize, ToolCallState>,
    finish: Option<String>,
    seen: Usage,
    done: bool,
}

impl Decoder {
    pub fn new(flavor: Flavor) -> Self {
        Self {
            flavor,
            splitter: ThinkSplitter::default(),
            thinking_open: false,
            text: String::new(),
            refusal: String::new(),
            reasoning: String::new(),
            reasoning_details: Vec::new(),
            tools: BTreeMap::new(),
            finish: None,
            seen: Usage::default(),
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn push(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError> {
        let data = ev.data.trim();
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data == "[DONE]" {
            return Ok(self.finalize());
        }
        let chunk: Value =
            serde_json::from_str(data).map_err(|e| ProviderError::Decode(format!("chunk: {e}")))?;
        if chunk.get("error").is_some_and(|e| !e.is_null()) {
            return Err(chunk_error(&chunk["error"]));
        }
        let mut out = Vec::new();
        if let Some(choice) = chunk["choices"].as_array().and_then(|c| c.first()) {
            self.delta(&choice["delta"], &mut out);
            if let Some(reason) = choice["finish_reason"].as_str() {
                if reason == "error" {
                    return Err(ProviderError::Server {
                        status: 500,
                        message: "the provider ended the stream with an error".into(),
                    });
                }
                self.finish = Some(reason.to_owned());
            }
        }
        self.usage(&chunk["usage"], &mut out);
        Ok(out)
    }

    /// The body ended without `[DONE]`. Some servers omit it; a finish reason is
    /// enough. Without one the connection dropped.
    pub fn end_of_stream(&mut self) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.finish.is_some() {
            Ok(self.finalize())
        } else {
            Err(ProviderError::Incomplete)
        }
    }

    fn delta(&mut self, delta: &Value, out: &mut Vec<StreamEvent>) {
        let mut plain_reasoning = false;
        for field in ["reasoning", "reasoning_content"] {
            if let Some(r) = delta[field].as_str().filter(|r| !r.is_empty()) {
                plain_reasoning = true;
                if field == "reasoning" {
                    self.reasoning.push_str(r);
                }
                self.think(r, out);
            }
        }
        if let Some(details) = delta["reasoning_details"].as_array() {
            for d in details {
                if !plain_reasoning && let Some(t) = d["text"].as_str().or(d["summary"].as_str()) {
                    self.think(t, out);
                }
                self.reasoning_details.push(d.clone());
            }
        }
        if let Some(c) = delta["content"].as_str().filter(|c| !c.is_empty()) {
            for piece in self.splitter.push(c) {
                self.piece(piece, out);
            }
        }
        if let Some(r) = delta["refusal"].as_str().filter(|r| !r.is_empty()) {
            self.close_thinking(out);
            self.refusal.push_str(r);
            out.push(StreamEvent::TextDelta(r.to_owned()));
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            self.close_thinking(out);
            for (pos, call) in calls.iter().enumerate() {
                let index = call["index"].as_u64().map_or(pos, |i| i as usize);
                let state = self.tools.entry(index).or_default();
                if let Some(id) = call["id"].as_str().filter(|s| !s.is_empty()) {
                    state.id = id.to_owned();
                }
                if let Some(name) = call["function"]["name"].as_str().filter(|s| !s.is_empty()) {
                    state.name.push_str(name);
                }
                let frag = call["function"]["arguments"].as_str().unwrap_or_default();
                state.args.push_str(frag);
                if !state.started && !state.name.is_empty() {
                    // Start once the name is known; flush arguments buffered so far.
                    state.started = true;
                    if state.id.is_empty() {
                        state.id = format!("call_{index}");
                    }
                    out.push(StreamEvent::ToolCallStart {
                        index,
                        id: state.id.clone(),
                        name: state.name.clone(),
                    });
                    if !state.args.is_empty() {
                        out.push(StreamEvent::ToolCallDelta {
                            index,
                            arguments_fragment: state.args.clone(),
                        });
                    }
                } else if state.started && !frag.is_empty() {
                    out.push(StreamEvent::ToolCallDelta {
                        index,
                        arguments_fragment: frag.to_owned(),
                    });
                }
            }
        }
    }

    fn piece(&mut self, piece: Piece, out: &mut Vec<StreamEvent>) {
        match piece {
            Piece::Thinking(t) => self.think(&t, out),
            Piece::ThinkEnd => self.close_thinking(out),
            Piece::Text(t) => {
                self.close_thinking(out);
                self.text.push_str(&t);
                out.push(StreamEvent::TextDelta(t));
            }
        }
    }

    fn think(&mut self, t: &str, out: &mut Vec<StreamEvent>) {
        self.thinking_open = true;
        out.push(StreamEvent::ThinkingDelta(t.to_owned()));
    }

    fn close_thinking(&mut self, out: &mut Vec<StreamEvent>) {
        if self.thinking_open {
            self.thinking_open = false;
            out.push(StreamEvent::ThinkingEnd);
        }
    }

    fn usage(&mut self, u: &Value, out: &mut Vec<StreamEvent>) {
        if !u.is_object() {
            return;
        }
        let n = |v: &Value| v.as_u64().unwrap_or(0);
        let cached = n(&u["prompt_tokens_details"]["cached_tokens"]);
        let total = Usage {
            // Neutral input excludes cache reads, as Anthropic reports it.
            input_tokens: n(&u["prompt_tokens"]).saturating_sub(cached),
            output_tokens: n(&u["completion_tokens"]),
            cache_read_tokens: cached,
            cache_write_tokens: n(&u["prompt_tokens_details"]["cache_write_tokens"]),
        };
        let delta = Usage {
            input_tokens: total.input_tokens.saturating_sub(self.seen.input_tokens),
            output_tokens: total.output_tokens.saturating_sub(self.seen.output_tokens),
            cache_read_tokens: total
                .cache_read_tokens
                .saturating_sub(self.seen.cache_read_tokens),
            cache_write_tokens: total
                .cache_write_tokens
                .saturating_sub(self.seen.cache_write_tokens),
        };
        self.seen = total;
        if delta != Usage::default() {
            out.push(StreamEvent::Usage(delta));
        }
    }

    fn finalize(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        for piece in self.splitter.finish() {
            self.piece(piece, &mut out);
        }
        self.close_thinking(&mut out);

        let mut msg = json!({
            "role": "assistant",
            "content": if self.text.is_empty() { Value::Null } else { json!(self.text) },
        });
        if let Some(o) = msg.as_object_mut() {
            let started: Vec<&ToolCallState> = self.tools.values().filter(|t| t.started).collect();
            if !started.is_empty() {
                let calls: Vec<Value> = started
                    .iter()
                    .map(|t| {
                        // Never echo invalid JSON: servers reject it.
                        let args = if serde_json::from_str::<Value>(&t.args).is_ok() {
                            t.args.clone()
                        } else {
                            "{}".to_owned()
                        };
                        json!({ "id": t.id, "type": "function",
                                "function": { "name": t.name, "arguments": args } })
                    })
                    .collect();
                o.insert("tool_calls".into(), Value::Array(calls));
            }
            if self.flavor == Flavor::Ollama && !self.reasoning.is_empty() {
                o.insert("reasoning".into(), json!(self.reasoning));
            }
            if !self.reasoning_details.is_empty() {
                o.insert(
                    "reasoning_details".into(),
                    Value::Array(self.reasoning_details.clone()),
                );
            }
        }
        out.push(StreamEvent::Replay(msg));

        let has_tools = self.tools.values().any(|t| t.started);
        let stop_reason = match self.finish.as_deref() {
            _ if !self.refusal.is_empty() => StopReason::Refusal { category: None },
            Some("content_filter") => StopReason::Refusal {
                category: Some("content_filter".into()),
            },
            Some("length") => StopReason::MaxTokens,
            Some("tool_calls" | "function_call") => StopReason::ToolUse,
            // Some servers report `stop` even when they returned tool calls.
            _ if has_tools => StopReason::ToolUse,
            Some("stop") => StopReason::EndTurn,
            Some(other) => StopReason::Other(other.to_owned()),
            None => StopReason::Other("unknown".into()),
        };
        out.push(StreamEvent::Done { stop_reason });
        self.done = true;
        out
    }
}

fn chunk_error(err: &Value) -> ProviderError {
    let message = err["message"]
        .as_str()
        .or(err.as_str())
        .unwrap_or("provider error")
        .to_owned();
    match err["code"].as_u64().and_then(|c| u16::try_from(c).ok()) {
        Some(status) => ProviderError::from_status(status, None, &message),
        None => ProviderError::Server {
            status: 500,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(v: Value) -> SseEvent {
        SseEvent {
            event: None,
            data: v.to_string(),
        }
    }

    fn run(flavor: Flavor, chunks: Vec<Value>) -> Vec<StreamEvent> {
        let mut d = Decoder::new(flavor);
        let mut out: Vec<StreamEvent> = chunks
            .into_iter()
            .flat_map(|c| d.push(&data(c)).unwrap())
            .collect();
        out.extend(
            d.push(&SseEvent {
                event: None,
                data: "[DONE]".into(),
            })
            .unwrap(),
        );
        out
    }

    fn replay(events: &[StreamEvent]) -> Value {
        events
            .iter()
            .find_map(|e| match e {
                StreamEvent::Replay(v) => Some(v.clone()),
                _ => None,
            })
            .unwrap()
    }

    fn stop(events: &[StreamEvent]) -> StopReason {
        events
            .iter()
            .find_map(|e| match e {
                StreamEvent::Done { stop_reason } => Some(stop_reason.clone()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn name_arriving_late_delays_tool_start() {
        let ev = run(
            Flavor::Generic,
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"arguments":"{\"a\""}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"t","arguments":":1}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
            ],
        );
        assert!(matches!(&ev[0], StreamEvent::ToolCallStart { name, .. } if name == "t"));
        assert_eq!(
            ev[1],
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments_fragment: "{\"a\":1}".into()
            }
        );
        assert_eq!(
            stop(&ev),
            StopReason::ToolUse,
            "stop + tool calls => ToolUse"
        );
        assert_eq!(
            replay(&ev)["tool_calls"][0]["function"]["arguments"],
            "{\"a\":1}"
        );
    }

    #[test]
    fn refusal_and_content_filter() {
        let ev = run(
            Flavor::OpenAi,
            vec![
                json!({"choices":[{"delta":{"refusal":"I can't help with that."},"finish_reason":"stop"}]}),
            ],
        );
        assert_eq!(stop(&ev), StopReason::Refusal { category: None });
        let ev = run(
            Flavor::OpenAi,
            vec![json!({"choices":[{"delta":{},"finish_reason":"content_filter"}]})],
        );
        assert_eq!(
            stop(&ev),
            StopReason::Refusal {
                category: Some("content_filter".into())
            }
        );
    }

    #[test]
    fn error_chunks_and_missing_done() {
        let mut d = Decoder::new(Flavor::OpenRouter);
        let err = d
            .push(&data(
                json!({"error":{"code":429,"message":"slow down"},"choices":[]}),
            ))
            .unwrap_err();
        assert!(matches!(err, ProviderError::RateLimited { .. }));

        let mut d = Decoder::new(Flavor::Generic);
        d.push(&data(json!({"choices":[{"delta":{"content":"cut"}}]})))
            .unwrap();
        assert_eq!(d.end_of_stream().unwrap_err(), ProviderError::Incomplete);

        let mut d = Decoder::new(Flavor::Generic);
        d.push(&data(
            json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}),
        ))
        .unwrap();
        assert!(
            d.end_of_stream().is_ok(),
            "finish reason without [DONE] is fine"
        );
    }

    #[test]
    fn usage_excludes_cached_from_input() {
        let ev = run(
            Flavor::OpenAi,
            vec![
                json!({"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":7,
                         "prompt_tokens_details":{"cached_tokens":60}}}),
            ],
        );
        assert!(ev.contains(&StreamEvent::Usage(Usage {
            input_tokens: 40,
            output_tokens: 7,
            cache_read_tokens: 60,
            cache_write_tokens: 0,
        })));
    }
}
