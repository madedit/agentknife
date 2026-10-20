//! Agent loop against the scripted fake LLM.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use ak_agent::lock::TurnLock;
use ak_agent::permission::{AllowAll, ConfirmRequest, Confirmer, Decision, PermissionGate};
use ak_agent::scripted::{Script, ScriptedProvider, args, text, tool_calls};
use ak_agent::tool::{Tool, ToolContext, ToolOutput, ToolRegistry};
use ak_agent::{Agent, AgentConfig, Checkpointer, TurnSummary};
use ak_providers::{ProviderError, StopReason, StreamEvent};
use ak_types::agent::{AgentEvent, ConfirmKind, ConfirmReason, TurnOutcome};
use ak_types::chat::{ContentPart, Conversation, Entry, Message, ToolSpec};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// Records its calls; writes `touched` paths; can block until cancelled.
struct FakeTool {
    name: &'static str,
    calls: Mutex<Vec<Value>>,
    block: bool,
    /// Also reports whether `.agentknife/turn.lock` exists while running.
    saw_lock: Mutex<Option<bool>>,
}

impl FakeTool {
    fn new(name: &'static str) -> Arc<Self> {
        Arc::new(Self {
            name,
            calls: Mutex::new(Vec::new()),
            block: false,
            saw_lock: Mutex::new(None),
        })
    }

    fn blocking(name: &'static str) -> Arc<Self> {
        Arc::new(Self {
            block: true,
            ..Arc::into_inner(Self::new(name)).unwrap()
        })
    }
}

impl Tool for FakeTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.into(),
            description: format!("fake {}", self.name),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(args.clone());
            if let Some(dir) = &ctx.project_dir {
                *self.saw_lock.lock().unwrap() = Some(TurnLock::read(dir).is_some());
            }
            if self.block {
                ctx.cancel.cancelled().await;
            }
            let path = args["path"].as_str().unwrap_or("?").to_owned();
            ToolOutput {
                touched: vec![path.clone()],
                ..ToolOutput::text(format!("contents of {path}"))
            }
        })
    }
}

struct AskForWrites;

impl PermissionGate for AskForWrites {
    fn check(&self, tool: &str, _: &Value, _: &ToolContext) -> Decision {
        match tool {
            "write_file" => Decision::Ask {
                reason: ConfirmReason {
                    kind: ConfirmKind::Change,
                    detail: "index.html".into(),
                },
            },
            "delete_everything" => Decision::Deny {
                reason: ConfirmReason {
                    kind: ConfirmKind::Dangerous,
                    detail: "/".into(),
                },
            },
            _ => Decision::Allow,
        }
    }
}

struct Answer(bool, Mutex<Vec<ConfirmRequest>>);

impl Confirmer for Answer {
    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'_, bool> {
        self.1.lock().unwrap().push(request);
        let yes = self.0;
        Box::pin(async move { yes })
    }
}

struct RecordCheckpoints(Mutex<Vec<TurnSummary>>);

impl Checkpointer for RecordCheckpoints {
    fn checkpoint<'a>(
        &'a self,
        summary: &'a TurnSummary,
    ) -> BoxFuture<'a, Result<Option<(String, String)>, String>> {
        self.0.lock().unwrap().push(summary.clone());
        Box::pin(async { Ok(Some(("cp-1".to_owned(), "Read a file".to_owned()))) })
    }
}

async fn run(
    agent: &Agent,
    conv: &mut Conversation,
    user: &str,
    cancel: &CancellationToken,
) -> (TurnOutcome, Vec<AgentEvent>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = agent
        .run_turn(conv, Message::user_text(user), &tx, cancel)
        .await;
    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (outcome, events)
}

fn agent(provider: Arc<ScriptedProvider>, tools: ToolRegistry) -> Agent {
    Agent::new(provider, tools, AgentConfig::new("scripted")).with_gate(Arc::new(AllowAll))
}

fn read_call(id: &str, label: &str) -> (String, String) {
    (
        id.to_owned(),
        args(json!({ "label": label, "path": "src/app.rs" })),
    )
}

#[tokio::test]
async fn text_turn_completes() {
    let p = Arc::new(ScriptedProvider::new(vec![text(
        "Hi! What shall we build?",
    )]));
    let a = agent(p.clone(), ToolRegistry::new());
    let mut conv = Conversation::new();
    let (outcome, events) = run(&a, &mut conv, "hello", &CancellationToken::new()).await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(conv.messages().len(), 2);
    assert!(matches!(
        events.first(),
        Some(AgentEvent::TurnStarted { .. })
    ));
    assert!(events.contains(&AgentEvent::TextDelta {
        text: "Hi! What shall we build?".into()
    }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Usage { usage } if usage.input_tokens == 10))
    );
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            outcome: TurnOutcome::Completed
        })
    ));
}

#[tokio::test]
async fn tool_loop_with_ai_labels() {
    let (id, a1) = read_call("c1", "正在讀取 app.rs");
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[(&id, "read_file", &a1)]),
        text("It has an empty main."),
    ]));
    let read = FakeTool::new("read_file");
    let a = agent(p.clone(), ToolRegistry::new().with(read.clone()));
    let mut conv = Conversation::new();
    let (outcome, events) = run(
        &a,
        &mut conv,
        "what is in app.rs?",
        &CancellationToken::new(),
    )
    .await;

    assert_eq!(outcome, TurnOutcome::Completed);
    // The tool never sees the label; the UI does.
    assert_eq!(
        *read.calls.lock().unwrap(),
        [json!({ "path": "src/app.rs" })]
    );
    // "Details" shows the file, not the label.
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::StepStarted { tool, label, detail, .. }
            if tool == "read_file" && label == "正在讀取 app.rs" && detail == "src/app.rs")));
    assert!(events.iter().any(
        |e| matches!(e, AgentEvent::StepFinished { ok: true, output, .. } if !output.is_empty())
    ));

    let reqs = p.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        reqs[0].tools[0].input_schema["required"],
        json!(["label", "path"])
    );
    // Second call sees user, assistant(tool call), tool result.
    assert_eq!(reqs[1].messages.len(), 3);
    assert!(
        matches!(&reqs[1].messages[2], Message::ToolResult { call_id, is_error: false, .. } if call_id == "c1")
    );
    // Append-only: the first request's messages are a prefix of the second's.
    assert_eq!(
        reqs[1].messages[..reqs[0].messages.len()],
        reqs[0].messages[..]
    );
}

#[tokio::test]
async fn bad_calls_become_error_results_and_the_loop_continues() {
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[
            ("c1", "no_such_tool", "{}"),
            ("c2", "read_file", "{\"path\": \"cut off"),
        ]),
        text("Sorry, retrying differently."),
    ]));
    let read = FakeTool::new("read_file");
    let a = agent(p.clone(), ToolRegistry::new().with(read.clone()));
    let mut conv = Conversation::new();
    let (outcome, events) = run(&a, &mut conv, "go", &CancellationToken::new()).await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert!(
        read.calls.lock().unwrap().is_empty(),
        "invalid JSON is never executed"
    );
    let results: Vec<bool> = conv
        .messages()
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { is_error, .. } => Some(*is_error),
            _ => None,
        })
        .collect();
    assert_eq!(results, [true, true]);
    // Without a label the tool name is shown.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::StepStarted { label, .. } if label == "no_such_tool"))
    );
}

#[tokio::test]
async fn permission_ask_confirm_and_deny() {
    for yes in [true, false] {
        let write_args = args(json!({ "label": "Saving the page", "path": "index.html" }));
        let p = Arc::new(ScriptedProvider::new(vec![
            tool_calls(&[
                ("c1", "write_file", &write_args),
                ("c2", "delete_everything", "{\"path\": \"/\"}"),
            ]),
            text("ok"),
        ]));
        let write = FakeTool::new("write_file");
        let nuke = FakeTool::new("delete_everything");
        let answer = Arc::new(Answer(yes, Mutex::new(Vec::new())));
        let a = agent(
            p.clone(),
            ToolRegistry::new().with(write.clone()).with(nuke.clone()),
        )
        .with_gate(Arc::new(AskForWrites))
        .with_confirmer(answer.clone());
        let mut conv = Conversation::new();
        let (_, events) = run(&a, &mut conv, "save it", &CancellationToken::new()).await;
        assert!(events.iter().any(|e| matches!(e,
            AgentEvent::NeedsConfirmation { label, reason, .. } if label == "Saving the page" && reason.kind == ConfirmKind::Change && reason.detail == "index.html")));
        assert_eq!(answer.1.lock().unwrap().len(), 1);
        assert_eq!(write.calls.lock().unwrap().len(), usize::from(yes));
        assert!(
            nuke.calls.lock().unwrap().is_empty(),
            "denied tools never run"
        );
    }
}

#[tokio::test]
async fn cancel_during_stream_appends_nothing_and_keeps_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let p = Arc::new(ScriptedProvider::new(vec![Script::Stall(vec![
        StreamEvent::TextDelta("Let me th".into()),
    ])]));
    let mut config = AgentConfig::new("scripted");
    config.project_dir = Some(dir.path().to_owned());
    let a = Agent::new(p, ToolRegistry::new(), config).with_gate(Arc::new(AllowAll));
    let mut conv = Conversation::new();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        c2.cancel();
    });
    let (outcome, events) = run(&a, &mut conv, "build a todo app", &cancel).await;
    assert_eq!(outcome, TurnOutcome::Cancelled);
    assert!(events.contains(&AgentEvent::TextDelta {
        text: "Let me th".into()
    }));
    assert_eq!(conv.messages().len(), 1, "only the user message");
    let lock = TurnLock::read(dir.path()).expect("lock kept for recovery");
    assert_eq!(lock.request, "build a todo app");
}

#[tokio::test]
async fn cancel_during_tool_still_answers_every_call() {
    let (id1, a1) = read_call("c1", "Waiting");
    let (id2, a2) = read_call("c2", "Never runs");
    let p = Arc::new(ScriptedProvider::new(vec![tool_calls(&[
        (&id1, "slow", &a1),
        (&id2, "slow", &a2),
    ])]));
    let slow = FakeTool::blocking("slow");
    let a = agent(p, ToolRegistry::new().with(slow.clone()));
    let mut conv = Conversation::new();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        c2.cancel();
    });
    let (outcome, _) = run(&a, &mut conv, "go", &cancel).await;
    assert_eq!(outcome, TurnOutcome::Cancelled);
    assert_eq!(slow.calls.lock().unwrap().len(), 1);
    let results: Vec<String> = conv
        .messages()
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results, ["c1", "c2"], "history stays well-formed");
}

#[tokio::test]
async fn provider_errors_refusals_and_limits() {
    let p = Arc::new(ScriptedProvider::new(vec![Script::Fail(
        ProviderError::RateLimited {
            retry_after_secs: Some(3),
        },
    )]));
    let mut conv = Conversation::new();
    let (outcome, _) = run(
        &agent(p, ToolRegistry::new()),
        &mut conv,
        "x",
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        outcome,
        TurnOutcome::Failed {
            retryable: true,
            ..
        }
    ));

    let p = Arc::new(ScriptedProvider::new(vec![Script::Reply(vec![
        StreamEvent::TextDelta("Partial".into()),
        StreamEvent::Done {
            stop_reason: StopReason::Refusal {
                category: Some("cyber".into()),
            },
        },
    ])]));
    let mut conv = Conversation::new();
    let (outcome, _) = run(
        &agent(p, ToolRegistry::new()),
        &mut conv,
        "x",
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        outcome,
        TurnOutcome::Refused {
            category: Some("cyber".into())
        }
    );
    assert_eq!(
        conv.messages().len(),
        1,
        "refused partial output is discarded"
    );

    let (id, a1) = read_call("c", "again");
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[(&id, "read_file", &a1)]),
        tool_calls(&[(&id, "read_file", &a1)]),
    ]));
    let mut config = AgentConfig::new("scripted");
    config.max_steps = 2;
    let a = Agent::new(
        p,
        ToolRegistry::new().with(FakeTool::new("read_file")),
        config,
    );
    let mut conv = Conversation::new();
    let (outcome, _) = run(&a, &mut conv, "x", &CancellationToken::new()).await;
    assert_eq!(outcome, TurnOutcome::StepLimit);
}

#[tokio::test]
async fn completed_turn_checkpoints_then_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let (id, a1) = read_call("c1", "Reading");
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[(&id, "read_file", &a1)]),
        text("done"),
    ]));
    let read = FakeTool::new("read_file");
    let checkpoints = Arc::new(RecordCheckpoints(Mutex::new(Vec::new())));
    let mut config = AgentConfig::new("scripted");
    config.project_dir = Some(dir.path().to_owned());
    let a = Agent::new(p, ToolRegistry::new().with(read.clone()), config)
        .with_gate(Arc::new(AllowAll))
        .with_checkpointer(checkpoints.clone());
    let mut conv = Conversation::new();
    let (outcome, _) = run(&a, &mut conv, "read it", &CancellationToken::new()).await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(
        *read.saw_lock.lock().unwrap(),
        Some(true),
        "lock exists during the turn"
    );
    assert!(
        TurnLock::read(dir.path()).is_none(),
        "lock released after the checkpoint"
    );
    let cps = checkpoints.0.lock().unwrap();
    assert_eq!(cps[0].request, "read it");
    assert_eq!(cps[0].touched_files, ["src/app.rs"]);
    assert!(matches!(conv.entries().last(), Some(Entry::Checkpoint { id, .. }) if id == "cp-1"));
}

/// The default gate is the project policy: escapes are refused before the tool runs,
/// risky commands wait for the user, routine work proceeds.
#[tokio::test]
async fn default_policy_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let escape = args(json!({ "label": "Saving outside", "path": "../evil.txt" }));
    let build = args(json!({ "label": "Building", "command": "cargo build" }));
    let delete = args(json!({ "label": "Cleaning up", "command": "rm -rf dist" }));
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[
            ("c1", "write_file", &escape),
            ("c2", "run_command", &build),
            ("c3", "run_command", &delete),
        ]),
        text("done"),
    ]));
    let write = FakeTool::new("write_file");
    let run_cmd = FakeTool::new("run_command");
    let answer = Arc::new(Answer(false, Mutex::new(Vec::new())));
    let mut config = AgentConfig::new("scripted");
    config.project_dir = Some(dir.path().to_owned());
    let a = Agent::new(
        p.clone(),
        ToolRegistry::new()
            .with(write.clone())
            .with(run_cmd.clone()),
        config,
    )
    .with_confirmer(answer.clone());
    let mut conv = Conversation::new();
    let (outcome, events) = run(&a, &mut conv, "go", &CancellationToken::new()).await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert!(write.calls.lock().unwrap().is_empty(), "escape refused");
    assert_eq!(
        run_cmd.calls.lock().unwrap().len(),
        1,
        "build ran, delete declined"
    );
    let asked: Vec<ConfirmKind> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::NeedsConfirmation { reason, .. } => Some(reason.kind),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [ConfirmKind::Delete]);
    let refusal = match &p.requests()[1].messages[2] {
        Message::ToolResult {
            content,
            is_error: true,
            ..
        } => match &content[0] {
            ContentPart::Text { text } => text.clone(),
            _ => String::new(),
        },
        other => panic!("{other:?}"),
    };
    assert!(
        refusal.contains("Not allowed because the path is outside the project"),
        "{refusal}"
    );
}

#[tokio::test]
async fn images_in_user_messages_pass_through() {
    let p = Arc::new(ScriptedProvider::new(vec![text("A red button.")]));
    let a = agent(p.clone(), ToolRegistry::new());
    let mut conv = Conversation::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let user = Message::User {
        content: vec![
            ContentPart::Image {
                media_type: "image/png".into(),
                data_base64: "AAAA".into(),
            },
            ContentPart::Text {
                text: "What is this?".into(),
            },
        ],
    };
    a.run_turn(&mut conv, user.clone(), &tx, &CancellationToken::new())
        .await;
    assert_eq!(p.requests()[0].messages[0], user);
}
