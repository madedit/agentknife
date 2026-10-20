//! Live check of the first greeting, in both UI languages, with the real product
//! prompt and tools. Manual check only; not part of CI.
//!
//! ```text
//! AK_BASE_URL=… AK_MODEL=… [AK_PROTOCOL=anthropic] cargo run -p ak-agent --example agent_greeting_live
//! ```

use std::error::Error;
use std::sync::Arc;

use ak_agent::prompt::{product_prompt, product_prompt_version};
use ak_agent::tools::{Asker, Question, ToolEnv, project_tools};
use ak_agent::{Agent, AgentConfig};
use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::agent::AgentEvent;
use ak_types::chat::{Conversation, Message};
use ak_types::i18n::Locale;
use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

struct NoAnswer;

impl Asker for NoAnswer {
    fn ask(&self, _: Question) -> BoxFuture<'_, Option<String>> {
        Box::pin(async { None })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let base = std::env::var("AK_BASE_URL").map_err(|_| "set AK_BASE_URL")?;
    let model = std::env::var("AK_MODEL").map_err(|_| "set AK_MODEL")?;
    println!("prompt-version {}", product_prompt_version());
    for locale in [Locale::ZhTw, Locale::En] {
        let transport = Arc::new(ReqwestTransport::new()?);
        let provider: Arc<dyn Provider> =
            if std::env::var("AK_PROTOCOL").as_deref() == Ok("anthropic") {
                let mut c = AnthropicConfig::new("local");
                c.base_url = base.clone();
                Arc::new(Anthropic::new(c, transport))
            } else {
                Arc::new(OpenAiCompat::new(
                    OpenAiCompatConfig::new(base.clone(), None),
                    transport,
                ))
            };
        let project =
            std::env::temp_dir().join(format!("agentknife-greet-{}-{locale}", std::process::id()));
        std::fs::create_dir_all(&project)?;
        let mut config = AgentConfig::new(model.clone());
        config.max_tokens = 2_048;
        config.ui_locale = locale;
        config.system = Some(product_prompt().to_owned());
        config.project_dir = Some(project.clone());
        let agent = Agent::new(
            provider,
            project_tools(ToolEnv::default(), Arc::new(NoAnswer)),
            config,
        );

        let mut conv = Conversation::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = agent.greet(&mut conv, &tx, &CancellationToken::new()).await;
        drop(tx);
        let mut steps = 0;
        while let Some(e) = rx.recv().await {
            if matches!(e, AgentEvent::StepStarted { .. }) {
                steps += 1;
            }
        }
        let greeting = match conv.messages().last() {
            Some(Message::Assistant { text, .. }) => text.clone(),
            _ => String::new(),
        };
        println!("===== UI {locale}: {outcome:?}, tool steps: {steps}\n{greeting}\n");
        std::fs::remove_dir_all(&project)?;
    }
    Ok(())
}
