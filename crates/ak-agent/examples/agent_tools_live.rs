//! Live check of the real project tools: a scratch project with a red button, and
//! the request "make the button blue". Prints the steps and the resulting file.
//! Manual check only; not part of CI.
//!
//! ```text
//! AK_BASE_URL=http://127.0.0.1:8080/v1 AK_MODEL=… cargo run -p ak-agent --example agent_tools_live
//! # AK_PROTOCOL=anthropic with AK_BASE_URL=http://127.0.0.1:8080 uses the Anthropic adapter.
//! ```

use std::error::Error;
use std::sync::Arc;

use ak_agent::tools::{Asker, Question, ToolEnv, project_tools};
use ak_agent::{Agent, AgentConfig};
use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::agent::AgentEvent;
use ak_types::chat::{Conversation, Message};
use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

/// Answers every question with the first (recommended) option.
struct FirstOption;

impl Asker for FirstOption {
    fn ask(&self, q: Question) -> BoxFuture<'_, Option<String>> {
        println!(
            "\n[question] {} {:?} -> {}",
            q.question, q.options, q.options[0]
        );
        Box::pin(async move { q.options.into_iter().next() })
    }
}

const APP: &str = r#"use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="p-8">
            <h1 class="text-2xl">"My Todo List"</h1>
            <button data-ak-id="add-button" class="bg-red-500 text-white px-4 py-2 rounded">
                "Add"
            </button>
        </main>
    }
}
"#;

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
            OpenAiCompatConfig::new(base, key),
            transport,
        ))
    };

    let project = std::env::temp_dir().join(format!("agentknife-live-{}", std::process::id()));
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(project.join("src/app.rs"), APP)?;
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"todo\"\nversion = \"0.1.0\"\n",
    )?;

    let mut config = AgentConfig::new(model);
    config.max_tokens = 8_192;
    config.max_steps = 12;
    config.project_dir = Some(project.clone());
    config.system = Some(
        "You are AgentKnife, a coding agent for people who do not program. Answer in the \
         user's language. Look at the project with your tools before changing it, make the \
         smallest change that does what the user asked, then say briefly what you changed."
            .into(),
    );
    let agent = Agent::new(
        provider,
        project_tools(ToolEnv::default(), Arc::new(FirstOption)),
        config,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let printer = tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            match e {
                AgentEvent::TextDelta { text } => print!("{text}"),
                AgentEvent::StepStarted { tool, label, .. } => {
                    println!("\n[step] {label}  ({tool})")
                }
                AgentEvent::StepFinished { ok, .. } => println!("[step ok={ok}]"),
                AgentEvent::TurnFinished { outcome } => println!("\n[turn] {outcome:?}"),
                _ => {}
            }
        }
    });
    let mut conv = Conversation::new();
    agent
        .run_turn(
            &mut conv,
            Message::user_text("請把「Add」按鈕改成藍色"),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    drop(tx);
    printer.await?;
    let after = std::fs::read_to_string(project.join("src/app.rs"))?;
    println!("\n----- src/app.rs after the turn -----");
    for line in after.lines().filter(|l| l.contains("button")) {
        println!("{line}");
    }
    let lock_left = project.join(".agentknife/turn.lock").exists();
    println!("turn.lock left behind: {lock_left}");
    std::fs::remove_dir_all(&project)?;
    Ok(())
}
