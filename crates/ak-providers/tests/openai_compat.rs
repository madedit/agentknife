//! OpenAI-compatible provider against fixtures (tests/fixtures/openai). No network.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use ak_providers::fixture::{Fixture, FixtureTransport};
use ak_providers::openai::{Flavor, OpenAiCompat, OpenAiCompatConfig, WireApi};
use ak_providers::{ChatRequest, Effort, Provider, ProviderError, StopReason, collect};
use ak_types::chat::{ContentPart, Message, ToolSpec};
use serde_json::json;

const CHUNKINGS: [Option<usize>; 4] = [None, Some(1), Some(5), Some(64)];

fn provider(
    name: &str,
    flavor: Flavor,
    chunking: Option<usize>,
) -> (OpenAiCompat, Arc<FixtureTransport>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/openai")
        .join(format!("{name}.json"));
    let fixtures = Fixture::load(path).unwrap();
    // The fixture says which protocol it speaks (OpenAI's Chat Completions fixtures
    // keep covering that decoder, which other servers use).
    let api = if fixtures[0].request.path.ends_with("/responses") {
        WireApi::Responses
    } else {
        WireApi::ChatCompletions
    };
    let mut t = FixtureTransport::new(fixtures);
    if let Some(n) = chunking {
        t = t.rechunked(n);
    }
    let t = Arc::new(t);
    let key = (flavor == Flavor::OpenAi || flavor == Flavor::OpenRouter).then(|| "sk-test".into());
    let mut config = OpenAiCompatConfig::for_flavor(flavor, key);
    config.api = api;
    (OpenAiCompat::new(config, t.clone()), t)
}

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

fn request(model: &str, effort: Option<Effort>) -> ChatRequest {
    ChatRequest {
        model: model.into(),
        system: Some("You are AgentKnife.".into()),
        messages: vec![Message::user_text("What is in src/app.rs?")],
        tools: vec![tool()],
        max_tokens: 4096,
        effort,
    }
}

fn parts(m: &Message) -> (String, Vec<String>, Vec<(String, serde_json::Value)>) {
    let Message::Assistant {
        text,
        thinking,
        tool_calls,
        ..
    } = m
    else {
        panic!("expected assistant message");
    };
    (
        text.clone(),
        thinking.iter().map(|t| t.text.clone()).collect(),
        tool_calls
            .iter()
            .map(|c| (c.id.clone(), c.arguments.clone()))
            .collect(),
    )
}

fn tool_result(id: &str) -> Message {
    Message::ToolResult {
        call_id: id.into(),
        content: vec![ContentPart::Text {
            text: "fn main() {}".into(),
        }],
        is_error: false,
    }
}

#[tokio::test]
async fn openai_streams_fragmented_tool_calls_and_usage() {
    for chunking in CHUNKINGS {
        let (p, t) = provider("openai_tools", Flavor::OpenAi, chunking);
        let done = collect(
            p.stream(request("gpt-test", Some(Effort::Low)))
                .await
                .unwrap(),
            p.id(),
        )
        .await
        .unwrap();
        let (text, _, calls) = parts(&done.message);
        assert_eq!(text, "Reading it.");
        assert_eq!(
            calls,
            [("call_abc".to_owned(), json!({ "path": "src/app.rs" }))]
        );
        assert_eq!(done.stop_reason, StopReason::ToolUse);
        assert_eq!(
            (
                done.usage.input_tokens,
                done.usage.cache_read_tokens,
                done.usage.output_tokens
            ),
            (50, 200, 31)
        );
        assert_eq!(t.remaining(), 0, "{chunking:?}");
        let auth = t.sent()[0]
            .headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .map(|(_, v)| v.clone());
        assert_eq!(auth.as_deref(), Some("Bearer sk-test"));
    }
}

#[tokio::test]
async fn ollama_reasoning_round_trip_echoes_reasoning() {
    for chunking in CHUNKINGS {
        let (p, t) = provider("ollama_reasoning_tools", Flavor::Ollama, chunking);
        assert_eq!(p.id(), "ollama");
        let mut req = request("qwen3:8b", Some(Effort::High));
        let first = collect(p.stream(req.clone()).await.unwrap(), p.id())
            .await
            .unwrap();
        let (_, thinking, calls) = parts(&first.message);
        assert_eq!(thinking, ["I should read the file."]);
        assert_eq!(calls[0].0, "call_x1");
        assert!(
            t.sent()[0]
                .headers
                .iter()
                .all(|(k, _)| k != "authorization"),
            "no key for local servers"
        );

        // The fixture asserts the follow-up body, including the echoed `reasoning`.
        req.messages.push(first.message);
        req.messages.push(tool_result("call_x1"));
        let second = collect(p.stream(req).await.unwrap(), p.id()).await.unwrap();
        assert_eq!(parts(&second.message).0, "It is an empty main.");
        assert_eq!(second.stop_reason, StopReason::EndTurn);
        assert_eq!(t.remaining(), 0, "{chunking:?}");
    }
}

#[tokio::test]
async fn ollama_capabilities_flag_models_without_tools() {
    let (p, t) = provider("ollama_show", Flavor::Ollama, None);
    let small = p.capabilities("gemma2:2b").await.unwrap();
    assert!(!small.tool_calling);
    assert_eq!(small.context_window, Some(8192));
    let qwen = p.capabilities("qwen3:8b").await.unwrap();
    assert!(qwen.tool_calling && qwen.thinking);
    assert_eq!(qwen.context_window, Some(40960));
    // Cached: no further requests.
    p.capabilities("gemma2:2b").await.unwrap();
    assert_eq!(t.remaining(), 0);
    assert_eq!(t.sent()[0].url, "http://localhost:11434/api/show");
}

#[tokio::test]
async fn lmstudio_think_tags_become_thinking() {
    for chunking in CHUNKINGS {
        let (p, _) = provider("lmstudio_think_tags", Flavor::LmStudio, chunking);
        let mut req = request("qwen/qwen3-8b", None);
        req.tools.clear();
        let done = collect(p.stream(req).await.unwrap(), p.id()).await.unwrap();
        let (text, thinking, _) = parts(&done.message);
        assert_eq!(text, "Hello!", "{chunking:?}");
        assert_eq!(thinking, ["The user wants a greeting."]);
    }
}

#[tokio::test]
async fn lmstudio_capabilities() {
    // One provider: the model list is fetched once and reused.
    let (p, t) = provider("lmstudio_models", Flavor::LmStudio, None);
    let qwen = p.capabilities("qwen/qwen3-8b").await.unwrap();
    assert!(qwen.tool_calling && qwen.thinking && !qwen.vision);
    assert_eq!(qwen.context_window, Some(32768));
    let gemma = p.capabilities("google/gemma-3-4b").await.unwrap();
    assert!(!gemma.tool_calling && gemma.vision);
    assert!(matches!(
        p.capabilities("nope").await.unwrap_err(),
        ProviderError::ModelNotFound(_)
    ));
    assert_eq!(t.sent().len(), 1);
    assert_eq!(t.sent()[0].url, "http://localhost:1234/api/v1/models");
}

#[tokio::test]
async fn openrouter_reasoning_details_are_echoed_unmodified() {
    for chunking in CHUNKINGS {
        let (p, t) = provider("openrouter_reasoning", Flavor::OpenRouter, chunking);
        let mut req = request("anthropic/claude-opus-5-5", None);
        let first = collect(p.stream(req.clone()).await.unwrap(), p.id())
            .await
            .unwrap();
        let (_, thinking, calls) = parts(&first.message);
        assert_eq!(thinking, ["Check the file first."]);
        assert_eq!(calls[0].0, "toolu_or1");
        assert!(
            t.sent()[0]
                .headers
                .iter()
                .any(|(k, v)| k == "x-openrouter-title" && v == "AgentKnife")
        );

        req.messages.push(first.message);
        req.messages.push(tool_result("toolu_or1"));
        let second = collect(p.stream(req).await.unwrap(), p.id()).await.unwrap();
        assert_eq!(parts(&second.message).0, "Empty main.");
        assert_eq!(t.remaining(), 0, "{chunking:?}");
    }
}

#[tokio::test]
async fn openrouter_capabilities_and_models() {
    let (p, _) = provider("openrouter_models", Flavor::OpenRouter, None);
    let opus = p.capabilities("anthropic/claude-opus-5-5").await.unwrap();
    assert!(opus.tool_calling && opus.vision && opus.thinking);
    assert_eq!(opus.max_output_tokens, Some(128_000));
    let tiny = p.capabilities("some/tiny-model").await.unwrap();
    assert!(
        !tiny.tool_calling,
        "no `tools` in supported_parameters => flagged"
    );
    assert_eq!(tiny.max_output_tokens, None);
}

/// Replays real recordings in `fixtures/openai/recorded/` (made by the
/// `record_openai_compat` example), named `<provider-id>_<scenario>.json`.
/// Model output varies, so this checks that every exchange decodes to completion.
#[tokio::test]
async fn recorded_fixtures_decode() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openai/recorded");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // Nothing recorded yet.
    };
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let flavor = [
            Flavor::OpenAi,
            Flavor::Ollama,
            Flavor::LmStudio,
            Flavor::OpenRouter,
            Flavor::Generic,
        ]
        .into_iter()
        .find(|f| name.starts_with(&format!("{}_", f.id())))
        .unwrap_or_else(|| panic!("{name}: unknown provider prefix"));
        let mut fx = Fixture::load(&path).unwrap();
        assert!(fx.iter().all(|f| f.source.as_deref() == Some("recorded")));
        let is_post = |f: &Fixture| {
            f.request.path.ends_with("/chat/completions") || f.request.path.ends_with("/responses")
        };
        let posts = fx.iter().filter(|f| is_post(f)).count();
        let api = if fx.iter().any(|f| f.request.path.ends_with("/responses")) {
            WireApi::Responses
        } else {
            WireApi::ChatCompletions
        };
        let probes = fx.len() - posts;
        for f in &mut fx {
            f.request.body = None; // Replay without knowing the original request.
        }
        let t = Arc::new(FixtureTransport::new(fx).rechunked(7));
        let config = OpenAiCompatConfig {
            base_url: flavor
                .default_base_url()
                .unwrap_or("http://recorded.invalid/v1")
                .to_owned(),
            api_key: None,
            flavor,
            api,
        };
        let p = OpenAiCompat::new(config, t.clone());
        if probes > 0 {
            p.capabilities("recorded").await.ok();
        }
        for _ in 0..posts {
            let s = p.stream(request("recorded", None)).await.unwrap();
            collect(s, p.id())
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
        assert_eq!(t.remaining(), 0, "{}", path.display());
    }
}

#[tokio::test]
async fn errors_are_typed() {
    let (p, _) = provider("openrouter_midstream_error", Flavor::OpenRouter, Some(3));
    let s = p.stream(request("x", None)).await.unwrap();
    let err = collect(s, p.id()).await.unwrap_err();
    assert!(
        matches!(err, ProviderError::Server { status: 502, .. }),
        "{err:?}"
    );

    let (p, _) = provider("openai_unauthorized", Flavor::OpenAi, None);
    assert_eq!(
        p.stream(request("gpt-test", None)).await.err().unwrap(),
        ProviderError::Auth
    );
}

fn sent_body(t: &FixtureTransport, n: usize) -> serde_json::Value {
    let req = &t.sent()[n];
    serde_json::from_slice(req.body.as_deref().unwrap()).unwrap()
}

#[tokio::test]
async fn responses_reasoning_and_tool_round_trip() {
    for chunking in CHUNKINGS {
        let (p, t) = provider("openai_responses_tools", Flavor::OpenAi, chunking);
        let first = collect(
            p.stream(request("gpt-6.1-sol", Some(Effort::XHigh)))
                .await
                .unwrap(),
            p.id(),
        )
        .await
        .unwrap();
        let (_, thinking, calls) = parts(&first.message);
        assert_eq!(
            thinking,
            ["**Reading the file**\n\nI should look at src/app.rs."]
        );
        assert_eq!(
            calls,
            [("call_A1".to_owned(), json!({ "path": "src/app.rs" }))]
        );
        assert_eq!(first.stop_reason, StopReason::ToolUse);
        assert_eq!(
            (first.usage.input_tokens, first.usage.cache_read_tokens),
            (176, 1024)
        );

        let body = sent_body(&t, 0);
        assert_eq!(t.sent()[0].url, "https://api.openai.com/v1/responses");
        assert_eq!(body["store"], false);
        assert_eq!(body["reasoning"]["effort"], "xhigh");
        assert_eq!(body["instructions"], "You are AgentKnife.");

        // Second turn: our own output items go back verbatim, then the tool output.
        let mut req = request("gpt-6.1-sol", Some(Effort::XHigh));
        req.messages.push(first.message.clone());
        req.messages.push(tool_result("call_A1"));
        let second = collect(p.stream(req).await.unwrap(), p.id()).await.unwrap();
        assert_eq!(parts(&second.message).0, "It renders a counter.");
        assert_eq!(second.stop_reason, StopReason::EndTurn);
        let input = sent_body(&t, 1)["input"].clone();
        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(
            input[1]["encrypted_content"],
            "gAAAAB-encrypted-reasoning-A1"
        );
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["call_id"], "call_A1");
        assert_eq!(
            input[3],
            json!({ "type": "function_call_output", "call_id": "call_A1", "output": "fn main() {}" })
        );
        assert_eq!(t.remaining(), 0, "{chunking:?}");
    }
}

#[tokio::test]
async fn responses_vision_sends_images_in_input_and_tool_output() {
    let (p, t) = provider("openai_responses_vision", Flavor::OpenAi, None);
    let img = || ContentPart::Image {
        media_type: "image/png".into(),
        data_base64: "iVBORw0KGgo=".into(),
    };
    let mut req = request("gpt-6-astra", None);
    req.messages = vec![
        Message::User {
            content: vec![
                ContentPart::Text {
                    text: "What is this?".into(),
                },
                img(),
            ],
        },
        Message::Assistant {
            thinking: vec![],
            text: String::new(),
            tool_calls: vec![ak_types::chat::ToolCall {
                id: "call_S".into(),
                name: "screenshot".into(),
                arguments: json!({}),
                invalid_arguments: None,
            }],
            replay: None,
        },
        Message::ToolResult {
            call_id: "call_S".into(),
            content: vec![img()],
            is_error: false,
        },
    ];
    let done = collect(p.stream(req).await.unwrap(), p.id()).await.unwrap();
    assert_eq!(parts(&done.message).0, "A blue button.");
    let input = sent_body(&t, 0)["input"].clone();
    assert_eq!(input[0]["content"][1]["type"], "input_image");
    assert_eq!(
        input[0]["content"][1]["image_url"],
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert_eq!(input[1]["type"], "function_call");
    assert_eq!(input[2]["output"][0]["type"], "input_image");
    assert_eq!(
        p.capabilities("gpt-6-astra")
            .await
            .unwrap()
            .max_output_tokens,
        Some(128_000)
    );
}

#[tokio::test]
async fn responses_errors() {
    let (p, _) = provider("openai_responses_unauthorized", Flavor::OpenAi, None);
    let err = p.stream(request("gpt-6-luna", None)).await.err().unwrap();
    assert_eq!(err, ProviderError::Auth);

    let (p, _) = provider("openai_responses_failed", Flavor::OpenAi, Some(3));
    let err = collect(p.stream(request("gpt-6-luna", None)).await.unwrap(), p.id())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ProviderError::Server { message, .. } if message.contains("server_error")),
        "{err:?}"
    );
    assert!(err.is_retryable());
}
