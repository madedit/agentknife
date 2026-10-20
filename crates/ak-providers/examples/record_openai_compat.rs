//! Records real Chat Completions exchanges into
//! `tests/fixtures/openai/recorded/<provider-id>_<scenario>.json`.
//!
//! Free against a local Ollama or LM Studio. Credentials come only from the
//! environment and are never written to fixtures.
//!
//! ```text
//! AK_MODEL=qwen3:8b cargo run -p ak-providers --example record_openai_compat
//! # optional: AK_BASE_URL (default Ollama, http://localhost:11434/v1), AK_API_KEY,
//! #   AK_WIRE=responses|chat (default: by flavor), AK_VISION=1 (adds an image scenario)
//! ```

use std::error::Error;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ak_providers::fixture::RecordingTransport;
use ak_providers::openai::{Flavor, OpenAiCompat, OpenAiCompatConfig, WireApi};
use ak_providers::{ChatRequest, Provider, ReqwestTransport, collect};
use ak_types::chat::{ContentPart, Message, ToolSpec};
use serde_json::json;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[tokio::main]
async fn main() -> Result<()> {
    let model = std::env::var("AK_MODEL").map_err(|_| "set AK_MODEL (e.g. qwen3:8b)")?;
    let base = std::env::var("AK_BASE_URL").unwrap_or_else(|_| {
        Flavor::Ollama
            .default_base_url()
            .unwrap_or_default()
            .to_owned()
    });
    let mut config =
        OpenAiCompatConfig::new(base, std::env::var("AK_API_KEY").ok().map(Into::into));
    let mut prefix = config.flavor.id().to_owned();
    match std::env::var("AK_WIRE").as_deref() {
        Ok("responses") => {
            config.api = WireApi::Responses;
            prefix.push_str("_responses");
        }
        Ok("chat") => config.api = WireApi::ChatCompletions,
        _ => {}
    }
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openai/recorded");
    std::fs::create_dir_all(&out)?;

    let req = |messages: Vec<Message>, tools: Vec<ToolSpec>| ChatRequest {
        model: model.clone(),
        system: Some("You are AgentKnife, a helpful coding agent.".into()),
        messages,
        tools,
        max_tokens: 2_048,
        effort: None,
    };
    let tool = ToolSpec {
        name: "read_file".into(),
        description: "Read a project file".into(),
        input_schema: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"],
        }),
    };

    let m = model.clone();
    record(
        &out,
        &format!("{prefix}_capabilities"),
        &config,
        |p| async move {
            println!("capabilities: {:?}", p.capabilities(&m).await?);
            Ok(())
        },
    )
    .await?;

    let r = req(vec![Message::user_text("Say hi in five words.")], vec![]);
    record(&out, &format!("{prefix}_text"), &config, |p| async move {
        collect(p.stream(r).await?, p.id()).await?;
        Ok(())
    })
    .await?;

    let r = req(
        vec![Message::user_text(
            "What is in src/app.rs? Use the read_file tool.",
        )],
        vec![tool],
    );
    record(
        &out,
        &format!("{prefix}_tool_use"),
        &config,
        |p| async move {
            let first = collect(p.stream(r.clone()).await?, p.id()).await?;
            let call_id =
                match &first.message {
                    Message::Assistant { tool_calls, .. } => tool_calls
                        .first()
                        .map(|c| c.id.clone())
                        .ok_or("model did not call the tool (does it support tools?)")?,
                    _ => return Err("not an assistant message".into()),
                };
            let mut next = r;
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
        },
    )
    .await?;

    if std::env::var("AK_VISION").is_ok_and(|v| v == "1") {
        let r = req(
            vec![Message::User {
                content: vec![
                    ContentPart::Text {
                        text: "What color is this image? One word.".into(),
                    },
                    ContentPart::Image {
                        media_type: "image/png".into(),
                        data_base64: RED_PIXEL_PNG.into(),
                    },
                ],
            }],
            vec![],
        );
        record(&out, &format!("{prefix}_vision"), &config, |p| async move {
            let done = collect(p.stream(r).await?, p.id()).await?;
            println!("vision answer: {:?}", done.message);
            Ok(())
        })
        .await?;
    }
    Ok(())
}

/// A 1x1 red PNG.
const RED_PIXEL_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

async fn record<F, Fut>(
    out: &Path,
    name: &str,
    config: &OpenAiCompatConfig,
    scenario: F,
) -> Result<()>
where
    F: FnOnce(Arc<OpenAiCompat>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let recorder = Arc::new(RecordingTransport::new(ReqwestTransport::new()?));
    let provider = Arc::new(OpenAiCompat::new(config.clone(), recorder.clone()));
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
