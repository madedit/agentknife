//! Live check of compaction: a fact from the first turn must survive after the
//! early history was summarized. Manual check only; not part of CI.
//!
//! ```text
//! AK_BASE_URL=… AK_MODEL=… [AK_PROTOCOL=anthropic] cargo run -p ak-agent --example agent_compact_live
//! ```

use std::error::Error;
use std::sync::Arc;

use ak_agent::tool::ToolRegistry;
use ak_agent::{Agent, AgentConfig};
use ak_providers::anthropic::{Anthropic, AnthropicConfig};
use ak_providers::openai::{OpenAiCompat, OpenAiCompatConfig};
use ak_providers::{Provider, ReqwestTransport};
use ak_types::agent::AgentEvent;
use ak_types::chat::{Conversation, Entry, Message};
use ak_types::i18n::Locale;
use tokio_util::sync::CancellationToken;

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
    // Small on purpose, so a few turns trigger compaction.
    config.context_window = Some(1_500);
    config.system = Some("You are AgentKnife. Answer in two or three sentences.".into());
    let agent = Agent::new(provider, ToolRegistry::new(), config);

    let turns = [
        "我的 App 叫做「小橘待辦」，主色是橘色 #FF8A00，請記住。",
        "待辦清單一般有哪些常見功能？請列出五個並各用一句話說明。",
        "深色模式要注意哪些設計重點？",
        "手機上的按鈕應該多大比較好按？",
        "怎麼讓清單項目可以拖曳排序？說明原理即可。",
        "完成的項目要用刪除線還是淡化比較好？比較一下優缺點。",
        "我的 App 叫什麼名字？主色是什麼？",
    ];
    let mut conv = Conversation::new();
    for (i, msg) in turns.iter().enumerate() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent
            .run_turn(
                &mut conv,
                Message::user_text(*msg),
                &tx,
                &CancellationToken::new(),
            )
            .await;
        drop(tx);
        let mut compacted = false;
        while let Some(e) = rx.recv().await {
            compacted |= matches!(e, AgentEvent::ContextCompacted);
            if let AgentEvent::Usage { usage } = e {
                println!(
                    "     usage: in={} cache_read={} out={}",
                    usage.input_tokens, usage.cache_read_tokens, usage.output_tokens
                );
            }
        }
        let reply = match conv.messages().last() {
            Some(Message::Assistant { text, .. }) => text.clone(),
            _ => String::new(),
        };
        let short: String = reply.chars().take(90).collect();
        println!(
            "[{}]{} {msg}\n     -> {}",
            i + 1,
            if compacted { " (compacted)" } else { "" },
            short.replace('\n', " ")
        );
    }
    for e in conv.entries() {
        if let Entry::Compaction { summary, keep_from } = e {
            let s: String = summary.chars().take(160).collect();
            println!("summary (keep_from {keep_from}): {}", s.replace('\n', " "));
        }
    }
    let reply = match conv.messages().last() {
        Some(Message::Assistant { text, .. }) => text.clone(),
        _ => String::new(),
    };
    println!(
        "remembers name: {} | remembers color: {}",
        reply.contains("小橘待辦"),
        reply.contains("FF8A00") || reply.contains("橘")
    );
    Ok(())
}
