//! Chat history files written by earlier versions must keep loading. Each
//! `fixtures/conversation_v<N>.json` is frozen: never edit one; add a new fixture
//! (and a migration in `Conversation::from_json`) when the schema changes.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use ak_project::Project;
use ak_types::chat::{ContentPart, Entry, Message};
use ak_types::i18n::Locale;

#[test]
fn v1_history_loads_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::init(dir.path(), "todo", Locale::ZhTw).unwrap();
    let chat = project.agentknife_dir().join("chat");
    std::fs::create_dir_all(&chat).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/conversation_v1.json");
    std::fs::copy(&fixture, chat.join("conversation.json")).unwrap();

    let loaded = project.load_conversation().unwrap();
    assert!(loaded.moved_aside.is_none());
    let c = loaded.conversation;
    assert_eq!(c.entries().len(), 5);
    assert!(
        matches!(&c.entries()[4], Entry::Checkpoint { label, .. } if label == "建立待辦清單畫面")
    );
    let messages = c.messages();
    assert_eq!(messages.len(), 4, "checkpoints are not sent to the model");
    match &messages[1] {
        Message::Assistant {
            thinking,
            tool_calls,
            replay,
            ..
        } => {
            assert_eq!(thinking[0].origin, "anthropic");
            assert_eq!(
                tool_calls[1].invalid_arguments.as_deref(),
                Some("{\"path\": \"src/cut")
            );
            assert_eq!(replay.as_ref().unwrap().content[0]["signature"], "sig-1");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &messages[3],
        Message::ToolResult { content, is_error: true, .. } if matches!(content[0], ContentPart::Image { .. })
    ));

    // Saving and loading again changes nothing.
    project.save_conversation(&c).unwrap();
    assert_eq!(project.load_conversation().unwrap().conversation, c);
}
