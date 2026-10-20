//! Runs one agent turn against a real model with a fake `read_file` tool and prints
//! the events. Manual check only; not part of CI.
//!
//! ```text
//! AK_BASE_URL=http://127.0.0.1:8080/v1 AK_MODEL=qwen3.8-flash-next-gptq-nvfp4 \
//!   cargo run -p ak-agent --example agent_live
//! # AK_PROTOCOL=anthropic with AK_BASE_URL=http://127.0.0.1:8080 uses the Anthropic adapter.
//! # AK_API_KEY is optional.
//! ```

use std::error::Error;
use std::sync::Arc;

use ak_agent::tool::{Tool, ToolContext, ToolOutput, ToolRegistry};
use ak_agent::{Agent, AgentConfig};
use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::agent::AgentEvent;
use ak_types::chat::{Conversation, Message, ToolSpec};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct ReadFile;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a file from the user's project.".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Project-relative path" } },
                "required": ["path"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            match args["path"].as_str() {
                Some("src/app.rs") => ToolOutput::text(
                    "use leptos::*;\n\n#[component]\nfn App() -> impl IntoView {\n    view! { <button data-ak-id=\"save\" class=\"bg-red-500\">\"Save\"</button> }\n}",
                ),
                Some(other) => ToolOutput::error(format!("No file at {other}")),
                None => ToolOutput::error("`path` is required"),
            }
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let base = std::env::var("AK_BASE_URL").map_err(|_| "set AK_BASE_URL")?;
    let model = std::env::var("AK_MODEL").map_err(|_| "set AK_MODEL")?;
    let key = std::env::var("AK_API_KEY").ok();
    let transport = Arc::new(ReqwestTransport::new()?);
    let provider: Arc<dyn Provider> = if std::env::var("AK_PROTOCOL").as_deref() == Ok("anthropic")
    {
        let mut c = AnthropicConfig::new(key.unwrap_or_else(|| "local".into()));
        c.base_url = base;
        Arc::new(Anthropic::new(c, transport))
    } else {
        Arc::new(OpenAiCompat::new(
            OpenAiCompatConfig::new(base, key.map(Into::into)),
            transport,
        ))
    };

    let mut config = AgentConfig::new(model);
    config.max_tokens = 4_096;
    config.system = Some(
        "You are AgentKnife, a coding agent for people who do not program. \
         Answer in the user's language. Use tools to look at the project before answering."
            .into(),
    );
    let agent = Agent::new(
        provider,
        ToolRegistry::new().with(Arc::new(ReadFile)),
        config,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let printer = tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            match e {
                AgentEvent::TextDelta { text } => print!("{text}"),
                AgentEvent::ThinkingDelta { .. } => print!("."),
                AgentEvent::StepStarted { tool, label, .. } => {
                    println!("\n[step] {label}  ({tool})")
                }
                AgentEvent::StepFinished { ok, .. } => println!("[step done ok={ok}]"),
                AgentEvent::Usage { usage } => println!("\n[usage] {usage:?}"),
                AgentEvent::TurnFinished { outcome } => println!("\n[turn] {outcome:?}"),
                _ => {}
            }
        }
    });
    let mut conv = Conversation::new();
    let outcome = agent
        .run_turn(
            &mut conv,
            Message::user_text("我的 src/app.rs 裡的按鈕是什麼顏色？"),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    drop(tx);
    printer.await?;
    println!(
        "messages in history: {} | outcome: {outcome:?}",
        conv.messages().len()
    );
    Ok(())
}
