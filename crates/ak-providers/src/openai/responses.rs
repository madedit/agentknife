//! Neutral [`ChatRequest`] -> Responses API request body (`POST /responses`),
//! and the OpenAI model capability table.
//!
//! Requests are stateless (`store: false`). Reasoning comes back encrypted and is
//! replayed verbatim: the replay of an assistant turn is the response's whole
//! `output` array (reasoning, message and function_call items, in order).

use ak_types::chat::{ContentPart, Message};
use serde_json::{Value, json};

use crate::{Capabilities, ChatRequest, Effort};

/// OpenAI models AgentKnife knows, from developers.openai.com/api/docs/models,
/// read 2026-10-20. Update the date with the table.
pub const MODEL_TABLE_DATE: &str = "2026-10-20";

pub struct ModelSpec {
    pub id: &'static str,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub vision: bool,
    /// Supported `reasoning.effort` levels (AgentKnife's subset).
    pub efforts: &'static [Effort],
}

const LOW_TO_MAX: &[Effort] = &[
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::XHigh,
    Effort::Max,
];

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "gpt-6-astra",
        context_window: 1_050_000,
        max_output_tokens: 128_000,
        vision: true,
        efforts: LOW_TO_MAX,
    },
    ModelSpec {
        id: "gpt-6.1-sol",
        context_window: 1_050_000,
        max_output_tokens: 128_000,
        vision: true,
        efforts: LOW_TO_MAX,
    },
    ModelSpec {
        id: "gpt-6-luna",
        context_window: 1_050_000,
        max_output_tokens: 128_000,
        vision: true,
        efforts: LOW_TO_MAX,
    },
];

/// The table entry for `model`, also for dated snapshots (`gpt-6-astra-2026-…`).
pub fn spec(model: &str) -> Option<&'static ModelSpec> {
    MODELS
        .iter()
        .filter(|m| model == m.id || model.starts_with(&format!("{}-", m.id)))
        .max_by_key(|m| m.id.len())
}

pub fn capabilities(model: &str) -> Capabilities {
    match spec(model) {
        Some(m) => Capabilities {
            tool_calling: true,
            vision: m.vision,
            thinking: true,
            context_window: Some(m.context_window),
            max_output_tokens: Some(m.max_output_tokens),
        },
        // Unknown (newer) model: Responses supports tools for every current model.
        None => Capabilities {
            tool_calling: true,
            ..Capabilities::default()
        },
    }
}

/// The highest supported level at or below `wanted`; the lowest one otherwise.
fn effort_for(model: &str, wanted: Effort) -> Effort {
    match spec(model) {
        Some(m) => m
            .efforts
            .iter()
            .copied()
            .filter(|e| *e <= wanted)
            .max()
            .or_else(|| m.efforts.iter().copied().min())
            .unwrap_or(wanted),
        None => wanted,
    }
}

/// `origin` is the provider id: replays from this provider are echoed verbatim.
pub fn build(req: &ChatRequest, origin: &str) -> Value {
    let max_tokens = match spec(&req.model) {
        Some(m) => req.max_tokens.min(m.max_output_tokens),
        None => req.max_tokens,
    };
    let mut body = json!({
        "model": req.model,
        "input": input(&req.messages, origin),
        "stream": true,
        "store": false,
        "include": ["reasoning.encrypted_content"],
        "max_output_tokens": max_tokens,
    });
    let Some(obj) = body.as_object_mut() else {
        return body;
    };
    if let Some(system) = req.system.as_ref().filter(|s| !s.is_empty()) {
        obj.insert("instructions".into(), json!(system));
    }
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                    // Our schemas have optional fields; strict mode would make them required.
                    "strict": false,
                })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
    }
    let mut reasoning = json!({ "summary": "auto" });
    if let Some(effort) = req.effort {
        reasoning["effort"] = json!(effort_for(&req.model, effort).as_str());
    }
    obj.insert("reasoning".into(), reasoning);
    body
}

fn input(history: &[Message], origin: &str) -> Vec<Value> {
    let mut items = Vec::new();
    for msg in history {
        match msg {
            Message::User { content } => {
                items.push(json!({ "role": "user", "content": user_content(content) }));
            }
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => items.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": tool_output(content, *is_error),
            })),
            Message::Assistant {
                text,
                tool_calls,
                replay,
                ..
            } => match replay {
                // Our own output items, verbatim and in order (encrypted reasoning,
                // message ids and phases included).
                Some(r) if r.origin == origin && r.content.is_array() => {
                    items.extend(r.content.as_array().into_iter().flatten().cloned());
                }
                _ => {
                    if !text.is_empty() {
                        items.push(json!({ "role": "assistant", "content": text }));
                    }
                    for c in tool_calls {
                        let args = if c.arguments.is_object() {
                            c.arguments.to_string()
                        } else {
                            "{}".to_owned()
                        };
                        items.push(json!({
                            "type": "function_call",
                            "call_id": c.id,
                            "name": c.name,
                            "arguments": args,
                        }));
                    }
                }
            },
        }
    }
    items
}

fn part(p: &ContentPart) -> Value {
    match p {
        ContentPart::Text { text } => json!({ "type": "input_text", "text": text }),
        ContentPart::Image {
            media_type,
            data_base64,
        } => json!({
            "type": "input_image",
            "image_url": format!("data:{media_type};base64,{data_base64}"),
            "detail": "auto",
        }),
    }
}

fn all_text(parts: &[ContentPart]) -> Option<Vec<&str>> {
    parts
        .iter()
        .map(|p| match p {
            ContentPart::Text { text } => Some(text.as_str()),
            ContentPart::Image { .. } => None,
        })
        .collect()
}

/// Text-only content goes as a plain string, which every server accepts.
fn user_content(parts: &[ContentPart]) -> Value {
    match all_text(parts) {
        Some(texts) => json!(texts.join("\n\n")),
        None => Value::Array(parts.iter().map(part).collect()),
    }
}

/// Function output may hold images directly (screenshots for vision models).
fn tool_output(parts: &[ContentPart], is_error: bool) -> Value {
    let prefix = if is_error { "Error: " } else { "" };
    match all_text(parts) {
        Some(texts) => json!(format!("{prefix}{}", texts.join("\n"))),
        None => {
            let mut out: Vec<Value> = Vec::new();
            if is_error {
                out.push(json!({ "type": "input_text", "text": "Error:" }));
            }
            out.extend(parts.iter().map(part));
            Value::Array(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use ak_types::chat::{ProviderReplay, ToolCall, ToolSpec};

    use super::*;

    fn req(model: &str, messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            system: Some("sys".into()),
            messages,
            tools: vec![ToolSpec {
                name: "read_file".into(),
                description: "Read".into(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 200_000,
            effort: Some(Effort::High),
        }
    }

    #[test]
    fn body_shape() {
        let b = build(
            &req("gpt-6-astra", vec![Message::user_text("hi")]),
            "openai",
        );
        assert_eq!(b["instructions"], "sys");
        assert_eq!(b["store"], false);
        assert_eq!(b["include"][0], "reasoning.encrypted_content");
        assert_eq!(b["max_output_tokens"], 128_000, "clamped to the model");
        assert_eq!(
            b["reasoning"],
            json!({ "summary": "auto", "effort": "high" })
        );
        assert_eq!(b["input"][0], json!({ "role": "user", "content": "hi" }));
        assert_eq!(b["tools"][0]["name"], "read_file");
        assert_eq!(b["tools"][0]["parameters"]["type"], "object");
        assert!(b["tools"][0].get("function").is_none(), "flat tool shape");
    }

    #[test]
    fn capability_table_and_snapshots() {
        let c = capabilities("gpt-6.1-sol");
        assert!(c.tool_calling && c.vision && c.thinking);
        assert_eq!(c.context_window, Some(1_050_000));
        assert!(spec("gpt-6-astra-2026-09-01").is_some());
        assert!(spec("gpt-6-astral").is_none());
        let unknown = capabilities("gpt-7");
        assert!(unknown.tool_calling && !unknown.vision);
        // Unknown models get the effort as asked.
        assert_eq!(effort_for("gpt-7", Effort::XHigh), Effort::XHigh);
    }

    #[test]
    fn tool_output_keeps_images() {
        let msgs = vec![
            Message::ToolResult {
                call_id: "c1".into(),
                content: vec![
                    ContentPart::Text {
                        text: "screenshot".into(),
                    },
                    ContentPart::Image {
                        media_type: "image/png".into(),
                        data_base64: "AAAA".into(),
                    },
                ],
                is_error: false,
            },
            Message::ToolResult {
                call_id: "c2".into(),
                content: vec![ContentPart::Text {
                    text: "boom".into(),
                }],
                is_error: true,
            },
        ];
        let items = input(&msgs, "openai");
        assert_eq!(items[0]["type"], "function_call_output");
        assert_eq!(items[0]["output"][1]["type"], "input_image");
        assert_eq!(
            items[0]["output"][1]["image_url"],
            "data:image/png;base64,AAAA"
        );
        assert_eq!(items[1]["output"], "Error: boom");
    }

    #[test]
    fn own_replay_is_verbatim_others_are_rebuilt() {
        let own = json!([
            { "id": "rs_1", "type": "reasoning", "summary": [], "encrypted_content": "enc" },
            { "id": "fc_1", "type": "function_call", "call_id": "call_1",
              "name": "read_file", "arguments": "{\"path\":\"a\"}", "status": "completed" }
        ]);
        let msgs = vec![
            Message::Assistant {
                thinking: vec![],
                text: String::new(),
                tool_calls: vec![],
                replay: Some(ProviderReplay {
                    origin: "openai".into(),
                    content: own.clone(),
                }),
            },
            Message::Assistant {
                thinking: vec![],
                text: "Hi".into(),
                tool_calls: vec![ToolCall {
                    id: "toolu_9".into(),
                    name: "read_file".into(),
                    arguments: json!({ "path": "b" }),
                    invalid_arguments: None,
                }],
                // Chat Completions replay (an object) or another provider: rebuild.
                replay: Some(ProviderReplay {
                    origin: "anthropic".into(),
                    content: json!([{ "type": "thinking" }]),
                }),
            },
        ];
        let items = input(&msgs, "openai");
        assert_eq!(items[0], own[0]);
        assert_eq!(items[1], own[1]);
        assert_eq!(items[2], json!({ "role": "assistant", "content": "Hi" }));
        assert_eq!(
            items[3],
            json!({ "type": "function_call", "call_id": "toolu_9",
                    "name": "read_file", "arguments": "{\"path\":\"b\"}" })
        );
    }
}
