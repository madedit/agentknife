//! Live check of the real project tools: a scratch project with a red button, and
//! the request "make the button blue". Prints the steps and the resulting file.
//! Manual check only; not part of CI.
//!
//! ```text
//! AK_BASE_URL=http://127.0.0.1:8080/v1 AK_MODEL=… cargo run -p ak-agent --example agent_tools_live
//! # AK_PROTOCOL=anthropic with AK_BASE_URL=http://127.0.0.1:8080 uses the Anthropic adapter;
//! # AK_PROTOCOL=responses uses the OpenAI Responses API.
//! ```

use std::error::Error;
use std::sync::Arc;

use ak_agent::permission::{ConfirmRequest, Confirmer};
use ak_agent::plan::PlanReviewer;
use ak_agent::tools::{Asker, Question, ToolEnv, project_tools};
use ak_agent::{Agent, AgentConfig};
use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig, WireApi};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::agent::AgentEvent;
use ak_types::agent::{PlanDecision, PlanProposal};
use ak_types::chat::{Conversation, Message};
use ak_types::i18n::{self, I18n, Locale};
use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

/// Prints the confirmation the user would see (zh-TW, via fluent) and allows it.
struct PrintAndAllow(I18n);

impl Confirmer for PrintAndAllow {
    fn confirm(&self, r: ConfirmRequest) -> BoxFuture<'_, bool> {
        let args = i18n::args([("detail", r.reason.detail.as_str())]);
        println!(
            "\n[confirm] {} — {} -> allow",
            r.label,
            self.0.t_args(r.reason.kind.message_id(), &args)
        );
        Box::pin(async { true })
    }
}

/// Prints a proposed plan and starts it.
struct PrintPlanAndStart;

impl PlanReviewer for PrintPlanAndStart {
    fn review(&self, _: String, plan: PlanProposal) -> BoxFuture<'_, PlanDecision> {
        println!(
            "\n[plan] {} ({})",
            plan.title,
            plan.estimate.unwrap_or_default()
        );
        for (i, s) in plan.steps.iter().enumerate() {
            println!("       {}. {s}", i + 1);
        }
        println!("       -> Start");
        Box::pin(async { PlanDecision::Start })
    }
}

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
        let mut c = OpenAiCompatConfig::new(base, key.map(Into::into));
        if std::env::var("AK_PROTOCOL").as_deref() == Ok("responses") {
            c.api = WireApi::Responses;
        }
        Arc::new(OpenAiCompat::new(c, transport))
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
    std::fs::create_dir_all(project.join("notes"))?;
    std::fs::write(project.join("notes/old.txt"), "outdated notes")?;
    // Default gate = the project policy; confirmations print the user-facing reason.
    let agent = Agent::new(
        provider,
        project_tools(ToolEnv::default(), Arc::new(FirstOption)),
        config,
    )
    .with_confirmer(Arc::new(PrintAndAllow(I18n::new(Locale::ZhTw)?)))
    .with_plan_reviewer(Arc::new(PrintPlanAndStart));
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
    for request in [
        "請把「Add」按鈕改成藍色",
        "notes/old.txt 用不到了，請把它刪掉",
        // A big change: the model should propose a plan first.
        "幫我把它做成完整的待辦清單：可以新增項目、勾選完成、刪除項目，並在下面顯示已完成幾項",
        // Another language: the reply should follow it.
        "Please make the title text bigger.",
    ] {
        println!("\n===== {request}");
        agent
            .run_turn(
                &mut conv,
                Message::user_text(request),
                &tx,
                &CancellationToken::new(),
            )
            .await;
    }
    drop(tx);
    printer.await?;
    let after = std::fs::read_to_string(project.join("src/app.rs"))?;
    println!("\n----- src/app.rs after the turn -----");
    for line in after.lines().filter(|l| l.contains("button")) {
        println!("{line}");
    }
    println!(
        "notes/old.txt still exists: {}",
        project.join("notes/old.txt").exists()
    );
    let lock_left = project.join(".agentknife/turn.lock").exists();
    println!("turn.lock left behind: {lock_left}");
    std::fs::remove_dir_all(&project)?;
    Ok(())
}
