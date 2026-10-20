//! Context management and interruption recovery (P1-AGENT-005).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ak_agent::context::estimate_tokens;
use ak_agent::lock::TurnLock;
use ak_agent::permission::AllowAll;
use ak_agent::scripted::{ScriptedProvider, args, text, tool_calls};
use ak_agent::tool::{Tool, ToolContext, ToolOutput, ToolRegistry};
use ak_agent::{Agent, AgentConfig, Checkpointer, ConversationStore, TurnSummary};
use ak_providers::ChatRequest;
use ak_types::agent::{AgentEvent, TurnOutcome};
use ak_types::chat::{
    CONTEXT_PREFIX, ContentPart, Conversation, Entry, Message, SUMMARY_PREFIX, ToolSpec,
};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn first_text(m: &Message) -> String {
    match m {
        Message::User { content } | Message::ToolResult { content, .. } => match content.first() {
            Some(ContentPart::Text { text }) => text.clone(),
            _ => String::new(),
        },
        Message::Assistant { text, .. } => text.clone(),
    }
}

async fn run(agent: &Agent, conv: &mut Conversation, user: &str) -> (TurnOutcome, Vec<AgentEvent>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let o = agent
        .run_turn(
            conv,
            Message::user_text(user),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    drop(tx);
    let mut ev = Vec::new();
    while let Some(e) = rx.recv().await {
        ev.push(e);
    }
    (o, ev)
}

#[tokio::test]
async fn session_start_loads_project_notes_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agentknife/plan")).unwrap();
    std::fs::write(
        dir.path().join(".agentknife/PROJECT.md"),
        "# 待辦清單\n可以新增和刪除",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".agentknife/plan/STATUS.md"),
        "## Done\n- T-001",
    )
    .unwrap();
    let p = Arc::new(ScriptedProvider::new(vec![text("好的")]));
    let mut config = AgentConfig::new("m");
    config.project_dir = Some(dir.path().to_owned());
    let a = Agent::new(p.clone(), ToolRegistry::new(), config);
    let mut conv = Conversation::new();

    assert!(a.open_session(&mut conv).unwrap());
    assert!(
        !a.open_session(&mut conv).unwrap(),
        "unchanged notes are not repeated"
    );
    run(&a, &mut conv, "繼續").await;
    let first = first_text(&p.requests()[0].messages[0]);
    assert!(first.starts_with(CONTEXT_PREFIX));
    assert!(first.contains("# 待辦清單") && first.contains("- T-001"));

    // The AI updated the notes during the turn; the next session sees the new ones.
    std::fs::write(
        dir.path().join(".agentknife/PROJECT.md"),
        "# 待辦清單\n加上統計",
    )
    .unwrap();
    assert!(a.open_session(&mut conv).unwrap());
    assert!(a.system_prompt().contains(".agentknife/PROJECT.md"));
}

#[tokio::test]
async fn long_session_stays_under_the_window_and_history_is_kept() {
    const WINDOW: u32 = 2_000;
    let turns = 14;
    let p = Arc::new(
        ScriptedProvider::new(
            (0..turns)
                .map(|i| text(&format!("answer {i}: {}", "ok ".repeat(60))))
                .collect(),
        )
        .with_auto_summary("They are building a todo app step by step."),
    );
    let mut config = AgentConfig::new("m");
    config.context_window = Some(WINDOW);
    let a = Agent::new(p.clone(), ToolRegistry::new(), config);
    let mut conv = Conversation::new();
    for i in 0..turns {
        let (o, _) = run(
            &a,
            &mut conv,
            &format!("request {i}: {}", "please ".repeat(60)),
        )
        .await;
        assert_eq!(o, TurnOutcome::Completed);
    }

    let requests = p.requests();
    let work: Vec<&ChatRequest> = requests
        .iter()
        .filter(|r| !ScriptedProvider::is_compaction(r))
        .collect();
    let compactions = requests.len() - work.len();
    assert!(
        compactions >= 2,
        "expected several compactions, got {compactions}"
    );
    for r in &work {
        let size = estimate_tokens(&r.messages);
        assert!(
            size < u64::from(WINDOW),
            "a request of ~{size} tokens exceeded the window"
        );
    }
    // Append-only: between compactions each request extends the previous one.
    let mut boundaries = 0;
    for pair in work.windows(2) {
        let (a, b) = (&pair[0].messages, &pair[1].messages);
        if b.len() > a.len() && b[..a.len()] == a[..] {
            continue;
        }
        boundaries += 1;
        assert!(
            first_text(&b[0]).starts_with(SUMMARY_PREFIX),
            "prefix changed without a compaction"
        );
    }
    assert_eq!(boundaries, compactions);
    // The stored chat still has every request and answer.
    let users = conv
        .entries()
        .iter()
        .filter(|e| matches!(e, Entry::Message(Message::User { .. })))
        .count();
    assert_eq!(users, turns);
    assert!(
        conv.entries()
            .iter()
            .any(|e| matches!(e, Entry::Compaction { .. }))
    );
}

/// Writes instantly, or hangs forever (to simulate a crash mid-tool).
struct Fake {
    name: &'static str,
    hang: bool,
}

impl Tool for Fake {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.into(),
            description: String::new(),
            input_schema: json!({ "type": "object" }),
        }
    }

    fn call<'a>(&'a self, args: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        let hang = self.hang;
        let path = args["path"].as_str().unwrap_or("?").to_owned();
        Box::pin(async move {
            if hang {
                std::future::pending::<()>().await;
            }
            ToolOutput {
                touched: vec![path.clone()],
                ..ToolOutput::text(format!("wrote {path}"))
            }
        })
    }
}

#[derive(Default)]
struct MemStore(Mutex<Option<Conversation>>);

impl ConversationStore for MemStore {
    fn save(&self, c: &Conversation) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(c.clone());
        Ok(())
    }
}

struct Checkpoints(Mutex<Vec<TurnSummary>>);

impl Checkpointer for Checkpoints {
    fn checkpoint<'a>(
        &'a self,
        s: &'a TurnSummary,
    ) -> BoxFuture<'a, Result<Option<(String, String)>, String>> {
        self.0.lock().unwrap().push(s.clone());
        Box::pin(async { Ok(Some(("cp".into(), "done".into()))) })
    }
}

#[tokio::test]
async fn crashed_turn_is_resumed_from_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let write = args(json!({ "label": "寫入清單", "path": "src/list.rs" }));
    let slow = args(json!({ "label": "建置", "path": "src/app.rs" }));
    let store = Arc::new(MemStore::default());
    let tools = || {
        ToolRegistry::new()
            .with(Arc::new(Fake {
                name: "write_file",
                hang: false,
            }))
            .with(Arc::new(Fake {
                name: "run_command",
                hang: true,
            }))
    };
    let mut config = AgentConfig::new("m");
    config.project_dir = Some(dir.path().to_owned());

    // 1. The turn writes a file, then hangs in a command, and the app is killed.
    let p1 = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", &write)]),
        tool_calls(&[("c2", "run_command", &slow)]),
    ]));
    let a1 = Agent::new(p1, tools(), config.clone())
        .with_gate(Arc::new(AllowAll))
        .with_store(store.clone());
    let mut conv = Conversation::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let killed = tokio::time::timeout(
        Duration::from_millis(300),
        a1.run_turn(
            &mut conv,
            Message::user_text("加上刪除按鈕"),
            &tx,
            &CancellationToken::new(),
        ),
    )
    .await;
    assert!(killed.is_err(), "the turn was cut off");
    let lock = TurnLock::read(dir.path()).expect("the lock survives the crash");
    assert_eq!(lock.request, "加上刪除按鈕");
    assert_eq!(lock.touched_files, ["src/list.rs"]);

    // 2. Next launch: the saved conversation ends with an unanswered tool call.
    let mut conv = store.0.lock().unwrap().clone().unwrap();
    assert!(matches!(
        conv.messages().last(),
        Some(Message::Assistant { .. })
    ));
    let p2 = Arc::new(ScriptedProvider::new(vec![text("刪除按鈕做好了。")]));
    let checkpoints = Arc::new(Checkpoints(Mutex::new(Vec::new())));
    let a2 = Agent::new(p2.clone(), tools(), config)
        .with_gate(Arc::new(AllowAll))
        .with_store(store.clone())
        .with_checkpointer(checkpoints.clone());
    let outcome = a2
        .resume_turn(&mut conv, &tx, &CancellationToken::new())
        .await;
    assert_eq!(outcome, Some(TurnOutcome::Completed));

    let req = &p2.requests()[0].messages;
    let n = req.len();
    assert!(
        matches!(&req[n - 2], Message::ToolResult { call_id, is_error: true, .. } if call_id == "c2")
    );
    let note = first_text(&req[n - 1]);
    assert!(
        note.starts_with(CONTEXT_PREFIX)
            && note.contains("加上刪除按鈕")
            && note.contains("src/list.rs")
    );
    assert!(
        TurnLock::read(dir.path()).is_none(),
        "settled: lock released"
    );
    assert_eq!(
        checkpoints.0.lock().unwrap()[0].touched_files,
        ["src/list.rs"]
    );
    assert!(
        a2.resume_turn(&mut conv, &tx, &CancellationToken::new())
            .await
            .is_none(),
        "nothing left to resume"
    );
    // Everything, including the repair and the note, was saved.
    assert_eq!(store.0.lock().unwrap().as_ref().unwrap(), &conv);
}
