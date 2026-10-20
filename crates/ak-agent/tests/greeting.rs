//! The first greeting after connecting an AI (P1-AGENT-007).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use ak_agent::prompt::product_prompt;
use ak_agent::scripted::{ScriptedProvider, text};
use ak_agent::tool::ToolRegistry;
use ak_agent::{Agent, AgentConfig};
use ak_types::agent::{AgentEvent, TurnOutcome};
use ak_types::chat::{CONTEXT_PREFIX, ContentPart, Conversation, Message};
use ak_types::i18n::Locale;
use tokio_util::sync::CancellationToken;

fn user_text(m: &Message) -> String {
    match m {
        Message::User { content } => match &content[0] {
            ContentPart::Text { text } => text.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

#[tokio::test]
async fn greets_once_in_the_ui_language_and_proposes_the_tool_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agentknife")).unwrap();
    std::fs::write(dir.path().join(".agentknife/PROJECT.md"), "# 新專案").unwrap();
    let p = Arc::new(ScriptedProvider::new(vec![text(
        "嗨！我是 AgentKnife。要先幫你檢查開發工具嗎？",
    )]));
    let mut config = AgentConfig::new("m");
    config.ui_locale = Locale::ZhTw;
    config.system = Some(product_prompt().to_owned());
    config.project_dir = Some(dir.path().to_owned());
    let a = Agent::new(p.clone(), ToolRegistry::new(), config);

    let mut conv = Conversation::new();
    // Project notes from the session start do not count as a started conversation.
    a.open_session(&mut conv).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = a.greet(&mut conv, &tx, &CancellationToken::new()).await;
    assert_eq!(outcome, Some(TurnOutcome::Completed));

    let req = &p.requests()[0];
    assert!(
        req.system
            .as_deref()
            .unwrap()
            .starts_with("You are AgentKnife")
    );
    let note = user_text(req.messages.last().unwrap());
    assert!(note.starts_with(CONTEXT_PREFIX));
    assert!(note.contains("Write in Traditional Chinese") && note.contains("development tools"));
    assert!(
        matches!(conv.messages().last(), Some(Message::Assistant { text, .. }) if text.contains("檢查開發工具"))
    );
    assert!(
        !dir.path().join(".agentknife/turn.lock").exists(),
        "a greeting takes no lock"
    );

    drop(tx);
    let mut texts = String::new();
    while let Some(e) = rx.recv().await {
        if let AgentEvent::TextDelta { text } = e {
            texts.push_str(&text);
        }
    }
    assert!(
        texts.contains("AgentKnife"),
        "the greeting streams to the UI"
    );

    // Only once: after that, the conversation has started.
    assert_eq!(
        a.greet(
            &mut conv,
            &tokio::sync::mpsc::unbounded_channel().0,
            &CancellationToken::new()
        )
        .await,
        None
    );
    assert_eq!(p.requests().len(), 1);
}
