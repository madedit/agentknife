//! Live check of the reply-language rule: the reply follows the language of the
//! latest message even after a long conversation in another language. Manual check
//! only; not part of CI.
//!
//! ```text
//! AK_BASE_URL=… AK_MODEL=… [AK_PROTOCOL=anthropic] cargo run -p ak-agent --example agent_language_live
//! ```

use std::error::Error;
use std::sync::Arc;

use ak_agent::tool::ToolRegistry;
use ak_agent::{Agent, AgentConfig};
use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::chat::{Conversation, Message};
use ak_types::i18n::Locale;
use tokio_util::sync::CancellationToken;

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let base = std::env::var("AK_BASE_URL").map_err(|_| "set AK_BASE_URL")?;
    let model = std::env::var("AK_MODEL").map_err(|_| "set AK_MODEL")?;
    let transport = Arc::new(ReqwestTransport::new()?);
    let provider: Arc<dyn Provider> = if std::env::var("AK_PROTOCOL").as_deref() == Ok("anthropic")
    {
        let mut c = AnthropicConfig::new("local");
        c.base_url = base;
        Arc::new(Anthropic::new(c, transport))
    } else {
        Arc::new(OpenAiCompat::new(
            OpenAiCompatConfig::new(base, None),
            transport,
        ))
    };
    let mut config = AgentConfig::new(model);
    config.max_tokens = 1_024;
    config.ui_locale = Locale::ZhTw;
    config.system =
        Some("You are AgentKnife, a coding helper. Keep answers to one or two sentences.".into());
    let agent = Agent::new(provider, ToolRegistry::new(), config);

    // (message, expects Chinese)
    let turns = [
        ("你好，我想做一個待辦清單 App", true),
        ("它可以有深色模式嗎？", true),
        ("What colors would you suggest for the buttons?", false),
        ("好，那就用你建議的顏色", true),
    ];
    let mut failures = 0;
    for round in 1..=3 {
        let mut conv = Conversation::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        for (msg, zh) in turns {
            agent
                .run_turn(
                    &mut conv,
                    Message::user_text(msg),
                    &tx,
                    &CancellationToken::new(),
                )
                .await;
            let reply = match conv.messages().last() {
                Some(Message::Assistant { text, .. }) => text.clone(),
                _ => String::new(),
            };
            let ok = has_cjk(&reply) == zh;
            if !ok {
                failures += 1;
            }
            let short: String = reply.chars().take(70).collect();
            println!(
                "[round {round}] {} {msg}\n          -> {}",
                if ok { "OK  " } else { "FAIL" },
                short.replace('\n', " ")
            );
        }
    }
    println!("language mismatches: {failures} / 12");
    Ok(())
}
