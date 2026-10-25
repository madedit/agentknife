//! The preview tools and the end-of-turn build check, against a fake preview.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ak_agent::permission::AllowAll;
use ak_agent::scripted::{ScriptedProvider, text, tool_calls};
use ak_agent::tool::{Tool, ToolContext, ToolOutput, ToolRegistry};
use ak_agent::tools::{PreviewAccess, with_preview_tools};
use ak_agent::{Agent, AgentConfig};
use ak_types::agent::{AgentEvent, TurnOutcome};
use ak_types::chat::{ContentPart, Conversation, Message, ToolSpec};
use ak_types::preview::{
    ControlAction, PAGE_CLOSED, PAGE_RELOADED_STUCK, Picked, PreviewStatus, Rect, ServerState, Size,
};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct FakePreview {
    /// Returned by successive `settled` calls; the last one repeats.
    statuses: Mutex<Vec<PreviewStatus>>,
    changed: Mutex<Vec<Vec<String>>>,
    actions: Mutex<Vec<ControlAction>>,
    selection: Option<Picked>,
    capture: bool,
    crashes: Vec<String>,
    /// `settled` never answers (a build that takes very long).
    endless_build: bool,
}

fn ok() -> PreviewStatus {
    PreviewStatus {
        frontend: ServerState::Ready,
        backend: ServerState::Ready,
        url: Some("http://127.0.0.1:1/".into()),
    }
}

fn broken(msg: &str) -> PreviewStatus {
    PreviewStatus {
        frontend: ServerState::Error { output: msg.into() },
        ..ok()
    }
}

impl FakePreview {
    fn with(statuses: Vec<PreviewStatus>) -> Arc<Self> {
        Arc::new(Self {
            statuses: Mutex::new(statuses),
            ..Self::default()
        })
    }

    fn next(&self) -> PreviewStatus {
        let mut s = self.statuses.lock().unwrap();
        if s.len() > 1 {
            s.remove(0)
        } else {
            s.first().cloned().unwrap_or_default()
        }
    }
}

impl PreviewAccess for FakePreview {
    fn status(&self) -> PreviewStatus {
        self.statuses
            .lock()
            .unwrap()
            .first()
            .cloned()
            .unwrap_or_default()
    }

    fn settled(&self, _: Duration) -> BoxFuture<'_, PreviewStatus> {
        if self.endless_build {
            return Box::pin(std::future::pending());
        }
        let s = self.next();
        Box::pin(async move { s })
    }

    fn files_changed<'a>(&'a self, paths: &'a [String]) -> BoxFuture<'a, ()> {
        self.changed.lock().unwrap().push(paths.to_vec());
        Box::pin(async {})
    }

    fn act(&self, action: ControlAction) -> BoxFuture<'_, Result<Value, String>> {
        let out = match &action {
            ControlAction::DomSummary => {
                Ok(json!({ "title": "My App", "url": "http://x/", "items": [
                    { "depth": 0, "tag": "h1", "ak_id": "home-title", "text": "My App" },
                    { "depth": 0, "tag": "button", "ak_id": "greet-button", "text": "Greet" }
                ]}))
            }
            ControlAction::Click { ak_id } if ak_id == "missing" => {
                Err("no element with data-ak-id `missing` on the page".into())
            }
            ControlAction::Click { ak_id } if ak_id == "reloading" => Err(PAGE_CLOSED.into()),
            ControlAction::Click { ak_id } if ak_id == "frozen" => Err(PAGE_RELOADED_STUCK.into()),
            ControlAction::AppState => Ok(json!({ "url": "u", "title": "t", "local_storage": {} })),
            _ => Ok(json!({ "done": true })),
        };
        self.actions.lock().unwrap().push(action);
        Box::pin(async move { out })
    }

    fn selection(&self) -> Option<Picked> {
        self.selection.clone()
    }

    fn can_capture(&self) -> bool {
        self.capture
    }

    fn screenshot(&self, _: Option<String>) -> BoxFuture<'_, Result<ContentPart, String>> {
        Box::pin(async {
            Ok(ContentPart::Image {
                media_type: "image/png".into(),
                data_base64: "iVBORw0K".into(),
            })
        })
    }

    fn backend_crashes(&self) -> Vec<String> {
        self.crashes.clone()
    }
}

fn text_of(out: &ToolOutput) -> String {
    out.content
        .iter()
        .map(|c| match c {
            ContentPart::Text { text } => text.clone(),
            ContentPart::Image { .. } => "[image]".into(),
        })
        .collect()
}

async fn call(registry: &ToolRegistry, name: &str, args: Value) -> ToolOutput {
    let tool = registry
        .get(name)
        .unwrap_or_else(|| panic!("{name} not registered"));
    tool.call(args, &ToolContext::for_project(".")).await
}

#[tokio::test]
async fn preview_tools() {
    let fake = Arc::new(FakePreview {
        statuses: Mutex::new(vec![ok()]),
        selection: Some(Picked {
            ak_id: Some("greet-button".into()),
            exact: false,
            path: "form > button".into(),
            tag: "button".into(),
            text: Some("Greet".into()),
            rect: Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
            },
            viewport: Size {
                width: 800.0,
                height: 600.0,
            },
            canvas: None,
        }),
        capture: true,
        ..FakePreview::default()
    });
    let r = with_preview_tools(ToolRegistry::new(), fake.clone(), true);

    assert!(
        text_of(&call(&r, "build_status", json!({})).await).contains("The app builds and runs.")
    );
    let dom = text_of(&call(&r, "dom_summary", json!({})).await);
    assert!(dom.contains("button #greet-button \"Greet\""), "{dom}");

    let out = call(
        &r,
        "interact",
        json!({ "action": "type", "ak_id": "name-input", "text": "Ada", "enter": true }),
    )
    .await;
    assert!(!out.is_error);
    let out = call(
        &r,
        "interact",
        json!({ "action": "click", "ak_id": "missing" }),
    )
    .await;
    assert!(out.is_error && text_of(&out).contains("no element"));
    assert!(
        text_of(&out).contains("the page reloaded"),
        "a missing element may mean a fresh page: {}",
        text_of(&out)
    );
    let out = call(&r, "interact", json!({ "action": "click" })).await;
    assert!(out.is_error && text_of(&out).contains("`ak_id` is required"));
    assert!(
        text_of(&call(&r, "interact", json!({ "action": "read_state" })).await)
            .contains("local_storage")
    );
    // Watching over time: one reading per sample, spread over wait_ms.
    let before = fake.actions.lock().unwrap().len();
    let watched = text_of(
        &call(
            &r,
            "interact",
            json!({ "action": "read_state", "wait_ms": 40, "samples": 3 }),
        )
        .await,
    );
    assert!(
        watched.contains("\"after_ms\": 0") && watched.contains("\"after_ms\": 40"),
        "{watched}"
    );
    assert_eq!(fake.actions.lock().unwrap().len(), before + 3);
    assert_eq!(
        fake.actions.lock().unwrap()[1],
        ControlAction::Type {
            ak_id: "name-input".into(),
            text: "Ada".into(),
            enter: true
        }
    );

    // Scrolling a list down; scrolling with nothing to scroll is refused.
    call(
        &r,
        "interact",
        json!({ "action": "scroll", "ak_id": "city-list", "by": 400 }),
    )
    .await;
    assert_eq!(
        fake.actions.lock().unwrap().last(),
        Some(&ControlAction::Scroll {
            ak_id: Some("city-list".into()),
            by: Some(400)
        })
    );
    assert!(
        call(&r, "interact", json!({ "action": "scroll" }))
            .await
            .is_error
    );

    // Dragging a card onto a column; without `to` it is refused.
    call(
        &r,
        "interact",
        json!({ "action": "drag", "ak_id": "card-1", "to": "column-done" }),
    )
    .await;
    assert_eq!(
        fake.actions.lock().unwrap().last(),
        Some(&ControlAction::Drag {
            ak_id: "card-1".into(),
            to: "column-done".into()
        })
    );
    let out = call(
        &r,
        "interact",
        json!({ "action": "drag", "ak_id": "card-1" }),
    )
    .await;
    assert!(out.is_error && text_of(&out).contains("drag needs `to`"));

    let sel = text_of(&call(&r, "get_selection", json!({})).await);
    assert!(
        sel.contains("greet-button") && sel.contains("has no data-ak-id of its own"),
        "{sel}"
    );

    let shot = call(&r, "screenshot_preview", json!({ "ak_id": "greet-button" })).await;
    assert!(matches!(shot.content[0], ContentPart::Image { .. }));

    // No vision, or no capture: no screenshot tool (dom_summary instead).
    assert!(
        with_preview_tools(ToolRegistry::new(), fake.clone(), false)
            .get("screenshot_preview")
            .is_none()
    );
    let blind = Arc::new(FakePreview {
        statuses: Mutex::new(vec![ok()]),
        ..FakePreview::default()
    });
    assert!(
        with_preview_tools(ToolRegistry::new(), blind, true)
            .get("screenshot_preview")
            .is_none()
    );

    // A broken build is shown with the compiler output; page tools say why they cannot look.
    let failing = FakePreview::with(vec![PreviewStatus {
        frontend: ServerState::Error {
            output: "error[E0308]: mismatched types".into(),
        },
        backend: ServerState::Ready,
        url: None,
    }]);
    let r = with_preview_tools(ToolRegistry::new(), failing, false);
    let status = text_of(&call(&r, "build_status", json!({})).await);
    assert!(
        status.contains("Screens: FAILED") && status.contains("error[E0308]"),
        "{status}"
    );
    let out = call(&r, "dom_summary", json!({})).await;
    assert!(out.is_error && text_of(&out).contains("not showing the app yet"));
}

/// Writes a file (reports it as touched).
struct Write(&'static str);

impl Tool for Write {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".into(),
            description: String::new(),
            input_schema: json!({ "type": "object" }),
        }
    }
    fn changes_project(&self) -> bool {
        true
    }
    fn call<'a>(&'a self, _: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        let out = ToolOutput {
            touched: vec![self.0.into()],
            ..ToolOutput::text("written")
        };
        Box::pin(async move { out })
    }
}

async fn turn(agent: &Agent, request: &str) -> (TurnOutcome, Vec<AgentEvent>, Conversation) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut c = Conversation::new();
    let outcome = agent
        .run_turn(
            &mut c,
            Message::user_text(request),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (outcome, events, c)
}

fn agent(
    p: Arc<ScriptedProvider>,
    preview: Arc<FakePreview>,
    path: &'static str,
    max_fix: u32,
) -> Agent {
    let mut config = AgentConfig::new("m");
    config.max_fix_attempts = max_fix;
    Agent::new(p, ToolRegistry::new().with(Arc::new(Write(path))), config)
        .with_gate(Arc::new(AllowAll))
        .with_preview(preview)
}

fn build_events(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::BuildChecking => Some("checking".to_owned()),
            AgentEvent::BuildChecked { ok, attempt, max } => {
                Some(format!("ok={ok} {attempt}/{max}"))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn stop_does_not_wait_for_the_build_check() {
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", "{}")]),
        text("Done."),
    ]));
    let preview = Arc::new(FakePreview {
        statuses: Mutex::new(vec![ok()]),
        endless_build: true,
        ..FakePreview::default()
    });
    let a = agent(p, preview, "src/main.rs", 3);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        c2.cancel();
    });
    let mut c = Conversation::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        a.run_turn(&mut c, Message::user_text("make it blue"), &tx, &cancel),
    )
    .await
    .expect("the turn stops while the build check waits");
    assert_eq!(outcome, TurnOutcome::Cancelled);
}

#[tokio::test]
async fn a_broken_build_goes_back_to_the_model_until_fixed() {
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", "{}")]),
        text("Done."),
        tool_calls(&[("c2", "write_file", "{}")]),
        text("Fixed the type error."),
    ]));
    let preview = FakePreview::with(vec![
        broken("error[E0308]: mismatched types\n --> src/main.rs:9:5"),
        ok(),
    ]);
    let a = agent(p.clone(), preview.clone(), "src/main.rs", 3);
    let (outcome, events, conversation) = turn(&a, "make the button blue").await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(
        build_events(&events),
        ["checking", "ok=false 1/3", "checking", "ok=true 0/3"]
    );
    // The model saw the compiler output before its fix.
    let third = &p.requests()[2];
    let note = match third.messages.last().unwrap() {
        Message::User { content } => match &content[0] {
            ContentPart::Text { text } => text.clone(),
            ContentPart::Image { .. } => String::new(),
        },
        other => panic!("{other:?}"),
    };
    assert!(
        note.contains("no longer builds") && note.contains("src/main.rs:9:5"),
        "{note}"
    );
    // The note is in the saved history, so the user and later turns see why.
    assert!(conversation.to_json().unwrap().contains("no longer builds"));
    // Every write told the preview.
    assert_eq!(
        *preview.changed.lock().unwrap(),
        vec![vec!["src/main.rs".to_owned()]; 2]
    );
}

#[tokio::test]
async fn a_build_that_stays_broken_stops_the_turn() {
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", "{}")]),
        text("Done."),
        text("Tried."),
        text("Tried again."),
        text("never reached"),
    ]));
    let preview = FakePreview::with(vec![broken("error[E0599]: no method named `foo`")]);
    let a = agent(p.clone(), preview, "src-tauri/src/main.rs", 2);
    let (outcome, events, _) = turn(&a, "add saving").await;
    assert_eq!(
        outcome,
        TurnOutcome::Stuck {
            attempts: 2,
            last_error: "error[E0599]: no method named `foo`".into()
        }
    );
    assert_eq!(
        build_events(&events),
        [
            "checking",
            "ok=false 1/2",
            "checking",
            "ok=false 2/2",
            "checking"
        ]
    );
    assert_eq!(p.remaining(), 1);
}

#[tokio::test]
async fn no_check_without_changes_or_without_a_running_preview() {
    let p = Arc::new(ScriptedProvider::new(vec![text("Hello!")]));
    let preview = FakePreview::with(vec![broken("old error")]);
    let a = agent(p, preview.clone(), "src/main.rs", 3);
    let (outcome, events, _) = turn(&a, "hi").await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert!(
        build_events(&events).is_empty(),
        "nothing changed: no build check"
    );
    assert!(preview.changed.lock().unwrap().is_empty());

    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", "{}")]),
        text("Done."),
    ]));
    let off = FakePreview::with(vec![PreviewStatus::default()]);
    let a = agent(p, off, "src/main.rs", 3);
    let (outcome, events, _) = turn(&a, "edit").await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert!(
        build_events(&events).is_empty(),
        "preview not running: no check"
    );
}

#[tokio::test]
async fn the_id_pass_runs_on_the_changed_files_before_the_build_check() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "<button>").unwrap();
    let p = Arc::new(ScriptedProvider::new(vec![
        tool_calls(&[("c1", "write_file", "{}")]),
        text("Done."),
    ]));
    let preview = FakePreview::with(vec![ok()]);
    let mut config = AgentConfig::new("m");
    config.project_dir = Some(dir.path().to_owned());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let a = Agent::new(
        p,
        ToolRegistry::new().with(Arc::new(Write("src/main.rs"))),
        config,
    )
    .with_gate(Arc::new(AllowAll))
    .with_preview(preview.clone())
    .with_id_pass(Arc::new(move |rel: &str, source: &str| {
        log.lock().unwrap().push(rel.to_owned());
        (!source.contains("data-ak-id")).then(|| source.replace(">", " data-ak-id=\"x\">"))
    }));
    let (outcome, events, _) = turn(&a, "add a button").await;
    assert_eq!(outcome, TurnOutcome::Completed);
    assert_eq!(*seen.lock().unwrap(), ["src/main.rs"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
        "<button data-ak-id=\"x\">"
    );
    // The write and then the id pass told the preview; the build was checked after.
    assert_eq!(
        *preview.changed.lock().unwrap(),
        vec![vec!["src/main.rs".to_owned()]; 2]
    );
    assert_eq!(build_events(&events), ["checking", "ok=true 0/3"]);
}

#[tokio::test]
async fn holding_pressing_keys_and_reloads() {
    let fake = FakePreview::with(vec![ok()]);
    let r = with_preview_tools(ToolRegistry::new(), fake.clone(), false);
    for args in [
        json!({ "action": "hold", "ak_id": "left-button" }),
        json!({ "action": "hold", "ak_id": "left-button", "ms": 99_999 }),
        json!({ "action": "key", "key": "ArrowLeft" }),
        json!({ "action": "key", "key": " ", "ms": 300 }),
        // What models send for space: quoted, or by name.
        json!({ "action": "key", "key": "\" \"" }),
        json!({ "action": "key", "key": "Space" }),
    ] {
        let out = call(&r, "interact", args).await;
        assert!(!out.is_error, "{}", text_of(&out));
    }
    let hold = |ms| ControlAction::Hold {
        ak_id: "left-button".into(),
        ms,
    };
    let key = |k: &str, ms| ControlAction::Key { key: k.into(), ms };
    assert_eq!(
        *fake.actions.lock().unwrap(),
        [
            hold(1000),
            hold(10_000),
            key("ArrowLeft", 0),
            key(" ", 300),
            key(" ", 0),
            key(" ", 0)
        ],
        "defaults, and holds are capped"
    );
    let out = call(
        &r,
        "interact",
        json!({ "action": "click", "ak_id": "frozen" }),
    )
    .await;
    assert!(
        out.is_error
            && text_of(&out).contains("endless loop")
            && text_of(&out).contains("started over"),
        "{}",
        text_of(&out)
    );
    let out = call(&r, "interact", json!({ "action": "key" })).await;
    assert!(out.is_error && text_of(&out).contains("`key` is required"));
    let out = call(&r, "interact", json!({ "action": "hold" })).await;
    assert!(out.is_error && text_of(&out).contains("`ak_id` is required"));

    // A rebuild reloads the page mid-action: say what to do next.
    let out = call(
        &r,
        "interact",
        json!({ "action": "click", "ak_id": "reloading" }),
    )
    .await;
    assert!(
        out.is_error && text_of(&out).contains("dom_summary"),
        "{}",
        text_of(&out)
    );
}

#[tokio::test]
async fn backend_crashes_reach_the_agent() {
    let fake = Arc::new(FakePreview {
        statuses: Mutex::new(vec![ok()]),
        crashes: vec![
            "thread 'main' panicked at src/main.rs:12:5:
called `Option::unwrap()` on a `None` value"
                .into(),
        ],
        ..FakePreview::default()
    });
    let r = with_preview_tools(ToolRegistry::new(), fake, false);
    for tool in ["build_status", "dom_summary"] {
        let text = text_of(&call(&r, tool, json!({})).await);
        assert!(
            text.contains("The backend crashed")
                && text.contains("src/main.rs:12:5")
                && text.contains("on a `None` value"),
            "{tool}: {text}"
        );
    }
    let quiet = with_preview_tools(ToolRegistry::new(), FakePreview::with(vec![ok()]), false);
    let text = text_of(&call(&quiet, "build_status", json!({})).await);
    assert!(!text.contains("crashed"), "{text}");
}
