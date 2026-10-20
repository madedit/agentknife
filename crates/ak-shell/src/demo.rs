//! In-memory stand-in for the app, for trying the screens in a plain browser
//! (`trunk serve`). Connecting: keys containing `bad` are refused, addresses
//! containing `nothing` do not answer, models with `3b` cannot use tools.
//! Chat: a scripted turn; words in the message pick what it shows (`plan`,
//! `delete`, `ask`, `stuck`, `long`, `fail`). Preview: a static demo page after
//! a short start, or a real preview given as `?preview=<url>` (for trying
//! point-and-select against `cargo run -p ak-preview --example preview_live`).
// xtask-i18n: demo data (the AI's words, not UI text)

use std::sync::{Arc, Mutex};

use ak_types::agent::{
    AgentEvent, ConfirmKind, ConfirmReason, PlanDecision, PlanProposal, TokenUsage, TurnOutcome,
    UsageSummary,
};
use ak_types::chat::{Entry, Message};
use ak_types::ipc::{
    ConnectError, ConnectRequest, HomeState, ModelCheck, ModelChoice, OpenedProject, ProjectRow,
    SendMessage, cmd,
};
use ak_types::preview::{PreviewStatus, ServerState};
use ak_types::settings::AiConnection;
use futures_channel::oneshot;
use serde_json::{Value, json};

use crate::backend::{Listener, PreviewListener, now, sleep};

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
    next: u32,
    pub(crate) listener: Option<Listener>,
    pub(crate) preview_listener: Option<PreviewListener>,
    preview: PreviewStatus,
    /// The scripted turn waits here for a card's answer.
    waiting: Option<oneshot::Sender<Value>>,
    running: bool,
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
            next: 3,
            listener: None,
            preview_listener: None,
            preview: PreviewStatus::default(),
            waiting: None,
            running: false,
        }
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
            cmd::HOME_STATE => {
                let mut projects = d.projects.clone();
                projects.sort_by_key(|p| std::cmp::Reverse(p.last_opened_at));
                Ok(json!(HomeState {
                    projects,
                    ai: d.ai.clone(),
                    projects_folder: DEMO_FOLDER.into(),
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
            cmd::OPEN_FOLDER => Ok(Value::Null),
            cmd::PREVIEW_STATUS => Ok(json!(d.preview)),
            cmd::RUN_APP => Ok(Value::Null),
            cmd::LIST_MODELS | cmd::CHECK_MODEL => {
                let r: ConnectRequest =
                    serde_json::from_value(req).map_err(|e| text(e.to_string()))?;
                let key = r.api_key.as_deref().unwrap_or_default().trim();
                let address = r.connection.base_url.clone().unwrap_or_default();
                let needs_key =
                    ak_types::ipc::preset(&r.connection.provider).is_some_and(|p| p.needs_key);
                if (needs_key && key.is_empty()) || key.contains("bad") {
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
                d.ai = Some(r.connection);
                Ok(Value::Null)
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

fn set_preview(demo: &Arc<Mutex<Demo>>, status: PreviewStatus) {
    let listener = demo.lock().ok().and_then(|mut d| {
        d.preview = status.clone();
        d.preview_listener.clone()
    });
    if let Some(l) = listener {
        l(status);
    }
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
    let listener = demo.lock().ok().and_then(|d| d.listener.clone());
    if let Some(l) = listener {
        l(event);
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
