//! Neutral [`ChatRequest`] -> Messages API request body.

use ak_types::chat::{ContentPart, Message};
use serde_json::{Value, json};

use super::{ID, ModelMeta};
use crate::{ChatRequest, Effort};

/// Beta that lets `thinking.block_binding` drop thinking blocks invalidated by a
/// history edit instead of failing the request with a 400.
pub const BETA_THINKING_BINDING: &str = "thinking-binding-controls-2026-08-01";
/// Beta for `fallbacks: "default"` (server-side retry on a refusal).
pub const BETA_SERVER_FALLBACK: &str = "server-side-fallback-2026-07-01";

/// Models that accept `fallbacks: "default"` on the Claude API.
const DEFAULT_FALLBACK_MODELS: &[&str] = &[
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

pub struct Built {
    pub body: Value,
    pub betas: Vec<&'static str>,
}

/// `server_fallbacks` is only true when talking to the Claude API itself.
pub fn build(req: &ChatRequest, meta: &ModelMeta, server_fallbacks: bool) -> Built {
    let mut betas = Vec::new();
    let max_tokens = meta
        .max_tokens
        .map_or(req.max_tokens, |m| req.max_tokens.min(m));
    let mut body = json!({
        "model": req.model,
        "max_tokens": max_tokens,
        "stream": true,
        // Automatic prompt caching on the last cacheable block: agent loops resend
        // the whole (append-only) history every turn.
        "cache_control": { "type": "ephemeral" },
        "messages": messages(&req.messages),
    });
    let obj = body.as_object_mut();
    let Some(obj) = obj else {
        return Built { body, betas };
    };
    if let Some(system) = req.system.as_ref().filter(|s| !s.is_empty()) {
        obj.insert("system".into(), json!(system));
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                    // Large tool inputs (file contents) stream as generated. The
                    // assembler marks cut-off JSON invalid instead of failing.
                    "eager_input_streaming": true,
                })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
    }

    if meta.adaptive_thinking {
        obj.insert(
            "thinking".into(),
            json!({
                "type": "adaptive",
                "display": "summarized",
                "block_binding": { "prefix_mismatch_behavior": "drop_block" },
            }),
        );
        betas.push(BETA_THINKING_BINDING);
    } else if meta.budget_thinking
        && let Some(budget) = req.effort.and_then(|e| budget_for(e, max_tokens))
    {
        obj.insert(
            "thinking".into(),
            json!({
                "type": "enabled",
                "budget_tokens": budget,
                "block_binding": { "prefix_mismatch_behavior": "drop_block" },
            }),
        );
        betas.push(BETA_THINKING_BINDING);
    }
    if let Some(effort) = req.effort.and_then(|e| meta.effort_at_most(e)) {
        obj.insert("output_config".into(), json!({ "effort": effort.as_str() }));
    }
    if server_fallbacks && DEFAULT_FALLBACK_MODELS.contains(&req.model.as_str()) {
        obj.insert("fallbacks".into(), json!("default"));
        betas.push(BETA_SERVER_FALLBACK);
    }
    Built { body, betas }
}

/// Thinking budget for models that predate adaptive thinking. Must be >= 1024 and
/// below `max_tokens`.
fn budget_for(effort: Effort, max_tokens: u32) -> Option<u32> {
    let wanted = match effort {
        Effort::Low => 2_048,
        Effort::Medium => 8_192,
        Effort::High => 16_000,
        Effort::XHigh | Effort::Max => 32_000,
    };
    let budget = wanted.min(max_tokens.saturating_sub(1));
    (budget >= 1_024).then_some(budget)
}

/// Converts the neutral history. Consecutive same-role messages (parallel tool
/// results, a user note after them) are merged, because roles must alternate.
fn messages(history: &[Message]) -> Vec<Value> {
    let mut out: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for msg in history {
        let (role, blocks) = match msg {
            Message::User { content } => ("user", content.iter().map(part).collect()),
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => {
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": content.iter().map(part).collect::<Vec<_>>(),
                });
                if *is_error && let Some(o) = block.as_object_mut() {
                    o.insert("is_error".into(), json!(true));
                }
                ("user", vec![block])
            }
            Message::Assistant {
                text,
                tool_calls,
                replay,
                ..
            } => {
                let blocks = match replay {
                    // Our own output: echo it unchanged, so signed thinking and block
                    // order survive. The API drops blocks the current model can't read.
                    Some(r) if r.origin == ID => r.content.as_array().cloned().unwrap_or_default(),
                    // Another provider's output: rebuild from neutral fields. Its
                    // reasoning cannot be read here and is left out.
                    _ => {
                        let mut blocks = Vec::new();
                        if !text.is_empty() {
                            blocks.push(json!({ "type": "text", "text": text }));
                        }
                        for call in tool_calls {
                            let input = if call.arguments.is_object() {
                                call.arguments.clone()
                            } else {
                                json!({})
                            };
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": call.id,
                                "name": call.name,
                                "input": input,
                            }));
                        }
                        blocks
                    }
                };
                ("assistant", blocks)
            }
        };
        match out.last_mut() {
            Some((last_role, last)) if *last_role == role => last.extend(blocks),
            _ => out.push((role, blocks)),
        }
    }
    out.into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect()
}

fn part(p: &ContentPart) -> Value {
    match p {
        ContentPart::Text { text } => json!({ "type": "text", "text": text }),
        ContentPart::Image {
            media_type,
            data_base64,
        } => json!({
            "type": "image",
            "source": { "type": "base64", "media_type": media_type, "data": data_base64 },
        }),
    }
}

#[cfg(test)]
mod tests {
    use ak_types::chat::{ProviderReplay, ToolCall, ToolSpec};

    use super::*;

    fn meta() -> ModelMeta {
        ModelMeta {
            max_tokens: Some(128_000),
            adaptive_thinking: true,
            effort_levels: vec![Effort::Low, Effort::Medium, Effort::High, Effort::Max],
            ..ModelMeta::default()
        }
    }

    fn request(model: &str, messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            system: Some("You are AgentKnife.".into()),
            messages,
            tools: vec![ToolSpec {
                name: "read_file".into(),
                description: "Read a file".into(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 200_000,
            effort: Some(Effort::XHigh),
        }
    }

    #[test]
    fn adaptive_model_request_shape() {
        let b = build(
            &request("claude-opus-5-5", vec![Message::user_text("hi")]),
            &meta(),
            true,
        );
        assert_eq!(b.body["max_tokens"], 128_000, "clamped to model limit");
        assert_eq!(b.body["thinking"]["type"], "adaptive");
        assert_eq!(b.body["thinking"]["display"], "summarized");
        // xhigh is unsupported here, so it steps down to high.
        assert_eq!(b.body["output_config"]["effort"], "high");
        assert_eq!(b.body["tools"][0]["eager_input_streaming"], true);
        assert_eq!(b.body["fallbacks"], "default");
        assert_eq!(b.body["cache_control"]["type"], "ephemeral");
        assert_eq!(b.betas, [BETA_THINKING_BINDING, BETA_SERVER_FALLBACK]);
    }

    #[test]
    fn no_fallbacks_off_the_claude_api_or_for_other_models() {
        let r = request("claude-opus-5-5", vec![Message::user_text("hi")]);
        assert!(build(&r, &meta(), false).body.get("fallbacks").is_none());
        let r = request("claude-haiku-5-5", vec![Message::user_text("hi")]);
        assert!(build(&r, &meta(), true).body.get("fallbacks").is_none());
    }

    #[test]
    fn budget_only_models_get_a_budget_and_no_effort() {
        let m = ModelMeta {
            max_tokens: Some(64_000),
            budget_thinking: true,
            ..ModelMeta::default()
        };
        let b = build(
            &request("claude-haiku-4-5", vec![Message::user_text("hi")]),
            &m,
            true,
        );
        assert_eq!(b.body["thinking"]["type"], "enabled");
        assert_eq!(b.body["thinking"]["budget_tokens"], 32_000);
        assert!(b.body.get("output_config").is_none());
    }

    #[test]
    fn own_replay_is_echoed_verbatim_and_foreign_is_rebuilt() {
        let raw = json!([
            { "type": "thinking", "thinking": "", "signature": "sig" },
            { "type": "tool_use", "id": "t1", "name": "read_file", "input": { "path": "a" } },
        ]);
        let ours = Message::Assistant {
            thinking: vec![],
            text: String::new(),
            tool_calls: vec![],
            replay: Some(ProviderReplay {
                origin: ID.into(),
                content: raw.clone(),
            }),
        };
        let foreign = Message::Assistant {
            thinking: vec![],
            text: "Done.".into(),
            tool_calls: vec![ToolCall {
                id: "c9".into(),
                name: "read_file".into(),
                arguments: serde_json::Value::Null,
                invalid_arguments: Some("{cut".into()),
            }],
            replay: Some(ProviderReplay {
                origin: "openai-compatible".into(),
                content: json!({ "anything": true }),
            }),
        };
        let msgs = messages(&[
            Message::user_text("go"),
            ours,
            Message::user_text("next"),
            foreign,
        ]);
        assert_eq!(msgs[1]["content"], raw);
        assert_eq!(
            msgs[3]["content"][0],
            json!({ "type": "text", "text": "Done." })
        );
        assert_eq!(msgs[3]["content"][1]["input"], json!({}));
    }

    #[test]
    fn tool_results_and_follow_up_text_merge_into_one_user_turn() {
        let img = ContentPart::Image {
            media_type: "image/png".into(),
            data_base64: "AAAA".into(),
        };
        let msgs = messages(&[
            Message::ToolResult {
                call_id: "a".into(),
                content: vec![img],
                is_error: false,
            },
            Message::ToolResult {
                call_id: "b".into(),
                content: vec![ContentPart::Text {
                    text: "no such file".into(),
                }],
                is_error: true,
            },
            Message::user_text("also make it blue"),
        ]);
        assert_eq!(msgs.len(), 1);
        let c = &msgs[0]["content"];
        assert_eq!(c[0]["content"][0]["source"]["type"], "base64");
        assert_eq!(c[1]["is_error"], true);
        assert!(c[0].get("is_error").is_none());
        assert_eq!(c[2]["text"], "also make it blue");
    }

    #[test]
    fn budget_respects_max_tokens() {
        assert_eq!(budget_for(Effort::High, 4_096), Some(4_095));
        assert_eq!(budget_for(Effort::Low, 1_000), None);
    }
}
