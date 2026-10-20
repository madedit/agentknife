//! Switching providers mid-conversation (AGENTS.md §4): history produced by one
//! adapter must continue on another.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::fixture::{Fixture, FixtureTransport, RecordedRequest, RecordedResponse};
use ak_providers::openai::{Flavor, OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{ChatRequest, Effort, Provider, collect};
use ak_types::chat::{ContentPart, Message, ToolSpec};
use serde_json::json;

fn tool() -> ToolSpec {
    ToolSpec {
        name: "read_file".into(),
        description: "Read a project file".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
        }),
    }
}

#[tokio::test]
async fn anthropic_history_continues_on_ollama() {
    // 1. First turn on Anthropic (synthetic fixture: thinking + text + tool call).
    let mut anthropic_fx = Fixture::load(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/anthropic/thinking_tool_use.json"),
    )
    .unwrap();
    anthropic_fx.truncate(2); // models lookup + first message
    let anthropic = Anthropic::new(
        AnthropicConfig::new("sk-ant-test"),
        Arc::new(FixtureTransport::new(anthropic_fx)),
    );
    let mut req = ChatRequest {
        model: "claude-opus-5-5".into(),
        system: Some("You are AgentKnife.".into()),
        messages: vec![Message::user_text("What is in src/app.rs?")],
        tools: vec![tool()],
        max_tokens: 64_000,
        effort: Some(Effort::High),
    };
    let first = collect(anthropic.stream(req.clone()).await.unwrap(), anthropic.id())
        .await
        .unwrap();
    req.messages.push(first.message);
    req.messages.push(Message::ToolResult {
        call_id: "toolu_01".into(),
        content: vec![ContentPart::Text {
            text: "fn main() {}".into(),
        }],
        is_error: false,
    });

    // 2. Continue on Ollama. Anthropic's thinking and raw blocks must not leak;
    //    text and tool calls are rebuilt in Chat Completions form.
    let expected_body = json!({
        "model": "qwen3:8b",
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": 64000,
        "reasoning_effort": "high",
        "tools": [{ "type": "function", "function": {
            "name": "read_file", "description": "Read a project file",
            "parameters": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] } } }],
        "messages": [
            { "role": "system", "content": "You are AgentKnife." },
            { "role": "user", "content": "What is in src/app.rs?" },
            { "role": "assistant", "content": "Let me read it.", "tool_calls": [{
                "id": "toolu_01", "type": "function",
                "function": { "name": "read_file", "arguments": "{\"path\":\"src/app.rs\"}" } }] },
            { "role": "tool", "tool_call_id": "toolu_01", "content": "fn main() {}" },
        ],
    });
    let ollama_fx = vec![Fixture {
        source: Some("synthetic".into()),
        request: RecordedRequest {
            method: "POST".into(),
            path: "/v1/chat/completions".into(),
            body: Some(expected_body),
        },
        response: RecordedResponse {
            status: 200,
            headers: vec![],
            chunks: vec![
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"An empty main.\"},\"finish_reason\":\"stop\"}]}\n\n".into(),
                "data: [DONE]\n\n".into(),
            ],
        },
    }];
    let transport = Arc::new(FixtureTransport::new(ollama_fx));
    let ollama = OpenAiCompat::new(
        OpenAiCompatConfig::for_flavor(Flavor::Ollama, None),
        transport.clone(),
    );
    req.model = "qwen3:8b".into();
    let second = collect(ollama.stream(req).await.unwrap(), ollama.id())
        .await
        .unwrap();
    match second.message {
        Message::Assistant { text, .. } => assert_eq!(text, "An empty main."),
        other => panic!("{other:?}"),
    }
    assert_eq!(transport.remaining(), 0);
}
