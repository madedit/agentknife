//! Messages API SSE events -> neutral [`StreamEvent`]s.
//!
//! Besides the live events, the decoder rebuilds the exact content blocks so they
//! can be echoed back unchanged (signed thinking, block order) as [`StreamEvent::Replay`].

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::sse::SseEvent;
use crate::{ProviderError, StopReason, StreamEvent, Usage};

#[derive(Default)]
pub struct Decoder {
    /// Raw blocks by stream index, rebuilt from deltas.
    blocks: BTreeMap<usize, Value>,
    /// `input_json_delta` text per tool_use block.
    tool_json: BTreeMap<usize, String>,
    stop_reason: Option<StopReason>,
    /// Cumulative usage seen so far; deltas are emitted against it.
    seen: Usage,
    done: bool,
}

impl Decoder {
    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn push(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError> {
        if ev.data.is_empty() {
            return Ok(Vec::new());
        }
        let data: Value = serde_json::from_str(&ev.data)
            .map_err(|e| ProviderError::Decode(format!("event `{:?}`: {e}", ev.event)))?;
        let kind = data["type"].as_str().or(ev.event.as_deref()).unwrap_or("");
        let mut out = Vec::new();
        match kind {
            "message_start" => self.usage(&data["message"]["usage"], &mut out),
            "content_block_start" => self.block_start(&data, &mut out)?,
            "content_block_delta" => self.block_delta(&data, &mut out)?,
            "content_block_stop" => self.block_stop(&data, &mut out),
            "message_delta" => {
                if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(stop_reason(reason, &data["delta"]["stop_details"]));
                }
                self.usage(&data["usage"], &mut out);
            }
            "message_stop" => {
                out.push(StreamEvent::Replay(Value::Array(self.replay_blocks())));
                out.push(StreamEvent::Done {
                    stop_reason: self
                        .stop_reason
                        .take()
                        .unwrap_or(StopReason::Other("unknown".into())),
                });
                self.done = true;
            }
            "error" => return Err(stream_error(&data["error"])),
            // `ping` and event types added later are ignored.
            _ => {}
        }
        Ok(out)
    }

    fn block_start(
        &mut self,
        data: &Value,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let index = index_of(data)?;
        let block = data["content_block"].clone();
        match block["type"].as_str().unwrap_or("") {
            "text" => {
                if let Some(t) = block["text"].as_str().filter(|t| !t.is_empty()) {
                    out.push(StreamEvent::TextDelta(t.to_owned()));
                }
            }
            "tool_use" => {
                out.push(StreamEvent::ToolCallStart {
                    index,
                    id: block["id"].as_str().unwrap_or_default().to_owned(),
                    name: block["name"].as_str().unwrap_or_default().to_owned(),
                });
                self.tool_json.insert(index, String::new());
            }
            "fallback" => out.push(StreamEvent::Fallback {
                from: block["from"]["model"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                to: block["to"]["model"].as_str().unwrap_or_default().to_owned(),
            }),
            // thinking (text arrives as deltas), redacted_thinking, server tools, and
            // unknown types: kept raw for replay only.
            _ => {}
        }
        self.blocks.insert(index, block);
        Ok(())
    }

    fn block_delta(
        &mut self,
        data: &Value,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let index = index_of(data)?;
        let delta = &data["delta"];
        let block = self.blocks.entry(index).or_insert_with(|| json!({}));
        let append = |block: &mut Value, field: &str, s: &str| {
            let cur = block[field].as_str().unwrap_or_default().to_owned();
            block[field] = Value::String(cur + s);
        };
        match delta["type"].as_str().unwrap_or("") {
            "text_delta" => {
                let t = delta["text"].as_str().unwrap_or_default();
                append(block, "text", t);
                out.push(StreamEvent::TextDelta(t.to_owned()));
            }
            "thinking_delta" => {
                let t = delta["thinking"].as_str().unwrap_or_default();
                append(block, "thinking", t);
                out.push(StreamEvent::ThinkingDelta(t.to_owned()));
            }
            "signature_delta" => {
                append(
                    block,
                    "signature",
                    delta["signature"].as_str().unwrap_or_default(),
                );
            }
            "input_json_delta" => {
                let frag = delta["partial_json"].as_str().unwrap_or_default();
                self.tool_json.entry(index).or_default().push_str(frag);
                out.push(StreamEvent::ToolCallDelta {
                    index,
                    arguments_fragment: frag.to_owned(),
                });
            }
            _ => {} // e.g. citations_delta: not used.
        }
        Ok(())
    }

    fn block_stop(&mut self, data: &Value, out: &mut Vec<StreamEvent>) {
        let Some(index) = data["index"].as_u64().map(|i| i as usize) else {
            return;
        };
        let Some(block) = self.blocks.get_mut(&index) else {
            return;
        };
        match block["type"].as_str().unwrap_or("") {
            "thinking" => out.push(StreamEvent::ThinkingEnd),
            "tool_use" => {
                let json = self.tool_json.get(&index).map(String::as_str).unwrap_or("");
                // Replayed input must be an object; cut-off JSON becomes `{}` here and
                // is flagged invalid by the assembler.
                block["input"] = serde_json::from_str::<Value>(json)
                    .ok()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
            }
            _ => {}
        }
    }

    /// Blocks to echo next turn. Before the last `fallback` boundary only text is
    /// kept; the boundary marker itself is dropped.
    fn replay_blocks(&self) -> Vec<Value> {
        let blocks: Vec<&Value> = self.blocks.values().collect();
        let boundary = blocks.iter().rposition(|b| b["type"] == "fallback");
        blocks
            .iter()
            .enumerate()
            .filter(|(i, b)| match boundary {
                Some(at) if *i < at => b["type"] == "text",
                Some(at) if *i == at => false,
                _ => true,
            })
            .map(|(_, b)| (*b).clone())
            .collect()
    }

    fn usage(&mut self, u: &Value, out: &mut Vec<StreamEvent>) {
        if !u.is_object() {
            return;
        }
        let mut delta = Usage::default();
        let track = |field: &str, seen: &mut u64, d: &mut u64| {
            if let Some(v) = u[field].as_u64() {
                *d = v.saturating_sub(*seen);
                *seen = (*seen).max(v);
            }
        };
        track(
            "input_tokens",
            &mut self.seen.input_tokens,
            &mut delta.input_tokens,
        );
        track(
            "output_tokens",
            &mut self.seen.output_tokens,
            &mut delta.output_tokens,
        );
        track(
            "cache_read_input_tokens",
            &mut self.seen.cache_read_tokens,
            &mut delta.cache_read_tokens,
        );
        track(
            "cache_creation_input_tokens",
            &mut self.seen.cache_write_tokens,
            &mut delta.cache_write_tokens,
        );
        if delta != Usage::default() {
            out.push(StreamEvent::Usage(delta));
        }
    }
}

fn index_of(data: &Value) -> Result<usize, ProviderError> {
    data["index"]
        .as_u64()
        .map(|i| i as usize)
        .ok_or_else(|| ProviderError::Decode("content block event without index".into()))
}

fn stop_reason(reason: &str, details: &Value) -> StopReason {
    match reason {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "refusal" => StopReason::Refusal {
            category: details["category"].as_str().map(str::to_owned),
        },
        other => StopReason::Other(other.to_owned()),
    }
}

/// Maps an in-stream `error` event (the HTTP status was already 200).
pub fn stream_error(err: &Value) -> ProviderError {
    let message = err["message"].as_str().unwrap_or_default().to_owned();
    match err["type"].as_str().unwrap_or("") {
        "overloaded_error" => ProviderError::Server {
            status: 529,
            message,
        },
        "rate_limit_error" => ProviderError::RateLimited {
            retry_after_secs: None,
        },
        "authentication_error" | "permission_error" => ProviderError::Auth,
        "invalid_request_error" => ProviderError::InvalidRequest(message),
        "not_found_error" => ProviderError::ModelNotFound(message),
        _ => ProviderError::Server {
            status: 500,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(data: Value) -> SseEvent {
        SseEvent {
            event: data["type"].as_str().map(str::to_owned),
            data: data.to_string(),
        }
    }

    fn run(events: Vec<Value>) -> Vec<StreamEvent> {
        let mut d = Decoder::default();
        events
            .into_iter()
            .flat_map(|e| d.push(&ev(e)).unwrap())
            .collect()
    }

    #[test]
    fn usage_is_emitted_as_deltas() {
        let out = run(vec![
            json!({"type":"message_start","message":{"usage":{"input_tokens":100,"output_tokens":1,"cache_read_input_tokens":80,"cache_creation_input_tokens":0}}}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}),
        ]);
        let total = out.iter().fold(Usage::default(), |mut acc, e| {
            if let StreamEvent::Usage(u) = e {
                acc += *u;
            }
            acc
        });
        assert_eq!(total.input_tokens, 100);
        assert_eq!(total.output_tokens, 42);
        assert_eq!(total.cache_read_tokens, 80);
    }

    #[test]
    fn refusal_and_unknown_stop_reasons() {
        assert_eq!(
            stop_reason("refusal", &json!({"category":"cyber"})),
            StopReason::Refusal {
                category: Some("cyber".into())
            }
        );
        assert_eq!(
            stop_reason("refusal", &Value::Null),
            StopReason::Refusal { category: None }
        );
        assert_eq!(
            stop_reason("pause_turn", &Value::Null),
            StopReason::Other("pause_turn".into())
        );
    }

    #[test]
    fn in_stream_errors_map_to_typed_errors() {
        let mut d = Decoder::default();
        let err = d
            .push(&ev(
                json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}),
            ))
            .unwrap_err();
        assert!(err.is_retryable());
    }

    #[test]
    fn replay_drops_pre_fallback_non_text_blocks() {
        let mut d = Decoder::default();
        for e in [
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"Partial."}}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"fallback","from":{"model":"a"},"to":{"model":"b"}}}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":3,"delta":{"type":"signature_delta","signature":"s2"}}),
        ] {
            d.push(&ev(e)).unwrap();
        }
        let kept = d.replay_blocks();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0]["text"], "Partial.");
        assert_eq!(kept[1]["signature"], "s2");
    }
}
