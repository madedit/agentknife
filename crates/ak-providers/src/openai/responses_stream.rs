//! Responses API SSE events -> neutral [`StreamEvent`]s.
//!
//! Reasoning arrives as summaries (`reasoning_summary_text`, OpenAI) or as full
//! text (`reasoning_text`, open-weight servers). Function calls are announced by
//! `output_item.added` (call id and name) and filled by argument deltas. The
//! finished output items (`output_item.done`) become the replay, in output order.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::sse::SseEvent;
use crate::{ProviderError, StopReason, StreamEvent, Usage};

#[derive(Default)]
struct Call {
    started: bool,
    /// Arguments sent so far.
    sent: String,
}

#[derive(Default)]
pub struct Decoder {
    thinking_open: bool,
    /// The thinking item being streamed, to separate summary parts.
    thinking_item: Option<String>,
    calls: BTreeMap<u64, Call>,
    /// Finished output items by `output_index`.
    items: BTreeMap<u64, Value>,
    refusal: bool,
    done: bool,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn push(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError> {
        let data = ev.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(Vec::new());
        }
        let v: Value =
            serde_json::from_str(data).map_err(|e| ProviderError::Decode(format!("event: {e}")))?;
        let kind = v["type"]
            .as_str()
            .or(ev.event.as_deref())
            .unwrap_or_default();
        let mut out = Vec::new();
        let index = v["output_index"].as_u64().unwrap_or(0);
        match kind {
            "response.output_item.added" => self.item_added(index, &v["item"], &mut out),
            "response.output_item.done" => self.item_done(index, &v["item"], &mut out),
            "response.output_text.delta" => {
                if let Some(d) = delta(&v) {
                    self.close_thinking(&mut out);
                    out.push(StreamEvent::TextDelta(d.to_owned()));
                }
            }
            "response.refusal.delta" => {
                if let Some(d) = delta(&v) {
                    self.close_thinking(&mut out);
                    self.refusal = true;
                    out.push(StreamEvent::TextDelta(d.to_owned()));
                }
            }
            "response.reasoning_summary_part.added" => {
                // A new summary paragraph within the same reasoning item.
                if v["summary_index"].as_u64().unwrap_or(0) > 0 && self.thinking_open {
                    out.push(StreamEvent::ThinkingDelta("\n\n".into()));
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(d) = delta(&v) {
                    let item = v["item_id"].as_str().unwrap_or_default();
                    if self.thinking_item.as_deref() != Some(item) {
                        self.close_thinking(&mut out);
                        self.thinking_item = Some(item.to_owned());
                    }
                    self.thinking_open = true;
                    out.push(StreamEvent::ThinkingDelta(d.to_owned()));
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some(d), Some(call)) = (delta(&v), self.calls.get_mut(&index))
                    && call.started
                {
                    call.sent.push_str(d);
                    out.push(StreamEvent::ToolCallDelta {
                        index: as_usize(index),
                        arguments_fragment: d.to_owned(),
                    });
                }
            }
            "response.function_call_arguments.done" => {
                if let Some(args) = v["arguments"].as_str() {
                    self.finish_arguments(index, args, &mut out);
                }
            }
            "response.completed" | "response.incomplete" => {
                self.finish(&v["response"], &mut out);
            }
            "response.failed" => return self.failed(&v["response"]),
            "error" => return Err(stream_error(&v)),
            // created, in_progress, content_part.*, *.done text events: nothing new.
            _ => {}
        }
        Ok(out)
    }

    /// The body ended without a final `response.*` event: the connection dropped.
    pub fn end_of_stream(&mut self) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.done {
            Ok(Vec::new())
        } else {
            Err(ProviderError::Incomplete)
        }
    }

    fn item_added(&mut self, index: u64, item: &Value, out: &mut Vec<StreamEvent>) {
        if item["type"] == "function_call" {
            self.close_thinking(out);
            self.start_call(index, item, out);
        }
    }

    fn start_call(&mut self, index: u64, item: &Value, out: &mut Vec<StreamEvent>) {
        let call = self.calls.entry(index).or_default();
        if call.started {
            return;
        }
        call.started = true;
        let id = item["call_id"]
            .as_str()
            .or(item["id"].as_str())
            .map_or_else(|| format!("call_{index}"), str::to_owned);
        out.push(StreamEvent::ToolCallStart {
            index: as_usize(index),
            id,
            name: item["name"].as_str().unwrap_or_default().to_owned(),
        });
        if let Some(args) = item["arguments"].as_str().filter(|a| !a.is_empty()) {
            call.sent.push_str(args);
            out.push(StreamEvent::ToolCallDelta {
                index: as_usize(index),
                arguments_fragment: args.to_owned(),
            });
        }
    }

    /// Sends whatever part of the final arguments the deltas did not carry.
    fn finish_arguments(&mut self, index: u64, args: &str, out: &mut Vec<StreamEvent>) {
        let Some(call) = self.calls.get_mut(&index) else {
            return;
        };
        if let Some(rest) = args.strip_prefix(call.sent.as_str())
            && !rest.is_empty()
        {
            call.sent.push_str(rest);
            out.push(StreamEvent::ToolCallDelta {
                index: as_usize(index),
                arguments_fragment: rest.to_owned(),
            });
        }
    }

    fn item_done(&mut self, index: u64, item: &Value, out: &mut Vec<StreamEvent>) {
        match item["type"].as_str() {
            Some("reasoning") => {
                if self.thinking_item.as_deref() == item["id"].as_str() {
                    self.close_thinking(out);
                }
            }
            Some("function_call") => {
                // Servers that skip `output_item.added` or the argument events.
                self.start_call(index, item, out);
                if let Some(args) = item["arguments"].as_str() {
                    self.finish_arguments(index, args, out);
                }
            }
            _ => {}
        }
        // `.done` carries the complete item (`.added` may lack encrypted content).
        self.items.insert(index, item.clone());
    }

    fn finish(&mut self, response: &Value, out: &mut Vec<StreamEvent>) {
        if self.done {
            return;
        }
        self.close_thinking(out);
        if let Some(u) = usage(&response["usage"]) {
            out.push(StreamEvent::Usage(u));
        }
        // Prefer the items we saw finish; fall back to the response's own list.
        let items: Vec<Value> = if self.items.is_empty() {
            response["output"].as_array().cloned().unwrap_or_default()
        } else {
            self.items.values().cloned().collect()
        };
        for (i, item) in items.iter().enumerate() {
            if item["type"] == "function_call" && !self.calls.contains_key(&(i as u64)) {
                self.start_call(i as u64, item, out);
            }
        }
        let has_calls = items.iter().any(|i| i["type"] == "function_call");
        out.push(StreamEvent::Replay(Value::Array(items)));
        let reason = response["incomplete_details"]["reason"].as_str();
        let stop_reason = match (response["status"].as_str(), reason) {
            _ if self.refusal => StopReason::Refusal { category: None },
            (Some("incomplete"), Some("max_output_tokens")) => StopReason::MaxTokens,
            (Some("incomplete"), Some("content_filter")) => StopReason::Refusal {
                category: Some("content_filter".into()),
            },
            (Some("incomplete"), r) => StopReason::Other(r.unwrap_or("incomplete").to_owned()),
            _ if has_calls => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        };
        out.push(StreamEvent::Done { stop_reason });
        self.done = true;
    }

    /// Policy blocks end the turn as a refusal; everything else is an error.
    fn failed(&mut self, response: &Value) -> Result<Vec<StreamEvent>, ProviderError> {
        let err = &response["error"];
        let code = err["code"].as_str().unwrap_or_default();
        if code.ends_with("_policy") || code.contains("policy_violation") {
            self.done = true;
            let mut out = Vec::new();
            self.close_thinking(&mut out);
            out.push(StreamEvent::Done {
                stop_reason: StopReason::Refusal {
                    category: Some(code.to_owned()),
                },
            });
            return Ok(out);
        }
        Err(error_from(code, err["message"].as_str()))
    }

    fn close_thinking(&mut self, out: &mut Vec<StreamEvent>) {
        if self.thinking_open {
            self.thinking_open = false;
            out.push(StreamEvent::ThinkingEnd);
        }
    }
}

fn delta(v: &Value) -> Option<&str> {
    v["delta"].as_str().filter(|d| !d.is_empty())
}

fn as_usize(i: u64) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// Responses usage is per response, sent once. Neutral input excludes cache reads.
fn usage(u: &Value) -> Option<Usage> {
    if !u.is_object() {
        return None;
    }
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let cached = n(&u["input_tokens_details"]["cached_tokens"]);
    Some(Usage {
        input_tokens: n(&u["input_tokens"]).saturating_sub(cached),
        output_tokens: n(&u["output_tokens"]),
        cache_read_tokens: cached,
        cache_write_tokens: n(&u["input_tokens_details"]["cache_write_tokens"]),
    })
}

/// The top-level `error` event: `{code, message}` (some servers nest `error`).
fn stream_error(v: &Value) -> ProviderError {
    let e = if v["error"].is_object() {
        &v["error"]
    } else {
        v
    };
    error_from(
        e["code"].as_str().unwrap_or_default(),
        e["message"].as_str(),
    )
}

fn error_from(code: &str, message: Option<&str>) -> ProviderError {
    let message = message.unwrap_or("the provider ended the response with an error");
    match code {
        "rate_limit_exceeded" => ProviderError::RateLimited {
            retry_after_secs: None,
        },
        "invalid_prompt"
        | "invalid_base64_image"
        | "image_too_large"
        | "context_length_exceeded" => ProviderError::InvalidRequest(message.to_owned()),
        _ => ProviderError::Server {
            status: 500,
            message: if code.is_empty() {
                message.to_owned()
            } else {
                format!("{code}: {message}")
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run(events: Vec<Value>) -> Vec<StreamEvent> {
        let mut d = Decoder::new();
        let mut out = Vec::new();
        for e in events {
            out.extend(
                d.push(&SseEvent {
                    event: e["type"].as_str().map(str::to_owned),
                    data: e.to_string(),
                })
                .unwrap(),
            );
        }
        out.extend(d.end_of_stream().unwrap());
        out
    }

    fn completed(status: &str, extra: Value) -> Value {
        let mut r = json!({ "status": status, "output": [],
            "usage": { "input_tokens": 100, "input_tokens_details": { "cached_tokens": 60 },
                       "output_tokens": 9 } });
        if let (Some(r), Some(extra)) = (r.as_object_mut(), extra.as_object()) {
            r.extend(extra.clone());
        }
        json!({ "type": format!("response.{}", if status == "completed" { "completed" } else { "incomplete" }),
                "response": r })
    }

    #[test]
    fn summary_text_and_tool_call() {
        let ev = run(vec![
            json!({"type":"response.output_item.added","output_index":0,
                   "item":{"id":"rs_1","type":"reasoning","summary":[]}}),
            json!({"type":"response.reasoning_summary_part.added","item_id":"rs_1","output_index":0,"summary_index":0}),
            json!({"type":"response.reasoning_summary_text.delta","item_id":"rs_1","output_index":0,"summary_index":0,"delta":"Look"}),
            json!({"type":"response.reasoning_summary_part.added","item_id":"rs_1","output_index":0,"summary_index":1}),
            json!({"type":"response.reasoning_summary_text.delta","item_id":"rs_1","output_index":0,"summary_index":1,"delta":"Read it"}),
            json!({"type":"response.output_item.done","output_index":0,
                   "item":{"id":"rs_1","type":"reasoning","summary":[{"type":"summary_text","text":"Look"}],"encrypted_content":"ENC"}}),
            json!({"type":"response.output_item.added","output_index":1,
                   "item":{"id":"fc_1","type":"function_call","call_id":"call_A","name":"read_file","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":1,"delta":"{\"path\":"}),
            json!({"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":1,"delta":"\"a\"}"}),
            json!({"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":1,"arguments":"{\"path\":\"a\"}"}),
            json!({"type":"response.output_item.done","output_index":1,
                   "item":{"id":"fc_1","type":"function_call","call_id":"call_A","name":"read_file","arguments":"{\"path\":\"a\"}","status":"completed"}}),
            completed("completed", json!({})),
        ]);
        assert_eq!(
            ev[..4],
            [
                StreamEvent::ThinkingDelta("Look".into()),
                StreamEvent::ThinkingDelta("\n\n".into()),
                StreamEvent::ThinkingDelta("Read it".into()),
                StreamEvent::ThinkingEnd,
            ]
        );
        assert_eq!(
            ev[4],
            StreamEvent::ToolCallStart {
                index: 1,
                id: "call_A".into(),
                name: "read_file".into()
            }
        );
        let args: String = ev
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolCallDelta {
                    arguments_fragment, ..
                } => Some(arguments_fragment.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            args, "{\"path\":\"a\"}",
            "the .done event adds nothing twice"
        );
        assert!(ev.contains(&StreamEvent::Usage(Usage {
            input_tokens: 40,
            output_tokens: 9,
            cache_read_tokens: 60,
            cache_write_tokens: 0,
        })));
        let replay = ev.iter().find_map(|e| match e {
            StreamEvent::Replay(v) => Some(v.clone()),
            _ => None,
        });
        let replay = replay.unwrap();
        assert_eq!(replay[0]["encrypted_content"], "ENC", "from the .done item");
        assert_eq!(replay[1]["call_id"], "call_A");
        assert_eq!(
            ev.last(),
            Some(&StreamEvent::Done {
                stop_reason: StopReason::ToolUse
            })
        );
    }

    #[test]
    fn reasoning_text_then_answer() {
        let ev = run(vec![
            json!({"type":"response.reasoning_text.delta","item_id":"rs_1","output_index":0,"content_index":0,"delta":"hmm"}),
            json!({"type":"response.output_item.done","output_index":0,
                   "item":{"id":"rs_1","type":"reasoning","summary":[],"content":[{"type":"reasoning_text","text":"hmm"}]}}),
            json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":1,"content_index":0,"delta":"Hi!"}),
            json!({"type":"response.output_item.done","output_index":1,
                   "item":{"id":"msg_1","type":"message","role":"assistant","status":"completed","phase":"final_answer",
                           "content":[{"type":"output_text","text":"Hi!","annotations":[]}]}}),
            completed("completed", json!({})),
        ]);
        assert_eq!(ev[0], StreamEvent::ThinkingDelta("hmm".into()));
        assert_eq!(ev[1], StreamEvent::ThinkingEnd);
        assert_eq!(ev[2], StreamEvent::TextDelta("Hi!".into()));
        assert!(
            ev.iter()
                .any(|e| matches!(e, StreamEvent::Replay(v) if v[1]["phase"] == "final_answer"))
        );
        assert_eq!(
            ev.last(),
            Some(&StreamEvent::Done {
                stop_reason: StopReason::EndTurn
            })
        );
    }

    #[test]
    fn stop_reasons() {
        let stop = |events| match run(events).pop() {
            Some(StreamEvent::Done { stop_reason }) => stop_reason,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            stop(vec![completed(
                "incomplete",
                json!({"incomplete_details":{"reason":"max_output_tokens"}})
            )]),
            StopReason::MaxTokens
        );
        assert_eq!(
            stop(vec![completed(
                "incomplete",
                json!({"incomplete_details":{"reason":"content_filter"}})
            )]),
            StopReason::Refusal {
                category: Some("content_filter".into())
            }
        );
        assert_eq!(
            stop(vec![
                json!({"type":"response.refusal.delta","item_id":"m","output_index":0,"content_index":0,"delta":"I can't help with that."}),
                completed("completed", json!({})),
            ]),
            StopReason::Refusal { category: None }
        );
        assert_eq!(
            stop(vec![
                json!({"type":"response.failed","response":{"status":"failed",
                "error":{"code":"bio_policy","message":"blocked"}}})
            ]),
            StopReason::Refusal {
                category: Some("bio_policy".into())
            }
        );
    }

    #[test]
    fn errors_and_dropped_connections() {
        let push = |v: Value| {
            Decoder::new().push(&SseEvent {
                event: None,
                data: v.to_string(),
            })
        };
        assert!(matches!(
            push(
                json!({"type":"error","code":"rate_limit_exceeded","message":"slow","param":null})
            ),
            Err(ProviderError::RateLimited { .. })
        ));
        assert!(matches!(
            push(json!({"type":"response.failed","response":{"status":"failed",
                "error":{"code":"server_error","message":"oops"}}})),
            Err(ProviderError::Server { status: 500, message }) if message.contains("oops")
        ));
        let mut d = Decoder::new();
        d.push(&SseEvent {
            event: None,
            data: json!({"type":"response.output_text.delta","delta":"cut"}).to_string(),
        })
        .unwrap();
        assert_eq!(d.end_of_stream().unwrap_err(), ProviderError::Incomplete);
    }

    #[test]
    fn calls_without_streamed_arguments() {
        // Some servers send the finished call only.
        let ev = run(vec![
            json!({"type":"response.output_item.done","output_index":0,
                   "item":{"id":"fc_1","type":"function_call","call_id":"c","name":"t","arguments":"{}"}}),
            completed("completed", json!({})),
        ]);
        assert!(matches!(&ev[0], StreamEvent::ToolCallStart { id, .. } if id == "c"));
        assert_eq!(
            ev[1],
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments_fragment: "{}".into()
            }
        );
    }
}
