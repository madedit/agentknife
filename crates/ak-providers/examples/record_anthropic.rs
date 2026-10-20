//! Records real Anthropic exchanges into `tests/fixtures/anthropic/recorded/`.
//!
//! Costs a few cents. Credentials come only from the environment and are never
//! written to fixtures (request headers are not recorded).
//!
//! ```text
//! ANTHROPIC_API_KEY=... cargo run -p ak-providers --example record_anthropic
//! # optional: AK_RECORD_MODEL=claude-sonnet-5-5
//! # optional: AK_BASE_URL=http://127.0.0.1:8080 (an Anthropic-compatible server;
//! #           then ANTHROPIC_API_KEY may be any placeholder)
//! ```

use std::error::Error;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use ak_providers::anthropic::{Anthropic, AnthropicConfig, DEFAULT_MODEL};
use ak_providers::fixture::RecordingTransport;
use ak_providers::{ChatRequest, Effort, Provider, ReqwestTransport, collect};
use ak_types::chat::{ContentPart, Message, ToolSpec};
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

#[tokio::main]
async fn main() -> Result<()> {
    let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| "set ANTHROPIC_API_KEY")?;
    let model = std::env::var("AK_RECORD_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/anthropic/recorded");
    std::fs::create_dir_all(&out)?;

    let req = |messages: Vec<Message>| ChatRequest {
        model: model.clone(),
        system: Some("You are AgentKnife, a helpful coding agent.".into()),
        messages,
        tools: vec![],
        max_tokens: 4_096,
        effort: Some(Effort::Low),
    };

    record(&out, "text", &key, |p| {
        let r = req(vec![Message::user_text("Say hi in five words.")]);
        async move {
            collect(p.stream(r).await?, p.id()).await?;
            Ok(())
        }
    })
    .await?;

    let mut tool_req = req(vec![Message::user_text(
        "What is in src/app.rs? Use the read_file tool.",
    )]);
    tool_req.tools = vec![ToolSpec {
        name: "read_file".into(),
        description: "Read a project file".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
        }),
    }];
    tool_req.effort = Some(Effort::High);
    record(&out, "thinking_tool_use", &key, |p| async move {
        let first = collect(p.stream(tool_req.clone()).await?, p.id()).await?;
        let mut next = tool_req;
        let call_id = match &first.message {
            Message::Assistant { tool_calls, .. } => tool_calls
                .first()
                .map(|c| c.id.clone())
                .ok_or("model did not call the tool")?,
            _ => return Err("not an assistant message".into()),
        };
        next.messages.push(first.message);
        next.messages.push(Message::ToolResult {
            call_id,
            content: vec![ContentPart::Text {
                text: "fn main() {}".into(),
            }],
            is_error: false,
        });
        collect(p.stream(next).await?, p.id()).await?;
        Ok(())
    })
    .await?;

    record(&out, "vision", &key, |p| {
        let r = req(vec![Message::User {
            content: vec![
                ContentPart::Image {
                    media_type: "image/png".into(),
                    data_base64: PIXEL_PNG.into(),
                },
                ContentPart::Text {
                    text: "What color is this pixel?".into(),
                },
            ],
        }]);
        async move {
            collect(p.stream(r).await?, p.id()).await?;
            Ok(())
        }
    })
    .await?;

    record(&out, "models_list", &key, |p| async move {
        p.list_models().await?;
        Ok(())
    })
    .await?;
    Ok(())
}

async fn record<F, Fut>(out: &std::path::Path, name: &str, key: &str, scenario: F) -> Result<()>
where
    F: FnOnce(Arc<Anthropic>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let recorder = Arc::new(RecordingTransport::new(ReqwestTransport::new()?));
    let mut config = AnthropicConfig::new(key);
    if let Ok(base) = std::env::var("AK_BASE_URL") {
        config.base_url = base;
    }
    let provider = Arc::new(Anthropic::new(config, recorder.clone()));
    scenario(provider).await?;
    let fixtures = recorder.take();
    let path = out.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&fixtures)? + "\n")?;
    println!(
        "recorded {} exchange(s) -> {}",
        fixtures.len(),
        path.display()
    );
    Ok(())
}
