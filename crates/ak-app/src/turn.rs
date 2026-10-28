//! Running the agent for the open project: one agent per turn (so a changed AI
//! connection applies at once), its events forwarded to the window, and the
//! answers it waits for coming back through the chat's commands.

use std::path::PathBuf;
use std::sync::Arc;

use ak_agent::permission::{ConfirmRequest, Confirmer};
use ak_agent::plan::PlanReviewer;
use ak_agent::policy::{PermissionMode, ProjectPolicy};
use ak_agent::prompt::PlanMode;
use ak_agent::tools::{Asker, Question, ToolEnv, project_tools, with_preview_tools};
use ak_agent::{Agent, AgentConfig, Checkpointer, ConversationStore, TurnSummary, prompt, usage};
use ak_preview::log::LogBook;
use ak_project::{CheckpointKind, Project, Timeline};
use ak_types::agent::{AgentEvent, PlanDecision, PlanProposal, TurnOutcome};
use ak_types::chat::{Conversation, Message};
use ak_types::i18n::I18n;
use ak_types::ipc::{AGENT_EVENT, Answer, InterruptionChoice, SendMessage, StuckChoice};
use ak_types::log::LogSource;
use ak_types::preview::Picked;
use ak_types::settings::{Permissions, Plans};
use futures_util::future::BoxFuture;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, Pending};
use crate::{Res, fail, ui_locale};

/// The chat's side of the agent's questions: each waits for its command.
pub(crate) struct UiBridge(pub(crate) Arc<std::sync::Mutex<Pending>>);

impl Confirmer for UiBridge {
    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'_, bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut p) = self.0.lock() {
            p.confirmations.insert(request.request_id, tx);
        }
        Box::pin(async move { rx.await.unwrap_or(false) })
    }
}

impl PlanReviewer for UiBridge {
    fn review(&self, request_id: String, _: PlanProposal) -> BoxFuture<'_, PlanDecision> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut p) = self.0.lock() {
            p.plans.insert(request_id, tx);
        }
        Box::pin(async move { rx.await.unwrap_or(PlanDecision::Cancel) })
    }
}

impl Asker for UiBridge {
    fn ask(&self, q: Question) -> BoxFuture<'_, Option<String>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut p) = self.0.lock() {
            p.questions.insert(q.request_id, tx);
        }
        Box::pin(async move { rx.await.ok().flatten() })
    }
}

struct Store(Arc<std::sync::Mutex<Project>>);

impl ConversationStore for Store {
    fn save(&self, conversation: &Conversation) -> Result<(), String> {
        let p = self
            .0
            .lock()
            .map_err(|_| "project state poisoned".to_owned())?;
        p.save_conversation(conversation).map_err(|e| e.to_string())
    }
}

/// A timeline entry after every turn that changed something.
struct Checkpoints(PathBuf);

impl Checkpointer for Checkpoints {
    fn checkpoint<'a>(
        &'a self,
        summary: &'a TurnSummary,
    ) -> BoxFuture<'a, Result<Option<(String, String)>, String>> {
        let root = self.0.clone();
        let label = summary.request.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut t = Timeline::open(&root).map_err(|e| e.to_string())?;
                let entry = t
                    .checkpoint(CheckpointKind::Turn, &label, None)
                    .map_err(|e| e.to_string())?;
                Ok(entry.map(|e| (e.id, e.label)))
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }
}

/// Everything a turn needs from the open project.
struct Parts {
    root: PathBuf,
    project: Arc<std::sync::Mutex<Project>>,
    preview: Arc<ak_preview::PreviewSession>,
    conversation: Arc<tokio::sync::Mutex<Conversation>>,
    cancel: CancellationToken,
    log: LogBook,
}

/// The agent for the open project with the current AI connection.
async fn agent(state: &AppState, parts: &Parts) -> anyhow::Result<Agent> {
    let (profile, effective, settings) = {
        let inner = state.inner.lock().await;
        let global = inner
            .settings
            .ai
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no AI is connected"))?;
        let project = parts
            .project
            .lock()
            .map_err(|_| anyhow::anyhow!("project state poisoned"))?;
        (
            crate::home::profile_of(&project),
            project.effective_ai(&global),
            inner.settings.clone(),
        )
    };
    let provider = match &state.provider {
        Some(p) => p.clone(),
        None => crate::connect::provider(&effective, None).map_err(|e| anyhow::anyhow!("{e}"))?,
    };
    let caps = provider
        .capabilities(&effective.model)
        .await
        .unwrap_or_default();

    let mut config = AgentConfig::new(effective.model.clone());
    config.system = Some(prompt::product_prompt().to_owned());
    config.project_dir = Some(parts.root.clone());
    config.ui_locale = ui_locale();
    config.max_fix_attempts = settings.max_fix_attempts;
    config.daily_cap_usd = settings.daily_cap_usd;
    config.plan_mode = match settings.plans {
        Plans::BigChanges => PlanMode::Auto,
        Plans::Always => PlanMode::AlwaysAsk,
        Plans::Never => PlanMode::NeverAsk,
    };
    config.context_window = caps.context_window;
    if let Some(max) = caps.max_output_tokens {
        config.max_tokens = config.max_tokens.min(max);
    }
    config.cost_basis = usage::cost_basis(
        &effective.provider,
        effective.base_url.as_deref(),
        &effective.model,
    );
    let bench = workbench(
        state,
        &parts.root,
        profile,
        parts.preview.clone(),
        &settings,
        caps.vision,
    );
    if let Some(system) = config.system.as_mut() {
        system.push_str(&bench.notes);
    }
    config.reference_dirs = bench.references.clone();
    let Workbench {
        registry,
        policy,
        bridge,
        ..
    } = bench;
    Ok(Agent::new(provider, registry, config)
        .with_gate(Arc::new(policy))
        .with_confirmer(bridge.clone())
        .with_plan_reviewer(bridge)
        .with_preview(parts.preview.clone())
        .with_id_pass(Arc::new(move |rel: &str, src: &str| {
            profile.add_missing_ids(rel, src)
        }))
        .with_checkpointer(Arc::new(Checkpoints(parts.root.clone())))
        .with_store(Arc::new(Store(parts.project.clone())))
        .with_usage_ledger(state.ledger.clone()))
}

/// The open project's tools and permissions, as the built-in agent has them
/// (the MCP server gives other AI tools the same).
pub(crate) struct Workbench {
    pub registry: ak_agent::tool::ToolRegistry,
    pub policy: ProjectPolicy,
    /// Folders the read tools may read besides the project.
    pub references: Vec<PathBuf>,
    /// What the AI should know about those folders (English: for the model).
    pub notes: String,
    pub bridge: Arc<UiBridge>,
}

pub(crate) fn workbench(
    state: &AppState,
    root: &std::path::Path,
    profile: &'static dyn ak_types::stack::StackProfile,
    preview: Arc<dyn ak_agent::tools::PreviewAccess>,
    settings: &ak_types::settings::AppSettings,
    vision: bool,
) -> Workbench {
    let mode = match settings.permissions {
        Permissions::Standard => PermissionMode::Standard,
        Permissions::AskEveryChange => PermissionMode::AskEveryChange,
    };
    // Library sources the AI may read without asking (no keys or user data
    // there), and the folders the user allowed in the settings.
    let mut references = reference_dirs(&state.toolchain);
    let mut notes = library_note(&state.toolchain.cargo_home());
    let allowed = allowed_folders(&settings.read_folders);
    if let Some(note) = folders_note(&allowed) {
        notes.push_str(&note);
    }
    references.extend(allowed);
    // What the app saved in the preview (its own folder, not the installed
    // app's), so the AI can check it.
    let data = root.join(profile.preview().data_dir);
    notes.push_str(&app_data_note(&data));
    references.push(data);
    let bridge = Arc::new(UiBridge(state.pending.clone()));
    let env = ToolEnv {
        path_prefix: state.tools.path_prefix.clone(),
        vars: state.tools.env.clone(),
    };
    let registry = with_preview_tools(
        project_tools(env, bridge.clone(), Some(profile.knowledge_pack())),
        preview,
        vision,
    );
    Workbench {
        registry,
        policy: ProjectPolicy::new(mode).with_reference_dirs(references.clone()),
        references,
        notes,
        bridge,
    }
}

/// The toolchain's library sources: crates (registry, git) and Rust's own.
/// Not the whole toolchain folder: cargo's credentials file lives there.
pub fn reference_dirs(tc: &ak_doctor::Toolchain) -> Vec<PathBuf> {
    vec![
        tc.cargo_home().join("registry"),
        tc.cargo_home().join("git"),
        tc.rustup_home().join("toolchains"),
    ]
}

/// The settings' readable folders that exist (absolute paths only).
pub fn allowed_folders(folders: &[String]) -> Vec<PathBuf> {
    folders
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .collect()
}

/// Tells the AI about the folders the user allowed (English: for the model).
pub fn folders_note(folders: &[PathBuf]) -> Option<String> {
    if folders.is_empty() {
        return None;
    }
    let list: Vec<String> = folders
        .iter()
        .map(|f| format!("- {}", f.display()))
        .collect();
    Some(format!(
        "\n\nThe user allows you to read files in these folders without asking \
         (read_file, list_files and search_files take absolute paths there). They are \
         for reading only: never change anything in them.\n{}",
        list.join("\n")
    ))
}

/// Tells the AI where the libraries' source code is (English: for the model).
pub fn library_note(cargo_home: &std::path::Path) -> String {
    format!(
        "\n\nThe source code of the project's downloaded libraries (crates) is in {} \
         (one folder per index, then `<crate>-<version>`, e.g. `leptos-0.8.22`). To check \
         a library's API, list_files, search_files and read_file take absolute paths \
         there; search one crate's folder, not the whole registry. Read only. A \
         crate's optional parts are the `[features]` of its `Cargo.toml` there. \
         ak-ui and ak-bridge are not there: their source is in the project's `ak/` \
         folder (`ak/ak-ui/src`, `ak/ak-bridge/src`).",
        cargo_home.join("registry").join("src").display()
    )
}

/// Tells the AI where the app saves its data (English: for the model).
pub fn app_data_note(folder: &std::path::Path) -> String {
    format!(
        "\n\nIn the preview the app saves its data (app_data_dir and its other app \
         folders) in {}, not where the installed app keeps it; it appears once the app \
         first saves something. To check what the app wrote, read_file and list_files \
         take absolute paths there. Read only: never change anything in it. The user \
         can delete it with \"Clear data and start over\".",
        folder.display()
    )
}

/// What a turn starts from.
enum Start {
    /// The user's words, after an optional note for the AI.
    Message { text: String, note: Option<String> },
    /// The interrupted turn in `turn.lock`.
    Resume,
    /// The first message in a new chat.
    Greet,
}

/// Claims the open project for a turn; `None` when nothing is open or a turn runs.
async fn claim(state: &AppState) -> Option<Parts> {
    let mut inner = state.inner.lock().await;
    let open = inner.open.as_mut()?;
    if open.cancel.is_some() || crate::mcp::busy(state) {
        return None;
    }
    let cancel = CancellationToken::new();
    open.cancel = Some(cancel.clone());
    open.before_turn = Timeline::open(&open.root)
        .ok()
        .and_then(|t| t.entries().last().map(|e| e.id.clone()));
    Some(Parts {
        root: open.root.clone(),
        project: open.project.clone(),
        preview: open.preview.clone(),
        conversation: open.conversation.clone(),
        cancel,
        log: open.log.clone(),
    })
}

/// The AI's steps in the project's log: what runs, then what it printed.
pub(crate) fn log_step(log: &LogBook, event: &AgentEvent) {
    match event {
        AgentEvent::StepStarted {
            tool,
            label,
            detail,
            ..
        } => {
            log.push(LogSource::Ai, &format!("> {label}"));
            log.push(LogSource::Ai, &format!("  {tool}: {detail}"));
        }
        AgentEvent::StepFinished { ok, output, .. } => {
            log.push(LogSource::Ai, output);
            if !ok {
                log.push(LogSource::Ai, "(this step failed)");
            }
        }
        _ => {}
    }
}

fn emit<R: Runtime>(app: &AppHandle<R>, event: AgentEvent) {
    let _ = app.emit(AGENT_EVENT, event);
}

/// How long streamed words are collected before they go to the window.
const DELTA_FLUSH: std::time::Duration = std::time::Duration::from_millis(100);

/// Joins the model's streamed words into fewer window events. A fast local
/// model sends a hundred or more pieces a second; one event each floods the
/// window, which then falls behind (Stop is handled long after it is pressed).
#[derive(Default)]
struct Deltas {
    pending: Option<AgentEvent>,
}

impl Deltas {
    /// The events to send now, in their order.
    fn push(&mut self, event: AgentEvent) -> Vec<AgentEvent> {
        match (self.pending.as_mut(), &event) {
            (Some(AgentEvent::TextDelta { text: a }), AgentEvent::TextDelta { text: b })
            | (
                Some(AgentEvent::ThinkingDelta { text: a }),
                AgentEvent::ThinkingDelta { text: b },
            ) => {
                a.push_str(b);
                return Vec::new();
            }
            _ => {}
        }
        let earlier = self.pending.take();
        if matches!(
            event,
            AgentEvent::TextDelta { .. } | AgentEvent::ThinkingDelta { .. }
        ) {
            self.pending = Some(event);
            earlier.into_iter().collect()
        } else {
            earlier.into_iter().chain([event]).collect()
        }
    }

    fn flush(&mut self) -> Option<AgentEvent> {
        self.pending.take()
    }
}

/// Runs one turn in the background.
fn spawn<R: Runtime>(app: AppHandle<R>, start: Start) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let Some(parts) = claim(&state).await else {
            if !matches!(start, Start::Greet) {
                emit(
                    &app,
                    AgentEvent::TurnFinished {
                        outcome: TurnOutcome::Failed {
                            message: "the AI is still working on the last request".into(),
                            retryable: false,
                        },
                    },
                );
            }
            return;
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let forward = {
            let app = app.clone();
            let log = parts.log.clone();
            tauri::async_runtime::spawn(async move {
                let mut deltas = Deltas::default();
                let mut flush_at = None;
                loop {
                    let next = match flush_at {
                        Some(at) => match tokio::time::timeout_at(at, rx.recv()).await {
                            Ok(next) => next,
                            Err(_) => {
                                if let Some(d) = deltas.flush() {
                                    emit(&app, d);
                                }
                                flush_at = None;
                                continue;
                            }
                        },
                        None => rx.recv().await,
                    };
                    let Some(e) = next else { break };
                    log_step(&log, &e);
                    for out in deltas.push(e) {
                        emit(&app, out);
                    }
                    flush_at = match (deltas.pending.is_some(), flush_at) {
                        (false, _) => None,
                        (true, Some(at)) => Some(at),
                        (true, None) => Some(tokio::time::Instant::now() + DELTA_FLUSH),
                    };
                }
                if let Some(d) = deltas.flush() {
                    emit(&app, d);
                }
            })
        };
        match agent(&state, &parts).await {
            Err(e) => {
                if !matches!(start, Start::Greet) {
                    let _ = tx.send(AgentEvent::TurnFinished {
                        outcome: TurnOutcome::Failed {
                            message: e.to_string(),
                            retryable: false,
                        },
                    });
                }
            }
            Ok(agent) => {
                let mut conversation = parts.conversation.lock().await;
                match start {
                    Start::Message { text, note } => {
                        if let Some(n) = note {
                            conversation.push_context(n);
                        }
                        agent
                            .run_turn(
                                &mut conversation,
                                Message::user_text(text),
                                &tx,
                                &parts.cancel,
                            )
                            .await;
                    }
                    Start::Resume => {
                        if agent
                            .resume_turn(&mut conversation, &tx, &parts.cancel)
                            .await
                            .is_none()
                        {
                            let _ = tx.send(AgentEvent::TurnFinished {
                                outcome: TurnOutcome::Completed,
                            });
                        }
                    }
                    Start::Greet => {
                        let _ = agent.greet(&mut conversation, &tx, &parts.cancel).await;
                    }
                }
            }
        }
        drop(tx);
        let _ = forward.await;
        let keep = {
            let mut inner = state.inner.lock().await;
            if let Some(open) = inner.open.as_mut() {
                open.cancel = None;
            }
            inner.settings.keep_turns as usize
        };
        // Older changes fold into their task (the version-history setting).
        let root = parts.root.clone();
        let _ =
            tokio::task::spawn_blocking(move || Timeline::open(&root).and_then(|mut t| t.gc(keep)))
                .await;
    });
}

/// After a project opens: the project notes for the AI, then the greeting.
pub async fn on_open<R: Runtime>(app: AppHandle<R>, greet: bool) {
    let state = app.state::<AppState>();
    let Some(parts) = claim(&state).await else {
        return;
    };
    if let Ok(agent) = agent(&state, &parts).await {
        let mut conversation = parts.conversation.lock().await;
        let _ = agent.open_session(&mut conversation);
    }
    if let Some(open) = state.inner.lock().await.open.as_mut() {
        open.cancel = None;
    }
    if greet {
        spawn(app, Start::Greet);
    }
}

/// What the user pointed at, for the AI (English: notes are for the model).
pub fn selection_note(p: &Picked) -> String {
    let id = match (&p.ak_id, p.exact) {
        (Some(id), true) => format!(" data-ak-id=\"{id}\""),
        (Some(id), false) => format!(" (inside the element with data-ak-id=\"{id}\")"),
        (None, _) => " (no data-ak-id nearby)".to_owned(),
    };
    let text = p
        .text
        .as_deref()
        .map(|t| format!(", text \"{t}\""))
        .unwrap_or_default();
    format!(
        "With this message the user pointed at an element in the preview: <{}>{id}{text}, \
         CSS path `{}`. \"This\", \"here\" and similar words mean that element. \
         get_selection has the details.",
        p.tag, p.path
    )
}

/// A fixed request in the user's language (continue, try another way).
fn words(id: &str) -> String {
    I18n::new(ui_locale())
        .map(|i| i.t(id))
        .unwrap_or_else(|_| id.to_owned())
}

#[tauri::command]
pub async fn send_message<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: SendMessage,
) -> Res<()> {
    let note = {
        let mut inner = state.inner.lock().await;
        let open = inner
            .open
            .as_mut()
            .ok_or_else(|| fail("no project is open"))?;
        open.preview.set_selection(request.selection.clone());
        let added = crate::assets::note(&std::mem::take(&mut open.unannounced));
        let renamed = crate::assets::rename_note(&std::mem::take(&mut open.unannounced_renames));
        let notes: Vec<String> = [
            request.selection.as_ref().map(selection_note),
            added,
            renamed,
        ]
        .into_iter()
        .flatten()
        .collect();
        (!notes.is_empty()).then(|| notes.join("\n\n"))
    };
    spawn(
        app,
        Start::Message {
            text: request.text,
            note,
        },
    );
    Ok(())
}

#[tauri::command]
pub fn answer_confirmation(state: State<'_, AppState>, request: Answer<bool>) -> Res<()> {
    let tx = state
        .pending
        .lock()
        .ok()
        .and_then(|mut p| p.confirmations.remove(&request.request_id));
    tx.map(|t| t.send(request.answer))
        .ok_or_else(|| fail("nothing is waiting for that answer"))?
        .map_err(|_| fail("the step no longer waits"))
}

#[tauri::command]
pub fn answer_plan(state: State<'_, AppState>, request: Answer<PlanDecision>) -> Res<()> {
    let tx = state
        .pending
        .lock()
        .ok()
        .and_then(|mut p| p.plans.remove(&request.request_id));
    tx.map(|t| t.send(request.answer))
        .ok_or_else(|| fail("nothing is waiting for that answer"))?
        .map_err(|_| fail("the plan no longer waits"))
}

#[tauri::command]
pub fn answer_question(state: State<'_, AppState>, request: Answer<String>) -> Res<()> {
    let tx = state
        .pending
        .lock()
        .ok()
        .and_then(|mut p| p.questions.remove(&request.request_id));
    tx.map(|t| t.send(Some(request.answer)))
        .ok_or_else(|| fail("nothing is waiting for that answer"))?
        .map_err(|_| fail("the question no longer waits"))
}

#[tauri::command]
pub async fn cancel_turn(state: State<'_, AppState>) -> Res<()> {
    if let Some(c) = state
        .inner
        .lock()
        .await
        .open
        .as_ref()
        .and_then(|o| o.cancel.clone())
    {
        c.cancel();
    }
    // Whatever waited for an answer stops waiting.
    if let Ok(mut p) = state.pending.lock() {
        *p = Pending::default();
    }
    Ok(())
}

#[tauri::command]
pub fn continue_turn<R: Runtime>(app: AppHandle<R>) -> Res<()> {
    spawn(
        app,
        Start::Message {
            text: words("agent-continue-request"),
            note: None,
        },
    );
    Ok(())
}

#[tauri::command]
pub async fn stuck_choice<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: StuckChoice,
) -> Res<()> {
    match request {
        StuckChoice::TryAnotherWay => {
            spawn(
                app,
                Start::Message {
                    text: words("agent-try-another-request"),
                    note: None,
                },
            );
            Ok(())
        }
        StuckChoice::RollBack => {
            let (root, before, preview) = {
                let inner = state.inner.lock().await;
                let open = inner
                    .open
                    .as_ref()
                    .ok_or_else(|| fail("no project is open"))?;
                (
                    open.root.clone(),
                    open.before_turn.clone(),
                    open.preview.clone(),
                )
            };
            let id = before.ok_or_else(|| fail("there is no earlier version"))?;
            let restored = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
                let mut t = Timeline::open(&root)?;
                let label = t
                    .entries()
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.label.clone())
                    .unwrap_or_default();
                Ok(t.restore(&id, &label)?.changed_files)
            })
            .await
            .map_err(fail)?
            .map_err(fail)?;
            let _ = preview.files_changed(&restored).await;
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn resolve_interruption<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: InterruptionChoice,
) -> Res<()> {
    match request {
        InterruptionChoice::Continue => {
            spawn(app, Start::Resume);
            Ok(())
        }
        InterruptionChoice::GoBack => {
            let root = state
                .inner
                .lock()
                .await
                .open
                .as_ref()
                .map(|o| o.root.clone())
                .ok_or_else(|| fail("no project is open"))?;
            let label = words("timeline-abandoned");
            tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                Timeline::open(&root)?.discard_interrupted(&label)?;
                Ok(())
            })
            .await
            .map_err(fail)?
            .map_err(fail)
        }
    }
}

#[cfg(test)]
mod tests {
    use ak_types::preview::{Rect, Size};

    use super::*;

    #[test]
    fn streamed_words_are_joined_and_keep_their_order() {
        let text = |t: &str| AgentEvent::TextDelta { text: t.into() };
        let think = |t: &str| AgentEvent::ThinkingDelta { text: t.into() };
        let mut d = Deltas::default();
        assert!(d.push(think("Hm")).is_empty());
        assert!(d.push(think("m.")).is_empty());
        assert_eq!(
            d.push(AgentEvent::ThinkingEnd),
            [think("Hmm."), AgentEvent::ThinkingEnd]
        );
        assert!(d.push(text("Hel")).is_empty());
        assert!(d.push(text("lo")).is_empty());
        assert_eq!(d.push(think("Then")), [text("Hello")]);
        assert_eq!(d.flush(), Some(think("Then")));
        assert_eq!(d.flush(), None);
    }

    #[test]
    fn allowed_folders_are_existing_absolute_folders_and_the_ai_hears_of_them() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap_or_else(|e| panic!("{e}"));
        let given = [
            dir.path().display().to_string(),
            "relative/folder".to_owned(),
            dir.path().join("missing").display().to_string(),
            file.display().to_string(),
        ];
        let allowed = allowed_folders(&given);
        assert_eq!(allowed, [dir.path().to_path_buf()]);
        let note = folders_note(&allowed).unwrap_or_default();
        assert!(note.contains(&dir.path().display().to_string()) && note.contains("never change"));
        assert_eq!(folders_note(&[]), None);
        let note = library_note(dir.path());
        assert!(
            note.contains(
                &dir.path()
                    .join("registry")
                    .join("src")
                    .display()
                    .to_string()
            ) && note.contains("ak/ak-ui/src")
                && !note.contains("  ")
        );
        let note = app_data_note(dir.path());
        assert!(note.contains(&dir.path().display().to_string()) && note.contains("Read only"));
    }

    #[test]
    fn the_selection_note_names_the_element_for_the_ai() {
        let mut p = Picked {
            ak_id: Some("greet-button".into()),
            exact: true,
            path: "main > button".into(),
            tag: "button".into(),
            text: Some("Greet".into()),
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            viewport: Size {
                width: 1.0,
                height: 1.0,
            },
            canvas: None,
        };
        let note = selection_note(&p);
        assert!(
            note.contains("<button> data-ak-id=\"greet-button\""),
            "{note}"
        );
        assert!(note.contains("text \"Greet\"") && note.contains("main > button"));
        p.exact = false;
        assert!(selection_note(&p).contains("inside the element with data-ak-id"));
    }
}
