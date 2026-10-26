//! Neutral [`ChatRequest`] -> Chat Completions request body.

use ak_types::chat::{ContentPart, Message};
use serde_json::{Value, json};

use super::Flavor;
use crate::ChatRequest;

pub fn build(req: &ChatRequest, flavor: Flavor) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = req.system.as_ref().filter(|s| !s.is_empty()) {
        messages.push(json!({ "role": "system", "content": system }));
    }
    convert(&req.messages, flavor, &mut messages);

    let mut body = json!({
        "model": req.model,
        "stream": true,
        "stream_options": { "include_usage": true },
        "messages": messages,
    });
    let Some(obj) = body.as_object_mut() else {
        return body;
    };
    // OpenAI deprecated `max_tokens`; Ollama only understands `max_tokens`.
    let max_field = match flavor {
        Flavor::OpenAi => "max_completion_tokens",
        _ => "max_tokens",
    };
    obj.insert(max_field.into(), json!(req.max_tokens));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    },
                })
            })
            .collect();
        obj.insert("tools".into(), Value::Array(tools));
    }
    if let Some(effort) = req.effort {
        match flavor {
            Flavor::OpenAi | Flavor::Ollama => {
                obj.insert("reasoning_effort".into(), json!(effort.as_str()));
            }
            Flavor::OpenRouter => {
                obj.insert("reasoning".into(), json!({ "effort": effort.as_str() }));
            }
            // Gemini knows minimal, low, medium and high.
            Flavor::Gemini => {
                let effort = match effort.as_str() {
                    "xhigh" | "max" => "high",
                    e => e,
                };
                obj.insert("reasoning_effort".into(), json!(effort));
            }
            // Support unverified: sending unknown fields can fail strict servers.
            Flavor::LmStudio | Flavor::Generic => {}
        }
    }
    body
}

/// Converts the neutral history. Tool messages cannot carry images, so images from a
/// run of tool results follow in one user message.
fn convert(history: &[Message], flavor: Flavor, out: &mut Vec<Value>) {
    let mut pending_images: Vec<Value> = Vec::new();
    for msg in history {
        if !matches!(msg, Message::ToolResult { .. }) {
            flush_images(&mut pending_images, out);
        }
        match msg {
            Message::User { content } => {
                out.push(json!({ "role": "user", "content": user_content(content) }))
            }
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => {
                let mut text: Vec<&str> = content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        ContentPart::Image { .. } => None,
                    })
                    .collect();
                let images: Vec<Value> = content.iter().filter_map(image_part).collect();
                if !images.is_empty() {
                    text.push("(image attached in the next message)");
                    pending_images.extend(images);
                }
                let mut text = text.join("\n");
                if *is_error {
                    text = format!("Error: {text}");
                }
                out.push(json!({ "role": "tool", "tool_call_id": call_id, "content": text }));
            }
            Message::Assistant {
                text,
                tool_calls,
                replay,
                ..
            } => match replay {
                // Our own output: echo it unchanged (keeps OpenRouter reasoning_details
                // and Ollama reasoning in place).
                Some(r) if r.origin == flavor.id() && r.content.is_object() => {
                    out.push(r.content.clone());
                }
                _ => {
                    let mut m = json!({
                        "role": "assistant",
                        "content": if text.is_empty() { Value::Null } else { json!(text) },
                    });
                    if !tool_calls.is_empty()
                        && let Some(o) = m.as_object_mut()
                    {
                        let calls: Vec<Value> = tool_calls
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                let args = if c.arguments.is_object() {
                                    c.arguments.to_string()
                                } else {
                                    "{}".to_owned()
                                };
                                let mut call = json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": { "name": c.name, "arguments": args },
                                });
                                // Gemini 3 refuses a step's first call without a thought
                                // signature; calls from another AI have none, and this
                                // placeholder is what Google documents for them.
                                if flavor == Flavor::Gemini && i == 0 {
                                    call["extra_content"] = json!({ "google": {
                                        "thought_signature": GEMINI_NO_SIGNATURE } });
                                }
                                call
                            })
                            .collect();
                        o.insert("tool_calls".into(), Value::Array(calls));
                    }
                    out.push(m);
                }
            },
        }
    }
    flush_images(&mut pending_images, out);
}

/// Gemini's placeholder thought signature for calls it did not make.
const GEMINI_NO_SIGNATURE: &str = "skip_thought_signature_validator";

fn flush_images(images: &mut Vec<Value>, out: &mut Vec<Value>) {
    if images.is_empty() {
        return;
    }
    let mut content =
        vec![json!({ "type": "text", "text": "Images from the tool results above:" })];
    content.append(images);
    out.push(json!({ "role": "user", "content": content }));
}

/// Text-only content is sent as a plain string, which every server accepts.
fn user_content(parts: &[ContentPart]) -> Value {
    if parts.iter().all(|p| matches!(p, ContentPart::Text { .. })) {
        let text: Vec<&str> = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect();
        return json!(text.join("\n\n"));
    }
    Value::Array(
        parts
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => json!({ "type": "text", "text": text }),
                ContentPart::Image { .. } => image_part(p).unwrap_or(Value::Null),
            })
            .collect(),
    )
}

fn image_part(p: &ContentPart) -> Option<Value> {
    match p {
        ContentPart::Image {
            media_type,
            data_base64,
        } => Some(json!({
            "type": "image_url",
            "image_url": { "url": format!("data:{media_type};base64,{data_base64}") },
        })),
        ContentPart::Text { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use ak_types::chat::{ProviderReplay, ToolCall, ToolSpec};

    use super::*;
    use crate::Effort;

    fn req(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            system: Some("sys".into()),
            messages,
            tools: vec![ToolSpec {
                name: "read_file".into(),
                description: "Read".into(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: 4096,
            effort: Some(Effort::High),
        }
    }

    #[test]
    fn flavor_specific_fields() {
        let r = req(vec![Message::user_text("hi")]);
        let openai = build(&r, Flavor::OpenAi);
        assert_eq!(openai["max_completion_tokens"], 4096);
        assert_eq!(openai["reasoning_effort"], "high");
        assert_eq!(
            openai["messages"][0],
            json!({ "role": "system", "content": "sys" })
        );
        assert_eq!(openai["messages"][1]["content"], "hi");
        assert_eq!(
            openai["tools"][0]["function"]["parameters"]["type"],
            "object"
        );

        let ollama = build(&r, Flavor::Ollama);
        assert_eq!(ollama["max_tokens"], 4096);
        assert!(ollama.get("max_completion_tokens").is_none());

        assert_eq!(build(&r, Flavor::OpenRouter)["reasoning"]["effort"], "high");
        let generic = build(&r, Flavor::Generic);
        assert!(generic.get("reasoning_effort").is_none() && generic.get("reasoning").is_none());
    }

    #[test]
    fn tool_result_images_move_to_a_user_message() {
        let img = ContentPart::Image {
            media_type: "image/png".into(),
            data_base64: "AAAA".into(),
        };
        let history = vec![
            Message::ToolResult {
                call_id: "a".into(),
                content: vec![
                    ContentPart::Text {
                        text: "screenshot".into(),
                    },
                    img,
                ],
                is_error: false,
            },
            Message::ToolResult {
                call_id: "b".into(),
                content: vec![ContentPart::Text {
                    text: "boom".into(),
                }],
                is_error: true,
            },
        ];
        let mut out = Vec::new();
        convert(&history, Flavor::OpenAi, &mut out);
        assert_eq!(out.len(), 3, "two tool messages, then one user message");
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[1]["content"], "Error: boom");
        assert_eq!(out[2]["role"], "user");
        assert_eq!(
            out[2]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
    }

    #[test]
    fn assistant_replay_vs_rebuild() {
        let own = json!({ "role": "assistant", "content": null, "reasoning": "hmm",
                          "tool_calls": [{ "id": "c1", "type": "function",
                                           "function": { "name": "read_file", "arguments": "{\"path\":\"a\"}" } }] });
        let msgs = vec![
            Message::Assistant {
                thinking: vec![],
                text: String::new(),
                tool_calls: vec![],
                replay: Some(ProviderReplay {
                    origin: "ollama".into(),
                    content: own.clone(),
                }),
            },
            Message::Assistant {
                thinking: vec![],
                text: "Hi".into(),
                tool_calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "read_file".into(),
                    arguments: json!({ "path": "b" }),
                    invalid_arguments: None,
                }],
                replay: Some(ProviderReplay {
                    origin: "anthropic".into(),
                    content: json!([]),
                }),
            },
        ];
        let mut out = Vec::new();
        convert(&msgs, Flavor::Ollama, &mut out);
        assert_eq!(out[0], own);
        assert_eq!(out[1]["content"], "Hi");
        assert_eq!(
            out[1]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"b\"}"
        );
        assert!(out[1]["tool_calls"][0].get("extra_content").is_none());

        // Gemini: another AI's first call in a step gets the placeholder signature.
        let mut out = Vec::new();
        convert(&msgs[1..], Flavor::Gemini, &mut out);
        assert_eq!(
            out[0]["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
            GEMINI_NO_SIGNATURE
        );
    }
}
