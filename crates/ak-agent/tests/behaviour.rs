//! Behaviour rules (P1-AGENT-004) against the scripted fake LLM: plan before big
//! changes, auto-fix limit, reply language.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use ak_agent::permission::AllowAll;
use ak_agent::plan::PlanReviewer;
use ak_agent::prompt::{PLAN_TOOL, PlanMode};
use ak_agent::scripted::{Script, ScriptedProvider, args, text, tool_calls};
use ak_agent::tool::{Tool, ToolContext, ToolOutput, ToolRegistry};
use ak_agent::{Agent, AgentConfig};
use ak_types::agent::{AgentEvent, PlanDecision, PlanProposal, TurnOutcome};
use ak_types::chat::{ContentPart, Conversation, Message, ToolSpec};
use ak_types::i18n::Locale;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// A tool whose results are scripted: each call pops the next (ok, text).
struct Scripted {
    name: &'static str,
    changes: bool,
    results: Mutex<Vec<(bool, &'static str)>>,
    calls: Mutex<usize>,
}

impl Scripted {
    fn new(name: &'static str, changes: bool, results: Vec<(bool, &'static str)>) -> Arc<Self> {
        Arc::new(Self {
            name,
            changes,
            results: Mutex::new(results),
            calls: Mutex::new(0),
        })
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl Tool for Scripted {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.into(),
            description: String::new(),
            input_schema: json!({ "type": "object" }),
        }
    }

    fn changes_project(&self) -> bool {
        self.changes
    }

    fn call<'a>(&'a self, _: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        *self.calls.lock().unwrap() += 1;
        let (ok, text) = {
            let mut r = self.results.lock().unwrap();
            if r.is_empty() {
                (true, "ok")
            } else {
                r.remove(0)
            }
        };
        Box::pin(async move {
            if ok {
                ToolOutput::text(text)
            } else {
                ToolOutput::error(text)
            }
        })
    }
}

struct Review(Mutex<Vec<PlanDecision>>, Mutex<Vec<PlanProposal>>);

impl Review {
    fn new(decisions: Vec<PlanDecision>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(decisions), Mutex::new(Vec::new())))
    }
}

impl PlanReviewer for Review {
    fn review(&self, _: String, plan: PlanProposal) -> BoxFuture<'_, PlanDecision> {
        self.1.lock().unwrap().push(plan);
        let d = self.0.lock().unwrap().remove(0);
        Box::pin(async move { d })
    }
}

fn plan_call() -> String {
    args(json!({
        "title": "會員登入",
        "steps": ["建立登入頁面", "加上帳號密碼檢查", "記住登入狀態"],
        "estimate": "大約 10 分鐘",
    }))
}

async fn run(agent: &Agent, conv: &mut Conversation, user: &str) -> (TurnOutcome, Vec<AgentEvent>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = agent
        .run_turn(
            conv,
            Message::user_text(user),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (outcome, events)
}

fn result_text(m: &Message) -> String {
    match m {
        Message::ToolResult { content, .. } => match &content[0] {
            ContentPart::Text { text } => text.clone(),
            _ => String::new(),
        },
        other => panic!("not a tool result: {other:?}"),
    }
}

fn agent_with(p: Arc<ScriptedProvider>, tools: ToolRegistry, config: AgentConfig) -> Agent {
    Agent::new(p, tools, config).with_gate(Arc::new(AllowAll))
}

#[tokio::test]
async fn plan_is_shown_and_work_starts_after_approval() {
    let plan = plan_call();
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", PLAN_TOOL, &plan)]),
        tool_calls(&[("c2", "write_file", "{}")]),
        text("完成了"),
    ]));
    let write = Scripted::new("write_file", true, vec![]);
    let review = Review::new(vec![PlanDecision::Start]);
    let a = agent_with(
        p.clone(),
        ToolRegistry::new().with(write.clone()),
        AgentConfig::new("m"),
    )
    .with_plan_reviewer(review.clone());
    let mut conv = Conversation::new();
    let (outcome, events) = run(&a, &mut conv, "加一個會員登入").await;

    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(write.calls(), 1);
    let shown = review.1.lock().unwrap();
    assert_eq!(shown[0].steps.len(), 3);
    assert_eq!(shown[0].estimate.as_deref(), Some("大約 10 分鐘"));
    assert!(
        events.iter().any(
            |e| matches!(e, AgentEvent::PlanProposed { plan, .. } if plan.title == "會員登入")
        )
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AgentEvent::StepStarted { tool, .. } if tool == PLAN_TOOL))
    );
    assert!(result_text(&p.requests()[1].messages[2]).contains("approved"));
    assert!(p.requests()[0].tools.iter().any(|t| t.name == PLAN_TOOL));
}

#[tokio::test]
async fn revise_and_cancel_reach_the_model() {
    let plan = plan_call();
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", PLAN_TOOL, &plan)]),
        tool_calls(&[("c2", PLAN_TOOL, &plan)]),
        text("好，不改了"),
    ]));
    let write = Scripted::new("write_file", true, vec![]);
    let review = Review::new(vec![
        PlanDecision::Revise {
            feedback: "先不要記住登入狀態".into(),
        },
        PlanDecision::Cancel,
    ]);
    let a = agent_with(
        p.clone(),
        ToolRegistry::new().with(write.clone()),
        AgentConfig::new("m"),
    )
    .with_plan_reviewer(review);
    let mut conv = Conversation::new();
    let (outcome, _) = run(&a, &mut conv, "加一個會員登入").await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(write.calls(), 0);
    let reqs = p.requests();
    assert!(result_text(&reqs[1].messages[2]).contains("先不要記住登入狀態"));
    assert!(result_text(&reqs[2].messages[4]).contains("cancelled"));
}

#[tokio::test]
async fn always_ask_blocks_changes_until_a_plan_is_approved() {
    let plan = plan_call();
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", "{}"), ("c2", "read_file", "{}")]),
        tool_calls(&[("c3", PLAN_TOOL, &plan)]),
        tool_calls(&[("c4", "write_file", "{}")]),
        text("done"),
    ]));
    let write = Scripted::new("write_file", true, vec![]);
    let read = Scripted::new("read_file", false, vec![]);
    let mut config = AgentConfig::new("m");
    config.plan_mode = PlanMode::AlwaysAsk;
    let a = agent_with(
        p.clone(),
        ToolRegistry::new().with(write.clone()).with(read.clone()),
        config,
    )
    .with_plan_reviewer(Review::new(vec![PlanDecision::Start]));
    let mut conv = Conversation::new();
    let (outcome, _) = run(&a, &mut conv, "change it").await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(read.calls(), 1, "reading needs no plan");
    assert_eq!(write.calls(), 1, "only the write after approval ran");
    assert!(result_text(&p.requests()[1].messages[2]).contains(PLAN_TOOL));
}

#[tokio::test]
async fn never_ask_offers_no_plan_tool() {
    let p = Arc::new(ScriptedProvider::new(vec![text("ok")]));
    let mut config = AgentConfig::new("m");
    config.plan_mode = PlanMode::NeverAsk;
    let a = agent_with(p.clone(), ToolRegistry::new(), config);
    run(&a, &mut Conversation::new(), "hi").await;
    assert!(p.requests()[0].tools.iter().all(|t| t.name != PLAN_TOOL));
    assert!(!a.system_prompt().contains(PLAN_TOOL));
}

fn failing_builds(n: usize) -> Vec<Script> {
    (0..n)
        .map(|i| {
            let id = format!("b{i}");
            let id: &'static str = Box::leak(id.into_boxed_str());
            tool_calls(&[(id, "run_command", "{}")])
        })
        .collect()
}

#[tokio::test]
async fn auto_fix_stops_after_the_limit() {
    // 1 failure + 3 fix attempts = 4 failed builds, then stop (no 5th model call).
    let mut scripts = failing_builds(4);
    scripts.push(text("never reached"));
    let p = Arc::new(ScriptedProvider::new(scripts));
    let build = Scripted::new(
        "run_command",
        true,
        vec![
            (false, "error[E0425]: cannot find value `x`"),
            (false, "error[E0308]: mismatched types"),
            (false, "error[E0308]: mismatched types"),
            (false, "error[E0599]: no method named `foo`"),
        ],
    );
    let a = agent_with(
        p.clone(),
        ToolRegistry::new().with(build.clone()),
        AgentConfig::new("m"),
    );
    let (outcome, _) = run(&a, &mut Conversation::new(), "build it").await;
    assert_eq!(
        outcome,
        TurnOutcome::Stuck {
            attempts: 3,
            last_error: "error[E0599]: no method named `foo`".into()
        }
    );
    assert_eq!(build.calls(), 4);
    assert_eq!(p.remaining(), 1, "the model was not called again");
}

#[tokio::test]
async fn a_successful_build_resets_the_count_and_limit_is_configurable() {
    let mut scripts = failing_builds(5);
    scripts.push(text("works now"));
    let p = Arc::new(ScriptedProvider::new(scripts));
    let build = Scripted::new(
        "run_command",
        true,
        vec![
            (false, "e1"),
            (false, "e2"),
            (true, "built"),
            (false, "e3"),
            (true, "built"),
        ],
    );
    let a = agent_with(p, ToolRegistry::new().with(build), AgentConfig::new("m"));
    let (outcome, _) = run(&a, &mut Conversation::new(), "build").await;
    assert_eq!(outcome, TurnOutcome::Completed);

    let p = Arc::new(ScriptedProvider::new(failing_builds(3)));
    let build = Scripted::new(
        "run_command",
        true,
        vec![(false, "e1"), (false, "e2"), (false, "e3")],
    );
    let mut config = AgentConfig::new("m");
    config.max_fix_attempts = 1;
    let a = agent_with(p, ToolRegistry::new().with(build.clone()), config);
    let (outcome, _) = run(&a, &mut Conversation::new(), "build").await;
    assert!(matches!(outcome, TurnOutcome::Stuck { attempts: 1, .. }));
    assert_eq!(build.calls(), 2);
}

#[tokio::test]
async fn language_rule_is_static_across_turns() {
    let p = Arc::new(ScriptedProvider::new(vec![text("你好"), text("hello")]));
    let mut config = AgentConfig::new("m");
    config.ui_locale = Locale::ZhTw;
    config.system = Some("You are AgentKnife.".into());
    let a = agent_with(p.clone(), ToolRegistry::new(), config);
    let mut conv = Conversation::new();
    run(&a, &mut conv, "你好").await;
    run(&a, &mut conv, "hello").await;
    let reqs = p.requests();
    let system = reqs[0].system.clone().unwrap();
    assert!(system.starts_with("You are AgentKnife."));
    assert!(system.contains("most recent message"));
    assert!(
        system.contains("Traditional Chinese (繁體中文)"),
        "UI-language fallback"
    );
    // Append-only: identical system prompt on every request of the conversation.
    assert_eq!(reqs[1].system.as_deref(), Some(system.as_str()));
}
