//! Live check of the agent with the running preview: a project from the stack's
//! template, its preview started with the isolated toolchain, the page opened in a
//! browser by hand, then one request to a real model. Prints the agent's steps,
//! the build checks and what the page shows at the end. Manual only.
//!
//! ```text
//! AK_BASE_URL=http://127.0.0.1:8080/v1 AK_MODEL=… cargo run -p ak-agent --example agent_preview_live -- "<request>"
//! # Open the printed page URL in a browser when asked (the control channel needs it).
//! # The project lives in target/agent-preview-live (kept, so later runs build fast).
//! ```

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use ak_agent::plan::PlanReviewer;
use ak_agent::tools::{Asker, Question, ToolEnv, project_tools, with_preview_tools};
use ak_agent::{Agent, AgentConfig, prompt};
use ak_doctor::{Pinned, Toolchain};
use ak_preview::{Preview, PreviewSession, Tools, profiles};
use ak_project::Project;
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::agent::{AgentEvent, PlanDecision, PlanProposal};
use ak_types::chat::{Conversation, Message};
use ak_types::i18n::Locale;
use ak_types::preview::ControlAction;
use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

struct FirstOption;

impl Asker for FirstOption {
    fn ask(&self, q: Question) -> BoxFuture<'_, Option<String>> {
        println!("\n[question] {} -> {:?}", q.question, q.options.first());
        Box::pin(async move { q.options.into_iter().next() })
    }
}

struct Start;

impl PlanReviewer for Start {
    fn review(&self, _: String, plan: PlanProposal) -> BoxFuture<'_, PlanDecision> {
        println!("\n[plan] {} -> Start", plan.title);
        Box::pin(async { PlanDecision::Start })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let base = std::env::var("AK_BASE_URL").map_err(|_| "set AK_BASE_URL")?;
    let model = std::env::var("AK_MODEL").map_err(|_| "set AK_MODEL")?;
    let request = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "把 Greet 按鈕的文字改成「打招呼」，並確認畫面上真的改了。".into());

    let profile = profiles::default_profile();
    let root = std::env::current_dir()?.join("target/agent-preview-live");
    std::fs::create_dir_all(&root)?;
    let mut project = Project::init(&root, "Preview Agent", Locale::ZhTw)?;
    project.scaffold(profile)?;
    let tc = Toolchain::new(Toolchain::default_root(), &Pinned::builtin()?);
    let tools = Tools::from_toolchain(&tc);
    let preview = Preview::new(&root, profile.preview(), tools.clone())?;
    let session = Arc::new(PreviewSession::new(preview, None));
    session.start().await?;
    println!("project: {}\nbuilding the preview…", root.display());
    let st = session.settled(Duration::from_secs(900)).await;
    if !st.ready() {
        return Err(format!("preview not ready: {st:?}").into());
    }
    println!("\n>>> OPEN IN A BROWSER: {}\n", session.page_url().await);
    // The control channel needs the page: wait until it answers.
    loop {
        if session.act(&ControlAction::DomSummary).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("page connected");

    let provider: Arc<dyn Provider> = Arc::new(OpenAiCompat::new(
        OpenAiCompatConfig::new(base, std::env::var("AK_API_KEY").ok().map(Into::into)),
        Arc::new(ReqwestTransport::new()?),
    ));
    let mut config = AgentConfig::new(model);
    config.max_tokens = 8_192;
    config.max_steps = 25;
    config.project_dir = Some(root.clone());
    config.ui_locale = Locale::ZhTw;
    config.system = Some(prompt::product_prompt().to_owned());
    let env = ToolEnv {
        path_prefix: tools.path_prefix.clone(),
        vars: tools.env.clone(),
    };
    let registry = with_preview_tools(
        project_tools(env, Arc::new(FirstOption), Some(profile.knowledge_pack())),
        session.clone(),
        false,
    );
    let agent = Agent::new(provider, registry, config)
        .with_gate(Arc::new(ak_agent::permission::AllowAll))
        .with_plan_reviewer(Arc::new(Start))
        .with_preview(session.clone());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let printer = tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            match e {
                AgentEvent::TextDelta { text } => print!("{text}"),
                AgentEvent::StepStarted { tool, label, .. } => {
                    println!("\n[step] {label}  ({tool})")
                }
                AgentEvent::StepFinished { ok, .. } => println!("[step ok={ok}]"),
                AgentEvent::BuildChecking => println!("\n[build] checking…"),
                AgentEvent::BuildChecked { ok, attempt, max } => {
                    println!("[build] ok={ok} attempt {attempt}/{max}")
                }
                AgentEvent::TurnFinished { outcome } => println!("\n[turn] {outcome:?}"),
                _ => {}
            }
        }
    });
    let mut conversation = Conversation::new();
    println!("[user] {request}");
    agent
        .run_turn(
            &mut conversation,
            Message::user_text(request),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    drop(tx);
    let _ = printer.await;

    // Why steps failed (the events only say that they did).
    for m in conversation.messages() {
        if let Message::ToolResult {
            content,
            is_error: true,
            ..
        } = m
        {
            for c in content {
                if let ak_types::chat::ContentPart::Text { text } = c {
                    let short: String = text.chars().take(160).collect();
                    println!("[failed step] {}", short.replace('\n', " / "));
                }
            }
        }
    }

    let final_view = session.dom_summary().await?;
    println!("\n----- the page now -----\n{}", final_view.render());
    session.stop().await;
    Ok(())
}
