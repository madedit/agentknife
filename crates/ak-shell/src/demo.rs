//! In-memory stand-in for the app, for trying the screens in a plain browser
//! (`trunk serve`). Connecting: keys containing `bad` are refused, addresses
//! containing `nothing` do not answer, models with `3b` cannot use tools.
//! Chat: a scripted turn; words in the message pick what it shows (`plan`,
//! `delete`, `ask`, `stuck`, `long`, `fail`). Preview: a static demo page after
//! a short start, or a real preview given as `?preview=<url>` (for trying
//! point-and-select against `cargo run -p ak-preview --example preview_live`).
// xtask-i18n: demo data (the AI's words, not UI text)

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ak_types::agent::{
    AgentEvent, ConfirmKind, ConfirmReason, PlanDecision, PlanProposal, TokenUsage, TurnOutcome,
    UsageSummary,
};
use ak_types::chat::{Entry, Message};
use ak_types::doctor::{
    CheckItem, CheckScope, CheckStatus, DoctorProgress, DoctorReport, GuideStep, InstallPhase,
    SystemAction, SystemGuide,
};
use ak_types::ipc::{
    BundleProgress, BundleResult, ConnectError, ConnectRequest, DoctorCheck, FileEntry, FileText,
    HomeState, ModelCheck, ModelChoice, OpenedProject, ProjectRow, SaveFile, SavedConnection,
    SendMessage, TimelineRow, cmd,
};
use ak_types::preview::{PreviewStatus, ServerState};
use ak_types::settings::AiConnection;
use futures_channel::oneshot;
use serde_json::{Value, json};

use crate::backend::{Handler, now, sleep};

/// The preview URL that stands for [`DEMO_PAGE`].
pub const DEMO_URL: &str = "about:srcdoc";

/// A tiny app for the demo preview (no inspector: try point-and-select with a
/// real preview through `?preview=`).
pub const DEMO_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><style>
body{font-family:system-ui,sans-serif;margin:0;background:#f8fafc;color:#0f172a}
main{max-width:28rem;margin:4rem auto;padding:1.5rem;display:flex;flex-direction:column;gap:1rem}
h1{font-size:1.75rem;margin:0}.card{background:#fff;border:1px solid #e2e8f0;border-radius:.75rem;padding:1rem}
input{width:100%;box-sizing:border-box;padding:.5rem;border:1px solid #cbd5e1;border-radius:.5rem}
button{margin-top:.75rem;background:#2563eb;color:#fff;border:0;border-radius:.5rem;padding:.5rem 1rem}
</style></head><body><main data-ak-id="home-screen"><h1 data-ak-id="home-title">My App</h1>
<div class="card" data-ak-id="greet-card"><label>Your name<input data-ak-id="name-input" placeholder="Ada"></label>
<button data-ak-id="greet-button">Greet</button></div></main></body></html>"#;

/// The preview page: `?preview=<url>` in the shell's address, else the demo page.
fn preview_url() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .unwrap_or_default();
        for pair in search.trim_start_matches('?').split('&') {
            if let Some(v) = pair.strip_prefix("preview=")
                && let Ok(url) = js_sys::decode_uri_component(v)
            {
                return url.into();
            }
        }
    }
    DEMO_URL.to_owned()
}

const DEMO_FOLDER: &str = "~/AgentKnife/Projects";

pub struct Demo {
    projects: Vec<ProjectRow>,
    ai: Option<AiConnection>,
    /// Providers whose key was saved (the demo's keychain).
    keys: std::collections::BTreeSet<String>,
    next: u32,
    /// Event name → listeners (`Backend::on`).
    pub(crate) listeners: BTreeMap<&'static str, Vec<Handler>>,
    settings: ak_types::settings::AppSettings,
    /// The tools check: what is still missing.
    doctor_missing: Vec<&'static str>,
    preview: PreviewStatus,
    /// The scripted turn waits here for a card's answer.
    waiting: Option<oneshot::Sender<Value>>,
    running: bool,
    /// The open project's files and history (one set for every demo project).
    files: BTreeMap<String, String>,
    timeline: Vec<TimelineRow>,
    /// Other AI tools: turning them on "connects" Claude Code after a moment.
    mcp: ak_types::ipc::McpStatus,
}

/// What the preview printed while it started.
fn demo_log() -> Vec<ak_types::log::LogLine> {
    use ak_types::log::LogSource::{Backend, Screens};
    [
        (Screens, "INFO starting build"),
        (Screens, "   Compiling app-ui v0.1.0"),
        (Screens, "INFO success"),
        (Backend, "   Compiling app v0.1.0 (src-tauri)"),
        (
            Backend,
            "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.20s",
        ),
        (Backend, "ak-bridge listening"),
    ]
    .into_iter()
    .zip(1..)
    .map(|((source, text), seq)| ak_types::log::LogLine {
        seq,
        at_ms: 0,
        source,
        text: text.into(),
        progress: None,
    })
    .collect()
}

/// A small project like the template's.
fn demo_files() -> BTreeMap<String, String> {
    [
        ("Cargo.toml", "[package]\nname = \"app-ui\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nleptos = { version = \"=0.8.22\", features = [\"csr\"] }\n"),
        ("README.md", "# My App\n\nMade with AgentKnife.\n"),
        ("index.html", "<!doctype html>\n<html>\n  <head><title>My App</title></head>\n  <body></body>\n</html>\n"),
        ("src/main.rs", "use leptos::prelude::*;\n\nfn main() {\n    ak_bridge::start();\n    leptos::mount::mount_to_body(App);\n}\n\n#[component]\nfn App() -> impl IntoView {\n    view! {\n        <main data-ak-id=\"home-screen\">\n            <h1 data-ak-id=\"home-title\">\"My App\"</h1>\n        </main>\n    }\n}\n"),
        ("src-tauri/src/main.rs", "fn main() {\n    tauri::Builder::default()\n        .plugin(ak_bridge::init())\n        .run(tauri::generate_context!())\n        .expect(\"the app failed to start\");\n}\n"),
        ("src-tauri/icons/icon.png", ""),
        ("style/input.css", "@import \"tailwindcss\";\n"),
    ]
    .into_iter()
    .map(|(p, t)| (p.to_owned(), t.to_owned()))
    .collect()
}

fn demo_timeline() -> Vec<TimelineRow> {
    let now = now();
    let row = |id: &str, kind: &str, label: &str, ago: u64| TimelineRow {
        id: id.into(),
        kind: kind.into(),
        label: label.into(),
        created_at: now.saturating_sub(ago),
        changed_files: Vec::new(),
        current: false,
    };
    vec![
        row("t1", "initial", "My App", 3 * 86_400),
        row("t2", "turn", "Make a to-do list", 2 * 86_400),
        row("t3", "marked", "First working version", 86_400),
        row("t4", "turn", "Make the buttons blue", 3_000),
    ]
}

impl Default for Demo {
    fn default() -> Self {
        Demo::new()
    }
}

impl Demo {
    pub fn new() -> Demo {
        let now = now();
        Demo {
            projects: vec![
                ProjectRow {
                    id: "demo-1".into(),
                    name: "Todo List".into(),
                    path: format!("{DEMO_FOLDER}/Todo List"),
                    last_opened_at: now.saturating_sub(3_600),
                    exists: true,
                },
                ProjectRow {
                    id: "demo-2".into(),
                    name: "Recipe Box".into(),
                    path: "~/Desktop/Recipe Box".into(),
                    last_opened_at: now.saturating_sub(9 * 86_400),
                    exists: false,
                },
            ],
            ai: None,
            keys: Default::default(),
            next: 3,
            listeners: BTreeMap::new(),
            doctor_missing: vec!["trunk", "tauri-cli", "webview2"],
            settings: ak_types::settings::AppSettings::default(),
            preview: PreviewStatus::default(),
            waiting: None,
            running: false,
            files: demo_files(),
            timeline: demo_timeline(),
            mcp: ak_types::ipc::McpStatus {
                folder: DEMO_FOLDER.into(),
                program: r"C:\Program Files\AgentKnife\agentknife-mcp.exe".into(),
                ..Default::default()
            },
        }
    }

    fn add_entry(&mut self, kind: &str, label: &str) {
        self.timeline.push(TimelineRow {
            id: format!("t{}", self.timeline.len() + 1),
            kind: kind.into(),
            label: label.into(),
            created_at: now(),
            changed_files: Vec::new(),
            current: false,
        });
    }

    fn opened(row: ProjectRow) -> OpenedProject {
        // The first demo project pretends its last turn was interrupted.
        let (interrupted, history) = if row.id == "demo-1" {
            (
                Some("Add a delete button to each item".to_owned()),
                vec![
                    Entry::Message(Message::user_text("Make a to-do list")),
                    Entry::Message(Message::Assistant {
                        thinking: Vec::new(),
                        text: "Done! You can add items and tick them off.".into(),
                        tool_calls: Vec::new(),
                        replay: None,
                    }),
                ],
            )
        } else {
            (None, Vec::new())
        };
        OpenedProject {
            project: row,
            interrupted,
            history,
        }
    }

    /// The commands that take a while (installing, packaging); `None` for the rest.
    pub async fn answer_slow(
        demo: &Arc<Mutex<Demo>>,
        command: &str,
    ) -> Option<Result<Value, Value>> {
        match command {
            cmd::DOCTOR_INSTALL => {
                doctor_install(demo).await;
                let missing = demo
                    .lock()
                    .map(|d| d.doctor_missing.clone())
                    .unwrap_or_default();
                Some(Ok(json!(doctor_check(&missing))))
            }
            cmd::BUNDLE => {
                // The demo packages a fresh project; "Broken" in its name fails.
                let broken = demo
                    .lock()
                    .map(|d| {
                        d.files
                            .get("README.md")
                            .is_some_and(|r| r.contains("Broken"))
                    })
                    .unwrap_or(false);
                if broken {
                    return Some(Err(json!(ak_types::ipc::BundleFailure {
                        kind: "build-failed".into(),
                        detail: "error[E0308]: mismatched types
 --> src/main.rs:12:5"
                            .into(),
                    })));
                }
                Some(Ok(json!(bundle(demo).await)))
            }
            _ => None,
        }
    }

    /// One command, answered at once (a turn then plays in the background).
    pub fn answer(
        demo: &Arc<Mutex<Demo>>,
        command: &str,
        request: Option<Value>,
    ) -> Result<Value, Value> {
        let text = |s: String| Value::String(s);
        let mut d = demo
            .lock()
            .map_err(|_| text("demo state poisoned".into()))?;
        let req = request.unwrap_or(Value::Null);
        let err = |e: ConnectError| serde_json::to_value(e).unwrap_or(Value::Null);
        match command {
            cmd::DELETE_PROJECT => {
                let r: ak_types::ipc::DeleteProject =
                    serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                d.projects.retain(|p| p.id != r.id);
                let mut projects = d.projects.clone();
                projects.sort_by_key(|p| std::cmp::Reverse(p.last_opened_at));
                Ok(json!(HomeState {
                    projects,
                    ai: d.ai.clone(),
                    projects_folder: DEMO_FOLDER.into(),
                    external: d.settings.mcp_enabled,
                }))
            }
            cmd::HOME_STATE => {
                let mut projects = d.projects.clone();
                projects.sort_by_key(|p| std::cmp::Reverse(p.last_opened_at));
                Ok(json!(HomeState {
                    projects,
                    ai: d.ai.clone(),
                    projects_folder: DEMO_FOLDER.into(),
                    external: d.settings.mcp_enabled,
                }))
            }
            cmd::CREATE_PROJECT => {
                let name = req["name"].as_str().unwrap_or_default().trim().to_owned();
                let row = ProjectRow {
                    id: format!("demo-{}", d.next),
                    path: format!("{DEMO_FOLDER}/{name}"),
                    name,
                    last_opened_at: now(),
                    exists: true,
                };
                d.next += 1;
                d.projects.push(row.clone());
                drop(d);
                start_preview(demo.clone());
                Ok(json!(Demo::opened(row)))
            }
            cmd::OPEN_PROJECT => {
                let id = req.as_str().unwrap_or_default();
                let row = d
                    .projects
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or_else(|| text(format!("no project {id}")))?;
                row.last_opened_at = now();
                let opened = Demo::opened(row.clone());
                drop(d);
                start_preview(demo.clone());
                Ok(json!(opened))
            }
            cmd::OPEN_FOLDER | cmd::PICK_FOLDER => Ok(Value::Null),
            cmd::PREVIEW_STATUS => Ok(json!(d.preview)),
            cmd::RUN_APP
            | cmd::REVEAL
            | cmd::CLEAR_LOG
            | cmd::SET_SELECTION
            | cmd::OPEN_TERMINAL
            | cmd::OPEN_IN_FILES => Ok(Value::Null),
            cmd::GET_LOG => Ok(json!(demo_log())),
            cmd::GET_SETTINGS => {
                let mut s = d.settings.clone();
                s.ai = d.ai.clone();
                Ok(json!(s))
            }
            cmd::SAVE_SETTINGS => {
                let s: ak_types::settings::AppSettings =
                    serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                d.settings = ak_types::settings::AppSettings {
                    ai: None,
                    ai_known: d.settings.ai_known.clone(),
                    ..s
                }
                .clamped();
                Ok(Value::Null)
            }
            cmd::MCP_STATUS => Ok(json!(d.mcp)),
            cmd::SET_MCP => {
                let on = req.as_bool().unwrap_or(false);
                d.settings.mcp_enabled = on;
                d.mcp.enabled = on;
                if !on {
                    d.mcp.clients.clear();
                }
                let status = d.mcp.clone();
                drop(d);
                if on && status.clients.is_empty() {
                    let demo = demo.clone();
                    leptos::task::spawn_local(async move {
                        sleep(3_000).await;
                        let status = demo.lock().ok().and_then(|mut d| {
                            d.mcp.enabled.then(|| {
                                d.mcp.clients = vec!["Claude Code".into()];
                                d.mcp.clone()
                            })
                        });
                        if let Some(status) = status {
                            send(&demo, ak_types::ipc::MCP_EVENT, status);
                        }
                    });
                }
                Ok(json!(status))
            }
            cmd::CRASH_REPORTS => Ok(json!(ak_types::ipc::CrashReports {
                folder: "~/AppData/Roaming/AgentKnife/crash-reports".into(),
                reports: vec![ak_types::ipc::CrashReport {
                    path: "~/AppData/Roaming/AgentKnife/crash-reports/1792500000.txt".into(),
                    created_at: now().saturating_sub(2 * 86_400),
                    text: "AgentKnife 0.1.0 crash report\nsystem: windows x86_64\nmessage: demo\n"
                        .into(),
                }],
            })),
            cmd::DOCTOR_CHECK => Ok(json!(doctor_check(&d.doctor_missing))),
            cmd::DOCTOR_ACTION => {
                // As if the official installer ran: the next check finds it.
                d.doctor_missing.retain(|id| *id != "webview2");
                Ok(Value::Null)
            }
            cmd::LIST_FILES => {
                let list: Vec<FileEntry> = d
                    .files
                    .keys()
                    .map(|p| FileEntry {
                        path: p.clone(),
                        dir: false,
                    })
                    .collect();
                Ok(json!(list))
            }
            cmd::READ_FILE => {
                let path = req.as_str().unwrap_or_default();
                let text = d
                    .files
                    .get(path)
                    .ok_or_else(|| text(format!("no file {path}")))?;
                Ok(json!(FileText {
                    path: path.to_owned(),
                    text: text.clone(),
                    binary: path.ends_with(".png"),
                }))
            }
            cmd::WRITE_FILE => {
                let f: SaveFile = serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                d.files.insert(f.path, f.text);
                drop(d);
                // Saving rebuilds the preview, as in the app.
                #[cfg(target_arch = "wasm32")]
                {
                    let demo = demo.clone();
                    leptos::task::spawn_local(async move { rebuild(&demo, None).await });
                }
                Ok(Value::Null)
            }
            cmd::TIMELINE => {
                let mut rows = d.timeline.clone();
                if let Some(last) = rows.last_mut() {
                    last.current = true;
                }
                rows.reverse();
                Ok(json!(rows))
            }
            cmd::RESTORE => {
                let id = req.as_str().unwrap_or_default();
                let label = d
                    .timeline
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.label.clone())
                    .ok_or_else(|| text(format!("no entry {id}")))?;
                d.add_entry("abandoned", "Work before going back");
                d.add_entry("restore", &label);
                Ok(Value::Null)
            }
            cmd::MARK_VERSION => {
                let label = req["label"].as_str().unwrap_or_default().to_owned();
                d.add_entry("marked", &label);
                Ok(Value::Null)
            }
            cmd::LIST_MODELS | cmd::CHECK_MODEL => {
                let r: ConnectRequest =
                    serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                let key = r.api_key.as_deref().unwrap_or_default().trim();
                let address = r.connection.base_url.clone().unwrap_or_default();
                let needs_key =
                    ak_types::ipc::preset(&r.connection.provider).is_some_and(|p| p.needs_key);
                let saved = d.keys.contains(&r.connection.provider);
                if (needs_key && key.is_empty() && !saved) || key.contains("bad") {
                    return Err(err(ConnectError::WrongKey));
                }
                if address.contains("nothing") {
                    return Err(err(ConnectError::Unreachable {
                        detail: format!("error sending request for url ({address}/models)"),
                    }));
                }
                if command == cmd::CHECK_MODEL {
                    let m = &r.connection.model;
                    return Ok(json!(ModelCheck {
                        tool_calling: !m.contains("3b"),
                        vision: m.starts_with("claude") || m.starts_with("gpt"),
                    }));
                }
                let ids: &[&str] = match r.connection.provider.as_str() {
                    "anthropic" => &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
                    "openai" => &["gpt-5.2", "gpt-5-mini"],
                    _ => &["qwen3:8b", "llama3.2:3b"],
                };
                let models: Vec<ModelChoice> = ids
                    .iter()
                    .map(|id| ModelChoice {
                        id: (*id).to_owned(),
                        display_name: None,
                    })
                    .collect();
                Ok(json!(models))
            }
            cmd::SAVE_CONNECTION => {
                let r: ConnectRequest =
                    serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                let c = r.connection;
                if r.api_key.is_some() {
                    d.keys.insert(c.provider.clone());
                }
                d.settings.ai_known.retain(|k| k.provider != c.provider);
                d.settings.ai_known.push(c.clone());
                d.ai = Some(c);
                Ok(Value::Null)
            }
            cmd::SAVED_CONNECTIONS => {
                let current = d.ai.as_ref().map(|a| a.provider.clone());
                let saved: Vec<SavedConnection> = d
                    .settings
                    .ai_known
                    .iter()
                    .map(|c| SavedConnection {
                        connection: c.clone(),
                        has_key: d.keys.contains(&c.provider),
                        current: current.as_ref() == Some(&c.provider),
                    })
                    .collect();
                Ok(json!(saved))
            }
            cmd::SEND_MESSAGE => {
                let m: SendMessage =
                    serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                if d.running {
                    return Err(text("a turn is already running".into()));
                }
                d.running = true;
                drop(d);
                play(demo.clone(), m.text);
                Ok(Value::Null)
            }
            cmd::CONTINUE_TURN | cmd::STUCK_CHOICE | cmd::RESOLVE_INTERRUPTION => {
                let what = match command {
                    cmd::CONTINUE_TURN => "continue",
                    cmd::STUCK_CHOICE => "another way",
                    _ => "resume",
                };
                d.running = true;
                drop(d);
                play(demo.clone(), what.to_owned());
                Ok(Value::Null)
            }
            cmd::ANSWER_CONFIRMATION | cmd::ANSWER_PLAN | cmd::ANSWER_QUESTION => {
                if let Some(tx) = d.waiting.take() {
                    let _ = tx.send(req["answer"].clone());
                }
                Ok(Value::Null)
            }
            cmd::CANCEL_TURN => {
                // Dropping the sender ends a waiting turn; a busy one checks below.
                d.waiting = None;
                d.running = false;
                Ok(Value::Null)
            }
            other => Err(text(format!("unknown command {other}"))),
        }
    }
}

fn play(demo: Arc<Mutex<Demo>>, text: String) {
    #[cfg(target_arch = "wasm32")]
    leptos::task::spawn_local(turn(demo, text));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (demo, text, turn);
}

/// Sends `payload` to every listener of `event`.
fn send(demo: &Arc<Mutex<Demo>>, event: &str, payload: impl serde::Serialize) {
    let handlers = demo
        .lock()
        .ok()
        .and_then(|d| d.listeners.get(event).cloned())
        .unwrap_or_default();
    let value = serde_json::to_value(payload).unwrap_or(Value::Null);
    for h in handlers {
        h(value.clone());
    }
}

fn set_preview(demo: &Arc<Mutex<Demo>>, status: PreviewStatus) {
    if let Ok(mut d) = demo.lock() {
        d.preview = status.clone();
    }
    send(demo, ak_types::ipc::PREVIEW_EVENT, status);
}

/// Starting → first build → ready, like a real preview (shortened).
fn start_preview(demo: Arc<Mutex<Demo>>) {
    set_preview(
        &demo,
        PreviewStatus {
            frontend: ServerState::Starting,
            backend: ServerState::Starting,
            url: None,
        },
    );
    #[cfg(target_arch = "wasm32")]
    leptos::task::spawn_local(async move {
        sleep(800).await;
        set_preview(
            &demo,
            PreviewStatus {
                frontend: ServerState::Building,
                backend: ServerState::Building,
                url: None,
            },
        );
        sleep(2_500).await;
        set_preview(&demo, ready());
    });
}

fn ready() -> PreviewStatus {
    PreviewStatus {
        frontend: ServerState::Ready,
        backend: ServerState::Ready,
        url: Some(preview_url()),
    }
}

/// A rebuild after the AI changed something: the page stays while it builds.
async fn rebuild(demo: &Arc<Mutex<Demo>>, error: Option<&str>) {
    set_preview(
        demo,
        PreviewStatus {
            frontend: ServerState::Building,
            ..ready()
        },
    );
    sleep(1_200).await;
    set_preview(
        demo,
        match error {
            Some(output) => PreviewStatus {
                frontend: ServerState::Error {
                    output: output.to_owned(),
                },
                ..ready()
            },
            None => ready(),
        },
    );
}

fn emit(demo: &Arc<Mutex<Demo>>, event: AgentEvent) {
    send(demo, ak_types::ipc::AGENT_EVENT, event);
}

fn doctor_check(missing: &[&str]) -> DoctorCheck {
    let item = |id: &str, scope: CheckScope| CheckItem {
        id: id.into(),
        scope,
        status: if missing.contains(&id) {
            CheckStatus::Missing
        } else {
            CheckStatus::Ok
        },
        found: None,
        wanted: None,
        detail: None,
    };
    let report = DoctorReport {
        items: vec![
            item("rust", CheckScope::Isolated),
            item("wasm32-target", CheckScope::Isolated),
            item("trunk", CheckScope::Isolated),
            item("tauri-cli", CheckScope::Isolated),
            item("tailwind", CheckScope::Isolated),
            item("msvc-build-tools", CheckScope::System),
            item("webview2", CheckScope::System),
        ],
    };
    let step = |m: &str| GuideStep {
        message: m.into(),
        command: None,
        link: None,
    };
    let guides = if missing.contains(&"webview2") {
        vec![SystemGuide {
            item: "webview2".into(),
            covers: vec!["webview2".into()],
            steps: vec![
                step("doctor-guide-webview2-what"),
                step("doctor-guide-webview2-winget"),
                step("doctor-guide-check-again"),
            ],
            action: Some(SystemAction::WingetWebView2),
        }]
    } else {
        Vec::new()
    };
    DoctorCheck { report, guides }
}

/// Installs the missing isolated tools, with progress like the real one.
async fn doctor_install(demo: &Arc<Mutex<Demo>>) {
    let missing: Vec<&'static str> = demo
        .lock()
        .map(|d| d.doctor_missing.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| *id != "webview2")
        .collect();
    let total = u32::try_from(missing.len()).unwrap_or(0);
    for (n, item) in missing.iter().enumerate() {
        let step = u32::try_from(n).unwrap_or(0) + 1;
        let progress = |phase| DoctorProgress {
            item: (*item).into(),
            step,
            total,
            phase,
        };
        let size = 8_000_000u64;
        for k in 1..=5u64 {
            send(
                demo,
                ak_types::ipc::DOCTOR_EVENT,
                progress(InstallPhase::Download {
                    done: size * k / 5,
                    size: Some(size),
                }),
            );
            sleep(250).await;
        }
        send(
            demo,
            ak_types::ipc::DOCTOR_EVENT,
            progress(InstallPhase::Verify),
        );
        sleep(300).await;
        send(
            demo,
            ak_types::ipc::DOCTOR_EVENT,
            progress(InstallPhase::Unpack),
        );
        sleep(300).await;
        send(
            demo,
            ak_types::ipc::DOCTOR_EVENT,
            progress(InstallPhase::Done),
        );
    }
    if let Ok(mut d) = demo.lock() {
        d.doctor_missing.retain(|id| *id == "webview2");
    }
}

async fn bundle(demo: &Arc<Mutex<Demo>>) -> BundleResult {
    let stage = |m: &str| BundleProgress::Stage { message: m.into() };
    send(demo, ak_types::ipc::BUNDLE_EVENT, stage("bundle-building"));
    for line in [
        "   Compiling app-ui v0.1.0",
        "   Compiling app v0.1.0 (src-tauri)",
        "    Finished `release` profile [optimized] target(s) in 41.20s",
    ] {
        sleep(700).await;
        send(
            demo,
            ak_types::ipc::BUNDLE_EVENT,
            BundleProgress::Output { line: line.into() },
        );
    }
    send(demo, ak_types::ipc::BUNDLE_EVENT, stage("bundle-checking"));
    sleep(400).await;
    send(demo, ak_types::ipc::BUNDLE_EVENT, stage("bundle-packing"));
    sleep(400).await;
    let step = |m: &str| GuideStep {
        message: m.into(),
        command: None,
        link: None,
    };
    BundleResult {
        path:
            "C:/Users/me/AgentKnife/Projects/My App/target/release/bundle/zip/My App_0.1.0_x64.zip"
                .into(),
        file: "My App_0.1.0_x64.zip".into(),
        app: "My App".into(),
        os: "windows".into(),
        steps: [
            "bundle-guide-win-extract",
            "bundle-guide-win-move",
            "bundle-guide-win-run",
            "bundle-guide-win-smartscreen",
            "bundle-guide-win-shortcut",
            "bundle-guide-share-windows",
        ]
        .map(step)
        .into(),
    }
}

fn still_running(demo: &Arc<Mutex<Demo>>) -> bool {
    demo.lock().is_ok_and(|d| d.running)
}

async fn wait_for_answer(demo: &Arc<Mutex<Demo>>) -> Option<Value> {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut d) = demo.lock() {
        d.waiting = Some(tx);
    }
    rx.await.ok()
}

/// Streams `text` in small pieces, like a model.
async fn stream(demo: &Arc<Mutex<Demo>>, text: &str, thinking: bool) {
    let words: Vec<&str> = text.split_inclusive(' ').collect();
    for chunk in words.chunks(2) {
        let piece = chunk.concat();
        emit(
            demo,
            if thinking {
                AgentEvent::ThinkingDelta { text: piece }
            } else {
                AgentEvent::TextDelta { text: piece }
            },
        );
        sleep(60).await;
    }
    if thinking {
        emit(demo, AgentEvent::ThinkingEnd);
    }
}

async fn step(
    demo: &Arc<Mutex<Demo>>,
    id: &str,
    label: &str,
    detail: &str,
    output: &str,
    ok: bool,
) {
    emit(
        demo,
        AgentEvent::StepStarted {
            step_id: id.into(),
            tool: "demo".into(),
            label: label.into(),
            detail: detail.into(),
        },
    );
    sleep(700).await;
    emit(
        demo,
        AgentEvent::StepFinished {
            step_id: id.into(),
            ok,
            output: output.into(),
        },
    );
}

fn usage(cents: f64) -> UsageSummary {
    UsageSummary {
        tokens: TokenUsage {
            input_tokens: 12_000,
            output_tokens: 900,
            ..TokenUsage::default()
        },
        cost_usd: Some(cents / 100.0),
        cost_is_lower_bound: false,
        free: false,
    }
}

async fn turn(demo: Arc<Mutex<Demo>>, text: String) {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    emit(
        &demo,
        AgentEvent::TurnStarted {
            turn_id: format!("demo-{}", now()),
        },
    );
    stream(
        &demo,
        "The user wants a change. Let me look at the screen code first.",
        true,
    )
    .await;
    let outcome = 'turn: {
        if has(&["plan", "計畫"]) {
            stream(
                &demo,
                "This is a bigger change, so here is a plan first.",
                false,
            )
            .await;
            emit(
                &demo,
                AgentEvent::PlanProposed {
                    request_id: "demo-plan".into(),
                    plan: PlanProposal {
                        title: "Save the list between visits".into(),
                        steps: vec![
                            "Store the items in a file on this computer".into(),
                            "Load them when the app starts".into(),
                            "Save after every change".into(),
                        ],
                        estimate: Some("about 3 minutes, a few cents".into()),
                    },
                },
            );
            let decision = wait_for_answer(&demo)
                .await
                .and_then(|v| serde_json::from_value::<PlanDecision>(v).ok());
            match decision {
                Some(PlanDecision::Start) => {}
                Some(PlanDecision::Revise { feedback }) => {
                    stream(
                        &demo,
                        &format!("OK, I will change the plan: {feedback}"),
                        false,
                    )
                    .await;
                    break 'turn TurnOutcome::Completed;
                }
                _ => {
                    stream(&demo, "OK, I left everything as it was.", false).await;
                    break 'turn TurnOutcome::Completed;
                }
            }
        }
        if has(&["delete", "刪"]) {
            emit(
                &demo,
                AgentEvent::NeedsConfirmation {
                    request_id: "demo-confirm".into(),
                    tool: "run_command".into(),
                    label: "Removing the old screen".into(),
                    reason: ConfirmReason {
                        kind: ConfirmKind::Delete,
                        detail: "src/old_screen.rs".into(),
                    },
                },
            );
            let allowed = wait_for_answer(&demo).await.and_then(|v| v.as_bool());
            if allowed != Some(true) {
                stream(
                    &demo,
                    "All right, I kept the old screen. What would you prefer?",
                    false,
                )
                .await;
                break 'turn TurnOutcome::Completed;
            }
            step(
                &demo,
                "demo-del",
                "Removing the old screen",
                "rm src/old_screen.rs",
                "",
                true,
            )
            .await;
        }
        if has(&["ask", "問"]) {
            emit(
                &demo,
                AgentEvent::QuestionAsked {
                    request_id: "demo-question".into(),
                    question: "Which color should the buttons be?".into(),
                    options: vec!["Blue (recommended)".into(), "Green".into()],
                    allow_other: true,
                },
            );
            let Some(answer) = wait_for_answer(&demo)
                .await
                .and_then(|v| v.as_str().map(str::to_owned))
            else {
                break 'turn TurnOutcome::Cancelled;
            };
            stream(&demo, &format!("{answer} it is."), false).await;
        }
        stream(&demo, "Sure, I will update the screen.", false).await;
        step(
            &demo,
            "demo-read",
            "Reading the main screen",
            "src/main.rs",
            "use leptos::prelude::*;\n\nfn main() {\n    ak_bridge::start();\n}",
            true,
        )
        .await;
        if !still_running(&demo) {
            break 'turn TurnOutcome::Cancelled;
        }
        step(
            &demo,
            "demo-edit",
            "Changing the button color",
            "src/main.rs",
            "Replaced 1 occurrence.",
            true,
        )
        .await;
        let broken = has(&["stuck", "壞"]).then_some(
            "error[E0308]: mismatched types\n --> src/main.rs:42:9\n   |\n42 |         \"blue\"\n   |         ^^^^^^ expected `bool`, found `&str`",
        );
        rebuild(&demo, broken).await;
        if has(&["long", "長"]) {
            break 'turn TurnOutcome::StepLimit;
        }
        if has(&["fail", "失敗"]) {
            break 'turn TurnOutcome::Failed {
                message: "network error: connection reset".into(),
                retryable: true,
            };
        }
        emit(&demo, AgentEvent::BuildChecking);
        sleep(900).await;
        if has(&["stuck", "壞"]) {
            for attempt in 1..=3 {
                emit(
                    &demo,
                    AgentEvent::BuildChecked {
                        ok: false,
                        attempt,
                        max: 3,
                    },
                );
                step(
                    &demo,
                    &format!("demo-fix-{attempt}"),
                    "Fixing the type error",
                    "src/main.rs",
                    "error[E0308]: mismatched types",
                    false,
                )
                .await;
                emit(&demo, AgentEvent::BuildChecking);
                sleep(600).await;
            }
            break 'turn TurnOutcome::Stuck {
                attempts: 3,
                last_error: "error[E0308]: mismatched types\n --> src/main.rs:42:9".into(),
            };
        }
        emit(
            &demo,
            AgentEvent::BuildChecked {
                ok: true,
                attempt: 0,
                max: 3,
            },
        );
        stream(
            &demo,
            "Done! The button is blue now. Try clicking it in the preview.",
            false,
        )
        .await;
        TurnOutcome::Completed
    };
    if outcome == TurnOutcome::Completed
        && let Ok(mut d) = demo.lock()
    {
        if let Some(main) = d.files.get_mut("src/main.rs") {
            main.push_str("// changed by the AI\n");
        }
        d.add_entry("turn", &text);
    }
    let outcome = if still_running(&demo) {
        outcome
    } else {
        TurnOutcome::Cancelled
    };
    emit(
        &demo,
        AgentEvent::UsageTotals {
            session: usage(1.2),
            today: usage(42.0),
        },
    );
    if let Ok(mut d) = demo.lock() {
        d.running = false;
        d.waiting = None;
    }
    emit(&demo, AgentEvent::TurnFinished { outcome });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Backend, CallError};

    fn block<F: std::future::Future>(f: F) -> F::Output {
        // The demo never awaits anything natively: one poll finishes it.
        let mut f = std::pin::pin!(f);
        let waker = std::task::Waker::noop();
        match f.as_mut().poll(&mut std::task::Context::from_waker(waker)) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("demo call did not finish"),
        }
    }

    fn request(provider: &str, key: Option<&str>, model: &str) -> ConnectRequest {
        ConnectRequest {
            connection: AiConnection {
                provider: provider.into(),
                base_url: None,
                model: model.into(),
                effort: None,
            },
            api_key: key.map(Into::into),
        }
    }

    #[test]
    fn the_demo_answers_like_the_app() {
        let b = Backend::demo();
        let home = block(b.home_state()).unwrap();
        assert_eq!(home.projects.len(), 2);
        assert!(home.ai.is_none());

        let opened = block(b.create_project("Shop")).unwrap();
        assert_eq!(
            block(b.home_state()).unwrap().projects[0].id,
            opened.project.id
        );
        assert!(opened.interrupted.is_none());
        let first = block(b.open_project("demo-1")).unwrap();
        assert!(first.interrupted.is_some() && !first.history.is_empty());

        assert_eq!(
            block(b.list_models(&request("anthropic", None, ""))),
            Err(CallError::Connect(ConnectError::WrongKey))
        );
        let models = block(b.list_models(&request("anthropic", Some("sk-1"), ""))).unwrap();
        assert_eq!(models[0].id, "claude-opus-5-5");
        let weak = block(b.check_model(&request("ollama", None, "llama3.2:3b"))).unwrap();
        assert!(!weak.tool_calling);

        block(b.save_connection(&request("ollama", None, "qwen3:8b"))).unwrap();
        assert_eq!(
            block(b.home_state()).unwrap().ai.map(|a| a.model),
            Some("qwen3:8b".into())
        );
        assert!(matches!(
            block(b.open_project("nope")),
            Err(CallError::Other(_))
        ));
        block(b.answer_confirmation("x", true)).unwrap();
        block(b.cancel_turn()).unwrap();
    }
}
