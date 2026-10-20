//! P1-E2E-001: the first-use story through AgentKnife's real commands, with
//! Tauri's mock runtime instead of a window and a scripted AI: connect → tools
//! check → new project → greeting → a to-do app (add, delete, local storage)
//! → point at a button and change its color → package → install guidance.
//! Every step is checked through the events the window would get and the
//! project's files.
//!
//! It builds a real app with AgentKnife's toolchain (minutes), so `cargo test`
//! skips it: run `cargo xtask e2e`. The toolchain is the one in AgentKnife's
//! data folder, or `AK_E2E_TOOLCHAIN`. With `AK_E2E_PROVISION=1` a missing
//! toolchain is installed first (downloads; CI caches the folder).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ak_agent::scripted::{Script, ScriptedProvider, args, text, tool_calls};
use ak_app::state::AppState;
use ak_project::Registry;
use ak_types::agent::{AgentEvent, TurnOutcome};
use ak_types::i18n::Locale;
use ak_types::ipc::{AGENT_EVENT, BUNDLE_EVENT, BundleResult, DoctorCheck, PREVIEW_EVENT};
use ak_types::preview::PreviewStatus;
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context};
use tauri::webview::InvokeRequest;
use tauri::{Listener, WebviewWindow, WebviewWindowBuilder};

/// The scripted AI's to-do app (src/main.rs).
const TODO_APP: &str = include_str!("fixtures/todo/main.rs");

/// The template's serde line, and the dependencies the to-do app adds after it.
const SERDE_DEP: &str = r#"serde = { version = "1.0.229", features = ["derive"] }"#;
const STORAGE_DEPS: &str = r#"serde_json = "1.0.154"
web-sys = { version = "0.3.106", features = ["Window", "Storage"] }"#;

/// The add button before and after "make this green".
const ADD_BUTTON: &str = r#"<Button ak_id="add-button" submit=true>
                        "加入"
                    </Button>"#;
const GREEN_BUTTON: &str = r#"<button
                        type="submit"
                        data-ak-id="add-button"
                        class="rounded-lg bg-green-600 px-4 py-2 text-sm font-medium text-white hover:bg-green-700"
                    >
                        "加入"
                    </button>"#;

const GREETING: &str =
    "你好！我是 AgentKnife 的 AI 助手。先檢查一下開發工具，接著告訴我你想做什麼 App。";
const TODO_REQUEST: &str = "做一個待辦清單：可以新增和刪除項目，關掉再開資料還在。";
const TODO_DONE: &str =
    "待辦清單做好了：輸入要做的事按「加入」，每一項旁邊都有「刪除」，資料存在這台電腦上。";
const COLOR_REQUEST: &str = "把這個按鈕改成綠色";
const COLOR_DONE: &str = "「加入」按鈕改成綠色了。";

/// A cold build of the generated app (frontend and backend).
const BUILD_WAIT: Duration = Duration::from_secs(40 * 60);
const TURN_WAIT: Duration = Duration::from_secs(20 * 60);

fn scripts() -> Vec<Script> {
    vec![
        // Greeting.
        text(GREETING),
        // The to-do app.
        tool_calls(&[
            (
                "c1",
                "write_file",
                &args(json!({ "path": "src/main.rs", "content": TODO_APP })),
            ),
            (
                "c2",
                "edit_file",
                &args(json!({
                    "path": "Cargo.toml",
                    "old_text": SERDE_DEP,
                    "new_text": format!("{SERDE_DEP}\n{STORAGE_DEPS}"),
                })),
            ),
        ]),
        text(TODO_DONE),
        // The selected button turns green.
        tool_calls(&[(
            "c3",
            "edit_file",
            &args(json!({
                "path": "src/main.rs",
                "old_text": ADD_BUTTON,
                "new_text": GREEN_BUTTON,
            })),
        )]),
        text(COLOR_DONE),
    ]
}

/// AgentKnife without a window: its commands on the mock runtime, and every
/// event it sends to the window.
struct Harness {
    app: tauri::App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    events: Arc<Mutex<Vec<(&'static str, Value)>>>,
}

impl Harness {
    fn new(state: AppState) -> Harness {
        let app = ak_app::with_commands(mock_builder(), state)
            .build(mock_context(tauri::test::noop_assets()))
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        for name in [AGENT_EVENT, PREVIEW_EVENT, BUNDLE_EVENT] {
            let events = events.clone();
            app.listen_any(name, move |e| {
                let payload = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
                events.lock().unwrap().push((name, payload));
            });
        }
        let window = WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        Harness {
            app,
            window,
            events,
        }
    }

    /// Calls a command as the window does (`{ request }`).
    fn call(&self, cmd: &str, request: Option<Value>) -> Result<Value, Value> {
        let url = if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        let body = request
            .map(|r| json!({ "request": r }))
            .unwrap_or(json!({}));
        get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: url.parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map(|b| b.deserialize::<Value>().unwrap())
    }

    fn ok<T: serde::de::DeserializeOwned>(&self, cmd: &str, request: Option<Value>) -> T {
        let v = self
            .call(cmd, request)
            .unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
        serde_json::from_value(v).unwrap_or_else(|e| panic!("{cmd}: {e}"))
    }

    /// The agent events so far.
    fn agent_events(&self) -> Vec<AgentEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| *name == AGENT_EVENT)
            .filter_map(|(_, v)| serde_json::from_value(v.clone()).ok())
            .collect()
    }

    fn count(&self, name: &str) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| *n == name)
            .count()
    }

    /// Waits for the turn after the first `from` agent events to finish;
    /// returns its events.
    fn turn(&self, from: usize, what: &str) -> Vec<AgentEvent> {
        let started = Instant::now();
        loop {
            let events = self.agent_events();
            let turn = events.get(from..).unwrap_or_default();
            if turn
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnFinished { .. }))
            {
                return turn.to_vec();
            }
            assert!(
                started.elapsed() < TURN_WAIT,
                "{what}: the turn did not finish: {turn:#?}"
            );
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits until the preview can be used; returns its status.
    fn preview_ready(&self, what: &str) -> PreviewStatus {
        let started = Instant::now();
        loop {
            let status: PreviewStatus = self.ok("preview_status", None);
            if status.ready() {
                return status;
            }
            assert!(
                status.error().is_none(),
                "{what}: the preview failed: {status:#?}"
            );
            assert!(
                started.elapsed() < BUILD_WAIT,
                "{what}: the preview is not ready: {status:#?}"
            );
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

impl Drop for Harness {
    /// Stops the preview's dev servers, also when a step failed.
    fn drop(&mut self) {
        ak_app::close_project(self.app.handle());
    }
}

fn completed(turn: &[AgentEvent]) -> bool {
    turn.iter().any(|e| {
        matches!(
            e,
            AgentEvent::TurnFinished {
                outcome: TurnOutcome::Completed
            }
        )
    })
}

fn build_ok(turn: &[AgentEvent]) -> bool {
    turn.iter()
        .any(|e| matches!(e, AgentEvent::BuildChecked { ok: true, .. }))
}

fn says(turn: &[AgentEvent], words: &str) -> bool {
    let text: String = turn
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    text.contains(words)
}

/// Whether the dev server answers `url` with 200.
fn page_answers(url: &str) -> bool {
    let rest = url.trim_start_matches("http://");
    let host = rest.split('/').next().unwrap_or_default();
    let Ok(mut s) = std::net::TcpStream::connect(host) else {
        return false;
    };
    let _ = s.write_all(b"GET / HTTP/1.0\r\n\r\n");
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    // The server answers in the request's HTTP version.
    let reply = String::from_utf8_lossy(&buf);
    let status = reply.lines().next().unwrap_or_default();
    status.starts_with("HTTP/") && status.split(' ').nth(1) == Some("200")
}

fn toolchain_root() -> PathBuf {
    std::env::var_os("AK_E2E_TOOLCHAIN")
        .map(PathBuf::from)
        .unwrap_or_else(ak_doctor::Toolchain::default_root)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
#[ignore = "builds and packages a real app (minutes): cargo xtask e2e"]
fn first_use_story_with_a_scripted_ai() {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("Projects");
    let registry = Registry::load(&dir.path().join("state"), &projects).unwrap();
    let mut state = AppState::new(dir.path().join("state"), registry, toolchain_root()).unwrap();
    let ai = Arc::new(ScriptedProvider::new(scripts()));
    state.provider = Some(ai.clone());
    ak_app::set_language(Some(Locale::ZhTw));
    let h = Harness::new(state);

    // Connect: the connection is saved (no key) and the home screen shows it.
    h.call(
        "save_connection",
        Some(json!({ "connection": { "provider": "ollama", "base_url": "http://localhost:11434/v1", "model": "scripted" } })),
    )
    .unwrap();
    let home: Value = h.ok("home_state", None);
    assert_eq!(home["ai"]["model"], "scripted", "{home}");
    assert!(
        dir.path().join("state").join("settings.json").exists(),
        "the connection is saved"
    );

    // Tools check: everything in place (installed first in CI).
    let mut check: DoctorCheck = h.ok("doctor_check", None);
    if !check.report.ready() && std::env::var_os("AK_E2E_PROVISION").is_some() {
        check = h.ok("doctor_install", None);
    }
    assert!(
        check.report.ready(),
        "the toolchain in {} is not ready (AK_E2E_PROVISION=1 installs it): {:#?}",
        toolchain_root().display(),
        check.report
    );

    // New project: the starter app, its preview starting, then the greeting.
    let opened: Value = h.ok("create_project", Some(json!({ "name": "待辦清單" })));
    let root = PathBuf::from(opened["project"]["path"].as_str().unwrap());
    assert!(root.starts_with(&projects), "{}", root.display());
    assert!(root.join("Cargo.toml").exists() && root.join("src/main.rs").exists());
    let greeting = h.turn(0, "greeting");
    assert!(completed(&greeting), "{greeting:#?}");
    assert!(says(&greeting, GREETING), "{greeting:#?}");
    let status = h.preview_ready("starter app");
    assert!(
        h.count(PREVIEW_EVENT) > 0,
        "the window hears about the preview"
    );
    assert!(page_answers(status.url.as_deref().unwrap()), "{status:?}");

    // The to-do app: written, still builds, kept in the timeline.
    let from = h.agent_events().len();
    h.call("send_message", Some(json!({ "text": TODO_REQUEST })))
        .unwrap();
    let todo = h.turn(from, "to-do app");
    assert!(completed(&todo) && build_ok(&todo), "{todo:#?}");
    assert!(says(&todo, TODO_DONE));
    let main = read(&root.join("src/main.rs"));
    assert!(main.contains("local_storage") && main.contains("todo-list"));
    assert!(read(&root.join("Cargo.toml")).contains("web-sys"));
    let status = h.preview_ready("to-do app");
    assert!(page_answers(status.url.as_deref().unwrap()), "{status:?}");
    let timeline: Value = h.ok("timeline", None);
    assert!(
        timeline.to_string().contains(TODO_REQUEST),
        "a version for the request: {timeline}"
    );

    // Point at the add button and make it green: the AI is told which element.
    let from = h.agent_events().len();
    let picked = json!({
        "ak_id": "add-button", "exact": true, "path": "main > div > form > button",
        "tag": "button", "text": "加入",
        "rect": { "x": 400.0, "y": 180.0, "width": 80.0, "height": 36.0 },
        "viewport": { "width": 1024.0, "height": 768.0 }
    });
    h.call(
        "send_message",
        Some(json!({ "text": COLOR_REQUEST, "selection": picked })),
    )
    .unwrap();
    let color = h.turn(from, "button color");
    assert!(completed(&color) && build_ok(&color), "{color:#?}");
    assert!(says(&color, COLOR_DONE));
    let asked = format!("{:?}", ai.requests().last().unwrap());
    assert!(
        asked.contains("add-button") && asked.contains("pointed at an element"),
        "the AI hears what was selected"
    );
    let main = read(&root.join("src/main.rs"));
    assert!(main.contains("bg-green-600") && !main.contains(ADD_BUTTON));
    assert_eq!(ai.remaining(), 0, "every scripted answer was used");
    h.preview_ready("green button");

    // Package, then the steps to install it.
    let bundled: BundleResult = h.ok("bundle", None);
    assert!(Path::new(&bundled.path).is_file(), "{bundled:?}");
    assert!(bundled.path.starts_with(&*root.to_string_lossy()));
    assert_eq!(
        bundled.os,
        match std::env::consts::OS {
            "macos" => "macos",
            "windows" => "windows",
            _ => "linux",
        }
    );
    assert!(!bundled.steps.is_empty(), "install guidance");
    assert!(h.count(BUNDLE_EVENT) > 0, "the window sees the progress");
}
