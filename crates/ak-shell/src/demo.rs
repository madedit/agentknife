//! In-memory stand-in for the app, for trying the screens in a plain browser
//! (`trunk serve`). Connecting: keys containing `bad` are refused, addresses
//! containing `nothing` do not answer, models with `3b` cannot use tools.
//! Chat: a scripted turn; words in the message pick what it shows (`plan`,
//! `delete`, `ask`, `stuck`, `long`, `fail`).
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
use ak_types::settings::AiConnection;
use futures_channel::oneshot;
use serde_json::{Value, json};

use crate::backend::{Listener, now, sleep};

const DEMO_FOLDER: &str = "~/AgentKnife/Projects";

pub struct Demo {
    projects: Vec<ProjectRow>,
    ai: Option<AiConnection>,
    next: u32,
    pub(crate) listener: Option<Listener>,
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
                Ok(json!(Demo::opened(row.clone())))
            }
            cmd::OPEN_FOLDER => Ok(Value::Null),
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
