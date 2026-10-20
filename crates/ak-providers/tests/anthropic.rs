//! Anthropic provider against fixtures (tests/fixtures/anthropic). No network.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::fixture::{Fixture, FixtureTransport};
use ak_providers::{
    ChatRequest, Effort, Provider, ProviderError, StopReason, StreamEvent, collect,
};
use ak_types::chat::{ContentPart, Message, ToolSpec};
use futures_util::TryStreamExt;
use serde_json::json;

/// `None` replays recorded chunks; `Some(n)` re-splits the body into n-byte chunks.
const CHUNKINGS: [Option<usize>; 4] = [None, Some(1), Some(3), Some(64)];

fn fixtures(name: &str) -> Vec<Fixture> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/anthropic")
        .join(format!("{name}.json"));
    Fixture::load(path).unwrap()
}

fn provider(name: &str, chunking: Option<usize>) -> (Anthropic, Arc<FixtureTransport>) {
    let mut t = FixtureTransport::new(fixtures(name));
    if let Some(n) = chunking {
        t = t.rechunked(n);
    }
    let t = Arc::new(t);
    (
        Anthropic::new(AnthropicConfig::new("sk-ant-test"), t.clone()),
        t,
    )
}

fn request(model: &str, messages: Vec<Message>) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        system: None,
        messages,
        tools: vec![],
        max_tokens: 64_000,
        effort: None,
    }
}

fn read_file_tool() -> ToolSpec {
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

fn assistant_parts(m: &Message) -> (&str, usize, usize) {
    match m {
        Message::Assistant {
            text,
            thinking,
            tool_calls,
            ..
        } => (text.as_str(), thinking.len(), tool_calls.len()),
        other => panic!("expected assistant message, got {other:?}"),
    }
}

#[tokio::test]
async fn streams_text_with_usage() {
    for chunking in CHUNKINGS {
        let (p, t) = provider("text", chunking);
        let stream = p
            .stream(request(
                "claude-opus-5-5",
                vec![Message::user_text("Say hi")],
            ))
            .await
            .unwrap();
        let done = collect(stream, p.id()).await.unwrap();
        assert_eq!(
            assistant_parts(&done.message).0,
            "Hi there! 你好",
            "{chunking:?}"
        );
        assert_eq!(done.stop_reason, StopReason::EndTurn);
        assert_eq!((done.usage.input_tokens, done.usage.output_tokens), (12, 9));
        assert_eq!(t.remaining(), 0);
    }
}

#[tokio::test]
async fn sends_api_version_key_and_betas() {
    let (p, t) = provider("text", None);
    let s = p
        .stream(request(
            "claude-opus-5-5",
            vec![Message::user_text("Say hi")],
        ))
        .await
        .unwrap();
    collect(s, p.id()).await.unwrap();
    let post = &t.sent()[1];
    let header = |name: &str| {
        post.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(header("x-api-key"), Some("sk-ant-test"));
    assert_eq!(
        header("anthropic-beta"),
        Some("thinking-binding-controls-2026-08-01,server-side-fallback-2026-07-01")
    );
}

#[tokio::test]
async fn thinking_and_tool_use_round_trip_replays_blocks_verbatim() {
    for chunking in CHUNKINGS {
        let (p, t) = provider("thinking_tool_use", chunking);
        let mut req = request(
            "claude-opus-5-5",
            vec![Message::user_text("What is in src/app.rs?")],
        );
        req.system = Some("You are AgentKnife.".into());
        req.tools = vec![read_file_tool()];
        req.effort = Some(Effort::High);

        // Live events arrive in order: thinking, text, tool call.
        let events: Vec<StreamEvent> = p
            .stream(req.clone())
            .await
            .unwrap()
            .try_collect()
            .await
            .unwrap();
        let kinds: Vec<&str> = events
            .iter()
            .map(|e| match e {
                StreamEvent::ThinkingDelta(_) => "thinking",
                StreamEvent::ThinkingEnd => "thinking_end",
                StreamEvent::TextDelta(_) => "text",
                StreamEvent::ToolCallStart { .. } => "tool_start",
                StreamEvent::ToolCallDelta { .. } => "tool_args",
                StreamEvent::Usage(_) => "usage",
                StreamEvent::Replay(_) => "replay",
                StreamEvent::Done { .. } => "done",
                StreamEvent::Fallback { .. } => "fallback",
            })
            .filter(|k| *k != "usage")
            .collect();
        assert_eq!(kinds.first(), Some(&"thinking"));
        assert_eq!(kinds.last(), Some(&"done"));

        let mut asm = ak_providers::MessageAssembler::new(p.id());
        for e in events {
            asm.push(e).unwrap();
        }
        let first = asm.finish().unwrap();
        assert_eq!(first.stop_reason, StopReason::ToolUse);
        let Message::Assistant {
            thinking,
            text,
            tool_calls,
            ..
        } = &first.message
        else {
            panic!()
        };
        assert_eq!(thinking[0].text, "The user wants the file contents.");
        assert_eq!(text, "Let me read it.");
        assert_eq!(tool_calls[0].id, "toolu_01");
        assert_eq!(tool_calls[0].arguments, json!({ "path": "src/app.rs" }));

        // Second request: the fixture asserts the exact body, including the
        // verbatim thinking block with its signature.
        req.messages.push(first.message.clone());
        req.messages.push(Message::ToolResult {
            call_id: "toolu_01".into(),
            content: vec![ContentPart::Text {
                text: "fn main() {}".into(),
            }],
            is_error: false,
        });
        let second = collect(p.stream(req).await.unwrap(), p.id()).await.unwrap();
        assert_eq!(
            assistant_parts(&second.message).0,
            "It contains an empty `main`."
        );
        assert_eq!(second.usage.cache_read_tokens, 840);
        assert_eq!(t.remaining(), 0, "{chunking:?}");
    }
}

#[tokio::test]
async fn sends_images() {
    let (p, t) = provider("vision", None);
    let msg = Message::User {
        content: vec![
            ContentPart::Image {
                media_type: "image/png".into(),
                data_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==".into(),
            },
            ContentPart::Text {
                text: "What color is this?".into(),
            },
        ],
    };
    let done = collect(
        p.stream(request("claude-opus-5-5", vec![msg]))
            .await
            .unwrap(),
        p.id(),
    )
    .await
    .unwrap();
    assert_eq!(
        assistant_parts(&done.message).0,
        "It is a single red pixel."
    );
    assert_eq!(t.remaining(), 0);
}

#[tokio::test]
async fn fallback_voids_declined_attempt() {
    for chunking in CHUNKINGS {
        let (p, _) = provider("fallback", chunking);
        let done = collect(
            p.stream(request("claude-fable-5-1", vec![Message::user_text("go")]))
                .await
                .unwrap(),
            p.id(),
        )
        .await
        .unwrap();
        let (text, thinking, tools) = assistant_parts(&done.message);
        assert_eq!(text, "Partial answer. Continued answer.");
        assert_eq!((thinking, tools), (0, 0), "declined attempt is void");
        let Message::Assistant { replay, .. } = &done.message else {
            panic!()
        };
        let replay = replay.as_ref().unwrap();
        let types: Vec<&str> = replay
            .content
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            types,
            ["text", "text"],
            "only text before the boundary is echoed"
        );
    }
}

#[tokio::test]
async fn refusal_is_a_stop_reason_not_an_error() {
    let (p, _) = provider("refusal", None);
    let done = collect(
        p.stream(request("claude-opus-5-5", vec![Message::user_text("x")]))
            .await
            .unwrap(),
        p.id(),
    )
    .await
    .unwrap();
    assert_eq!(
        done.stop_reason,
        StopReason::Refusal {
            category: Some("cyber".into())
        }
    );
}

#[tokio::test]
async fn http_and_stream_errors_are_typed() {
    let (p, _) = provider("rate_limited", None);
    let err = p
        .stream(request("claude-opus-5-5", vec![Message::user_text("x")]))
        .await
        .err()
        .unwrap();
    assert_eq!(
        err,
        ProviderError::RateLimited {
            retry_after_secs: Some(7)
        }
    );

    let (p, _) = provider("overloaded_midstream", Some(5));
    let s = p
        .stream(request("claude-opus-5-5", vec![Message::user_text("x")]))
        .await
        .unwrap();
    let err = collect(s, p.id()).await.unwrap_err();
    assert!(matches!(err, ProviderError::Server { status: 529, .. }) && err.is_retryable());

    let (p, _) = provider("truncated", None);
    let s = p
        .stream(request("claude-opus-5-5", vec![Message::user_text("x")]))
        .await
        .unwrap();
    assert_eq!(
        collect(s, p.id()).await.unwrap_err(),
        ProviderError::Incomplete
    );

    let (p, _) = provider("bad_key", None);
    assert_eq!(
        p.capabilities("claude-opus-5-5").await.unwrap_err(),
        ProviderError::Auth
    );
}

/// Replays every real recording in `fixtures/anthropic/recorded/` (made by the
/// `record_anthropic` example). Model output varies, so this checks that each
/// exchange decodes to completion rather than asserting exact text.
#[tokio::test]
async fn recorded_fixtures_decode() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/anthropic/recorded");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // Nothing recorded yet.
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let mut fx = Fixture::load(&path).unwrap();
        assert!(fx.iter().all(|f| f.source.as_deref() == Some("recorded")));
        let model = fx.iter().find_map(|f| {
            f.request
                .path
                .strip_prefix("/v1/models/")
                .map(str::to_owned)
        });
        let posts = fx
            .iter()
            .filter(|f| f.request.path == "/v1/messages")
            .count();
        let lists = fx
            .iter()
            .filter(|f| f.request.path.starts_with("/v1/models?"))
            .count();
        for f in &mut fx {
            f.request.body = None; // Replay without knowing the original request.
        }
        let t = Arc::new(FixtureTransport::new(fx).rechunked(7));
        let p = Anthropic::new(AnthropicConfig::new("sk-ant-test"), t.clone());
        for _ in 0..posts {
            let model = model.clone().unwrap();
            let s = p
                .stream(request(&model, vec![Message::user_text("replay")]))
                .await
                .unwrap();
            collect(s, p.id())
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
        if lists > 0 {
            assert!(!p.list_models().await.unwrap().is_empty());
        }
        assert_eq!(t.remaining(), 0, "{}", path.display());
    }
}

#[tokio::test]
async fn capabilities_and_model_list() {
    let (p, t) = provider("text", None);
    let caps = p.capabilities("claude-opus-5-5").await.unwrap();
    assert!(caps.tool_calling && caps.vision && caps.thinking);
    assert_eq!(caps.context_window, Some(1_000_000));
    assert_eq!(caps.max_output_tokens, Some(128_000));
    // Cached: the stream call must not fetch model metadata again.
    collect(
        p.stream(request(
            "claude-opus-5-5",
            vec![Message::user_text("Say hi")],
        ))
        .await
        .unwrap(),
        p.id(),
    )
    .await
    .unwrap();
    assert_eq!(t.remaining(), 0);

    let (p, _) = provider("models_list", None);
    let ids: Vec<String> = p
        .list_models()
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(
        ids,
        [
            "claude-fable-5-1",
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-haiku-5-5"
        ]
    );
}
