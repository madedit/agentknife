//! The turn loop: model -> tool calls -> results -> model, until the model is done.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ak_providers::{
    ChatRequest, Effort, MessageAssembler, Provider, ProviderError, StopReason, StreamEvent, Usage,
};
use ak_types::agent::{
    AgentEvent, ConfirmKind, ConfirmReason, PlanDecision, PlanProposal, TokenUsage, TurnOutcome,
};
use ak_types::chat::{ContentPart, Conversation, Entry, Message, ToolCall};
use ak_types::i18n::Locale;
use ak_types::preview::ServerState;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::context;
use crate::lock::TurnLock;
use crate::new_id;
use crate::permission::{ConfirmRequest, Confirmer, Decision, DeclineAll, PermissionGate};
use crate::plan::{CancelPlans, PlanReviewer};
use crate::policy::{PermissionMode, ProjectPolicy};
use crate::prompt::{PLAN_TOOL, PlanMode, greeting_note, plan_tool_spec, system_prompt};
use crate::tool::{ToolContext, ToolOutput, ToolRegistry, split_label};
use crate::tools::preview::{PreviewAccess, describe_status};
use crate::usage::{CapStatus, CostBasis, Totals, UsageLedger};

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: String,
    /// Product instructions; behaviour rules are appended (see `prompt`).
    pub system: Option<String>,
    pub max_tokens: u32,
    pub effort: Option<Effort>,
    /// Model calls per turn before stopping with [`TurnOutcome::StepLimit`].
    pub max_steps: u32,
    /// Project root; `None` disables `turn.lock` (tests, scratch chats).
    pub project_dir: Option<PathBuf>,
    /// Fallback reply language when a message has no words.
    pub ui_locale: Locale,
    pub plan_mode: PlanMode,
    /// Automatic fixes after a failed build/run before the turn stops as
    /// [`TurnOutcome::Stuck`].
    pub max_fix_attempts: u32,
    /// The model's context size (from `Provider::capabilities`); `None` disables
    /// compaction.
    pub context_window: Option<u32>,
    /// Compact when the context is this full (0–1).
    pub compact_at: f32,
    /// How cost is known (`usage::cost_basis`); ak-app decides it from the provider.
    pub cost_basis: CostBasis,
    /// Daily spending limit in US dollars, across all projects.
    pub daily_cap_usd: Option<f64>,
}

impl AgentConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: None,
            max_tokens: 64_000,
            effort: None,
            max_steps: 50,
            project_dir: None,
            ui_locale: Locale::FALLBACK,
            plan_mode: PlanMode::Auto,
            max_fix_attempts: 3,
            context_window: None,
            compact_at: 0.7,
            cost_basis: CostBasis::Unknown,
            daily_cap_usd: None,
        }
    }
}

/// Per-turn state of the behaviour rules.
#[derive(Default)]
struct TurnState {
    plan_approved: bool,
    /// Consecutive failed builds/runs.
    failures: u32,
    last_error: String,
}

/// Tools whose failure counts towards auto-fix.
const BUILD_TOOLS: &[&str] = &["run_command"];

/// How long the end-of-turn build check waits for the preview to rebuild.
const BUILD_CHECK_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

enum BuildCheck {
    Fine,
    /// The model gets the error and another step.
    FixIt,
    Stop(TurnOutcome),
}

/// What a finished turn did, for the checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    pub turn_id: String,
    pub request: String,
    pub touched_files: Vec<String>,
}

/// Creates the end-of-turn checkpoint (P1-PROJ-003). Returns `(id, label)` of the
/// checkpoint to record in the conversation, or `None` if nothing changed.
pub trait Checkpointer: Send + Sync {
    fn checkpoint<'a>(
        &'a self,
        summary: &'a TurnSummary,
    ) -> BoxFuture<'a, Result<Option<(String, String)>, String>>;
}

/// No checkpoints, until P1-PROJ-003.
pub struct NoCheckpoints;

impl Checkpointer for NoCheckpoints {
    fn checkpoint<'a>(
        &'a self,
        _: &'a TurnSummary,
    ) -> BoxFuture<'a, Result<Option<(String, String)>, String>> {
        Box::pin(async { Ok(None) })
    }
}

/// Saves the conversation after every change, so a crash loses at most the step in
/// progress. ak-app implements it with `ak_project::Project::save_conversation`.
pub trait ConversationStore: Send + Sync {
    fn save(&self, conversation: &Conversation) -> Result<(), String>;
}

/// Keeps nothing (tests, scratch chats).
pub struct NoStore;

impl ConversationStore for NoStore {
    fn save(&self, _: &Conversation) -> Result<(), String> {
        Ok(())
    }
}

pub struct Agent {
    provider: Arc<dyn Provider>,
    tools: ToolRegistry,
    gate: Arc<dyn PermissionGate>,
    confirmer: Arc<dyn Confirmer>,
    checkpointer: Arc<dyn Checkpointer>,
    reviewer: Arc<dyn PlanReviewer>,
    store: Arc<dyn ConversationStore>,
    /// Context size the provider reported for the latest call (any turn).
    last_context_tokens: AtomicU64,
    ledger: Arc<UsageLedger>,
    session: Mutex<Totals>,
    config: AgentConfig,
    /// Built once: never changes during a conversation.
    system: String,
    /// The project's running preview: told about file changes, and checked for a
    /// working build before a turn that changed files ends.
    preview: Option<Arc<dyn PreviewAccess>>,
    /// Gives the files a turn changed their missing `data-ak-id`s.
    id_pass: Option<IdPass>,
}

/// `(path relative to the project, source) -> new source`, `None` when nothing
/// changes (the stack profile's `add_missing_ids`).
pub type IdPass = Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

type Events = UnboundedSender<AgentEvent>;

impl Agent {
    pub fn new(provider: Arc<dyn Provider>, tools: ToolRegistry, config: AgentConfig) -> Self {
        let system = system_prompt(config.system.as_deref(), config.ui_locale, config.plan_mode);
        Self {
            provider,
            tools,
            gate: Arc::new(ProjectPolicy::new(PermissionMode::Standard)),
            confirmer: Arc::new(DeclineAll),
            checkpointer: Arc::new(NoCheckpoints),
            reviewer: Arc::new(CancelPlans),
            store: Arc::new(NoStore),
            last_context_tokens: AtomicU64::new(0),
            ledger: Arc::new(UsageLedger::in_memory()),
            session: Mutex::new(Totals::default()),
            config,
            system,
            preview: None,
            id_pass: None,
        }
    }

    /// Connects the id pass (see the `id_pass` field), run when the model ends
    /// a turn that changed files, before the build check.
    pub fn with_id_pass(mut self, pass: IdPass) -> Self {
        self.id_pass = Some(pass);
        self
    }

    /// Connects the project's preview (see the `preview` field).
    pub fn with_preview(mut self, preview: Arc<dyn PreviewAccess>) -> Self {
        self.preview = Some(preview);
        self
    }

    pub fn with_plan_reviewer(mut self, reviewer: Arc<dyn PlanReviewer>) -> Self {
        self.reviewer = reviewer;
        self
    }

    /// The exact system prompt sent with every request.
    pub fn system_prompt(&self) -> &str {
        &self.system
    }

    fn tool_specs(&self) -> Vec<ak_types::chat::ToolSpec> {
        let mut specs = self.tools.specs();
        if self.config.plan_mode != PlanMode::NeverAsk {
            specs.push(plan_tool_spec());
        }
        specs
    }

    pub fn with_gate(mut self, gate: Arc<dyn PermissionGate>) -> Self {
        self.gate = gate;
        self
    }

    pub fn with_confirmer(mut self, confirmer: Arc<dyn Confirmer>) -> Self {
        self.confirmer = confirmer;
        self
    }

    pub fn with_checkpointer(mut self, checkpointer: Arc<dyn Checkpointer>) -> Self {
        self.checkpointer = checkpointer;
        self
    }

    /// Shares today's spending across agents (one ledger per AgentKnife run).
    pub fn with_usage_ledger(mut self, ledger: Arc<UsageLedger>) -> Self {
        self.ledger = ledger;
        self
    }

    pub fn with_store(mut self, store: Arc<dyn ConversationStore>) -> Self {
        self.store = store;
        self
    }

    /// Call when a project session starts: appends the project notes
    /// (`PROJECT.md`, `plan/STATUS.md`, `plan/tasks.md`) as a context entry, unless
    /// they are unchanged since the last one. Returns whether anything was added.
    pub fn open_session(&self, conversation: &mut Conversation) -> Result<bool, String> {
        let Some(notes) = self
            .config
            .project_dir
            .as_deref()
            .and_then(context::project_notes)
        else {
            return Ok(false);
        };
        if conversation.last_context() == Some(notes.as_str()) {
            return Ok(false);
        }
        conversation.push_context(notes);
        self.store.save(conversation)?;
        Ok(true)
    }

    /// The first message after the user connects an AI: a greeting in the UI
    /// language that explains AgentKnife and proposes checking the development tools.
    /// `None` (nothing done) once the conversation has any message.
    pub async fn greet(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        cancel: &CancellationToken,
    ) -> Option<TurnOutcome> {
        let started = conversation
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::Message(_) | Entry::Compaction { .. }));
        if started {
            return None;
        }
        conversation.push_context(greeting_note(self.config.ui_locale));
        if let Err(e) = self.store.save(conversation) {
            return Some(finish(events, save_failed(e)));
        }
        let turn_id = new_id("turn");
        emit(
            events,
            AgentEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        );
        // Nothing in the project changes: no lock, and the checkpointer finds no change.
        Some(
            self.drive(
                conversation,
                events,
                cancel,
                turn_id,
                String::new(),
                None,
                Vec::new(),
            )
            .await,
        )
    }

    /// Runs one user turn to completion. Appends to `conversation` only (never edits
    /// it) and streams progress to `events`.
    pub async fn run_turn(
        &self,
        conversation: &mut Conversation,
        user: Message,
        events: &Events,
        cancel: &CancellationToken,
    ) -> TurnOutcome {
        let turn_id = new_id("turn");
        emit(
            events,
            AgentEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        );
        let request = plain_text(&user);
        let mut lock = None;
        if let Some(dir) = &self.config.project_dir {
            match TurnLock::acquire(dir, &turn_id, &request) {
                Ok(l) => lock = Some(l),
                Err(e) => {
                    return finish(
                        events,
                        TurnOutcome::Failed {
                            message: format!("could not write the turn lock: {e}"),
                            retryable: false,
                        },
                    );
                }
            }
        }
        if let Err(stop) = self.append(conversation, events, user) {
            return finish(events, stop);
        }
        self.drive(
            conversation,
            events,
            cancel,
            turn_id,
            request,
            lock,
            Vec::new(),
        )
        .await
    }

    /// Continues a turn that was interrupted (crash, force-quit, failure), as recorded
    /// in `turn.lock`. Unanswered tool calls get "interrupted" results first, so the
    /// history is well-formed. `None` when there is nothing to resume.
    pub async fn resume_turn(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        cancel: &CancellationToken,
    ) -> Option<TurnOutcome> {
        let dir = self.config.project_dir.as_deref()?;
        let lock = TurnLock::resume(dir).ok().flatten()?;
        let data = lock.data().clone();
        emit(
            events,
            AgentEvent::TurnStarted {
                turn_id: data.turn_id.clone(),
            },
        );
        for call_id in dangling_tool_calls(conversation) {
            let repair = Message::ToolResult {
                call_id,
                content: vec![ContentPart::Text {
                    text: "Interrupted before this step finished. Check the current state \
                           before relying on it."
                        .into(),
                }],
                is_error: true,
            };
            if let Err(stop) = self.append(conversation, events, repair) {
                return Some(finish(events, stop));
            }
        }
        let files = if data.touched_files.is_empty() {
            "none recorded".to_owned()
        } else {
            data.touched_files.join(", ")
        };
        conversation.push_context(format!(
            "The previous turn was interrupted (AgentKnife closed or failed) while working \
             on the user's request: \u{ab}{}\u{bb}. Files changed so far: {files}. Look at \
             their current state and finish that request; do not start over. Reply in the \
             language of that request.",
            data.request
        ));
        if let Err(e) = self.store.save(conversation) {
            return Some(finish(events, save_failed(e)));
        }
        Some(
            self.drive(
                conversation,
                events,
                cancel,
                data.turn_id,
                data.request,
                Some(lock),
                data.touched_files,
            )
            .await,
        )
    }

    /// Runs the steps of a started turn, then settles it: checkpoint, then release
    /// the lock (only for turns that ended normally).
    #[allow(clippy::too_many_arguments)]
    async fn drive(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        cancel: &CancellationToken,
        turn_id: String,
        request: String,
        mut lock: Option<TurnLock>,
        mut touched: Vec<String>,
    ) -> TurnOutcome {
        let outcome = self
            .steps(
                conversation,
                events,
                cancel,
                &turn_id,
                &mut lock,
                &mut touched,
            )
            .await;

        if outcome.ended_normally() {
            let summary = TurnSummary {
                turn_id,
                request,
                touched_files: touched,
            };
            match self.checkpointer.checkpoint(&summary).await {
                Ok(Some((id, label))) => {
                    conversation.push_checkpoint(id, label);
                    if let Err(e) = self.store.save(conversation) {
                        return finish(events, save_failed(e));
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    // Keep the lock: without a checkpoint this turn is not settled.
                    return finish(
                        events,
                        TurnOutcome::Failed {
                            message: format!("could not save a checkpoint: {e}"),
                            retryable: true,
                        },
                    );
                }
            }
            if let Some(lock) = lock
                && let Err(e) = lock.release()
            {
                return finish(
                    events,
                    TurnOutcome::Failed {
                        message: format!("could not remove the turn lock: {e}"),
                        retryable: false,
                    },
                );
            }
        }
        finish(events, outcome)
    }

    async fn steps(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        cancel: &CancellationToken,
        turn_id: &str,
        lock: &mut Option<TurnLock>,
        touched: &mut Vec<String>,
    ) -> TurnOutcome {
        let ctx = ToolContext {
            project_dir: self.config.project_dir.clone(),
            cancel: cancel.clone(),
            events: Some(events.clone()),
        };
        let mut state = TurnState::default();
        for step in 0..self.config.max_steps {
            if let Err(stop) = self.maybe_compact(conversation, events, cancel).await {
                return stop;
            }
            let request = ChatRequest {
                model: self.config.model.clone(),
                system: Some(self.system.clone()),
                messages: conversation.messages(),
                tools: self.tool_specs(),
                max_tokens: self.config.max_tokens,
                effort: self.config.effort,
            };
            let assembled = match self.model_call(request, events, cancel).await {
                Ok(a) => a,
                Err(stop) => return stop,
            };
            self.record_usage(events, assembled.usage);
            if let StopReason::Refusal { category } = assembled.stop_reason {
                // Partial output of a declined answer is discarded, not kept.
                return TurnOutcome::Refused { category };
            }
            self.last_context_tokens.store(
                assembled.usage.input_tokens
                    + assembled.usage.cache_read_tokens
                    + assembled.usage.cache_write_tokens,
                Ordering::Relaxed,
            );
            let calls = match &assembled.message {
                Message::Assistant { tool_calls, .. } => tool_calls.clone(),
                _ => Vec::new(),
            };
            if let Err(stop) = self.append(conversation, events, assembled.message) {
                return stop;
            }
            if calls.is_empty() {
                if assembled.stop_reason == StopReason::MaxTokens {
                    return TurnOutcome::MaxTokens;
                }
                // The model is done. What it added gets ids the user can point
                // at, and if it changed files, the app must still build.
                self.add_ids(touched).await;
                match self
                    .check_build(conversation, events, touched, &mut state)
                    .await
                {
                    BuildCheck::Fine => return TurnOutcome::Completed,
                    BuildCheck::FixIt => continue,
                    BuildCheck::Stop(outcome) => return outcome,
                }
            }

            let mut step_touched = Vec::new();
            let mut cancelled = false;
            for (i, call) in calls.iter().enumerate() {
                let output = if cancelled {
                    // Every tool call needs a result, or the history is malformed.
                    ToolOutput::error("Cancelled by the user before this step ran.")
                } else {
                    let step_id = format!("{turn_id}-{step}-{i}");
                    match self
                        .run_tool(call, &step_id, events, &ctx, &mut state)
                        .await
                    {
                        Some(out) => out,
                        None => {
                            cancelled = true;
                            ToolOutput::error("Cancelled by the user.")
                        }
                    }
                };
                if !cancelled && BUILD_TOOLS.contains(&call.name.as_str()) {
                    if output.is_error {
                        state.failures += 1;
                        state.last_error = output_text(&output);
                    } else {
                        state.failures = 0;
                    }
                }
                step_touched.extend(output.touched.iter().cloned());
                if let Err(stop) = self.append(
                    conversation,
                    events,
                    Message::ToolResult {
                        call_id: call.id.clone(),
                        content: output.content,
                        is_error: output.is_error,
                    },
                ) {
                    return stop;
                }
            }
            for t in &step_touched {
                if !touched.contains(t) {
                    touched.push(t.clone());
                }
            }
            if !step_touched.is_empty()
                && let Some(preview) = &self.preview
            {
                // Backend code needs a restart; the screens rebuild by themselves.
                preview.files_changed(&step_touched).await;
            }
            if let Some(l) = lock.as_mut()
                && let Err(e) = l.record_step(&step_touched)
            {
                return TurnOutcome::Failed {
                    message: format!("could not update the turn lock: {e}"),
                    retryable: false,
                };
            }
            if cancelled {
                return TurnOutcome::Cancelled;
            }
            if state.failures > self.config.max_fix_attempts {
                return TurnOutcome::Stuck {
                    attempts: self.config.max_fix_attempts,
                    last_error: std::mem::take(&mut state.last_error),
                };
            }
        }
        TurnOutcome::StepLimit
    }

    /// Runs the id pass over the turn's changed files. Best effort: a file that
    /// cannot be read or written stays as it is.
    async fn add_ids(&self, touched: &[String]) {
        let (Some(pass), Some(root)) = (&self.id_pass, &self.config.project_dir) else {
            return;
        };
        let mut changed = Vec::new();
        for rel in touched {
            let path = root.join(rel);
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(new) = pass(rel, &source)
                && std::fs::write(&path, new).is_ok()
            {
                changed.push(rel.clone());
            }
        }
        if !changed.is_empty()
            && let Some(preview) = &self.preview
        {
            preview.files_changed(&changed).await;
        }
    }

    /// After a turn that changed files: waits for the preview build. A failure
    /// goes back to the model with the compiler output (up to `max_fix_attempts`
    /// fixes, shared with failed builds/runs).
    async fn check_build(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        touched: &[String],
        state: &mut TurnState,
    ) -> BuildCheck {
        let Some(preview) = &self.preview else {
            return BuildCheck::Fine;
        };
        if touched.is_empty() || preview.status().frontend == ServerState::Off {
            return BuildCheck::Fine;
        }
        emit(events, AgentEvent::BuildChecking);
        let status = preview.settled(BUILD_CHECK_WAIT).await;
        let max = self.config.max_fix_attempts;
        let Some(error) = status.error() else {
            emit(
                events,
                AgentEvent::BuildChecked {
                    ok: true,
                    attempt: 0,
                    max,
                },
            );
            return BuildCheck::Fine;
        };
        state.failures += 1;
        state.last_error = error.to_owned();
        if state.failures > max {
            return BuildCheck::Stop(TurnOutcome::Stuck {
                attempts: max,
                last_error: std::mem::take(&mut state.last_error),
            });
        }
        emit(
            events,
            AgentEvent::BuildChecked {
                ok: false,
                attempt: state.failures,
                max,
            },
        );
        conversation.push_context(format!(
            "After your changes the app no longer builds, so the user's preview is broken.              Fix this before you finish (read the files involved first).

{}",
            describe_status(&status)
        ));
        if let Err(e) = self.store.save(conversation) {
            return BuildCheck::Stop(save_failed(e));
        }
        BuildCheck::FixIt
    }

    /// Adds a model call's usage to the session and today's ledger and reports it.
    fn record_usage(&self, events: &Events, usage: Usage) {
        let usage = token_usage(usage);
        emit(events, AgentEvent::Usage { usage });
        let basis = self.config.cost_basis;
        let today = UsageLedger::today();
        let status = self
            .ledger
            .record(&today, &usage, basis, self.config.daily_cap_usd);
        let session = match self.session.lock() {
            Ok(mut s) => {
                s.add(&usage, basis);
                s.summary()
            }
            Err(_) => return,
        };
        emit(
            events,
            AgentEvent::UsageTotals {
                session,
                today: self.ledger.day(&today).summary(),
            },
        );
        if let CapStatus::Warn { spent, cap } = status {
            emit(
                events,
                AgentEvent::BudgetWarning {
                    spent_usd: spent,
                    cap_usd: cap,
                },
            );
        }
    }

    /// At the daily cap, asks once whether to keep going today.
    async fn check_budget(
        &self,
        events: &Events,
        cancel: &CancellationToken,
    ) -> Result<(), TurnOutcome> {
        let today = UsageLedger::today();
        let Some(spent) = self.ledger.over_cap(&today, self.config.daily_cap_usd) else {
            return Ok(());
        };
        let cap = self.config.daily_cap_usd.unwrap_or_default();
        let request = ConfirmRequest {
            request_id: new_id("budget"),
            tool: String::new(),
            label: String::new(),
            reason: ConfirmReason {
                kind: ConfirmKind::DailyCap,
                detail: format!("US${spent:.2} / US${cap:.2}"),
            },
        };
        emit(
            events,
            AgentEvent::NeedsConfirmation {
                request_id: request.request_id.clone(),
                tool: String::new(),
                label: String::new(),
                reason: request.reason.clone(),
            },
        );
        let yes = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TurnOutcome::Cancelled),
            yes = self.confirmer.confirm(request) => yes,
        };
        if yes {
            self.ledger.override_cap(&today);
            Ok(())
        } else {
            Err(TurnOutcome::OverBudget {
                spent_usd: spent,
                cap_usd: cap,
            })
        }
    }

    fn append(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        message: Message,
    ) -> Result<(), TurnOutcome> {
        conversation.push_message(message.clone());
        emit(events, AgentEvent::MessageAppended { message });
        self.store.save(conversation).map_err(save_failed)
    }

    /// Compacts the model's view when it nears the context window: a summary of
    /// everything before the previous turn replaces it (append-only, see
    /// `Conversation::push_compaction`). Summary failures are not fatal: the turn
    /// goes on uncompacted.
    async fn maybe_compact(
        &self,
        conversation: &mut Conversation,
        events: &Events,
        cancel: &CancellationToken,
    ) -> Result<(), TurnOutcome> {
        let Some(window) = self.config.context_window else {
            return Ok(());
        };
        // Reported by the provider for the latest call, whatever turn it was in.
        let last_input_tokens = self.last_context_tokens.load(Ordering::Relaxed);
        let size = last_input_tokens.max(context::estimate_request(
            &self.system,
            &self.tool_specs(),
            &conversation.messages(),
        ));
        if (size as f64) < f64::from(window) * f64::from(self.config.compact_at) {
            return Ok(());
        }
        let Some(cut) = context::compaction_cut(conversation) else {
            return Ok(());
        };
        let view = conversation.view();
        let Some(keep_from) = view[cut].0 else {
            return Ok(());
        };
        let mut messages: Vec<Message> = view[..cut].iter().map(|(_, m)| m.clone()).collect();
        messages.push(Message::user_text(context::COMPACT_INSTRUCTION));
        let request = ChatRequest {
            model: self.config.model.clone(),
            system: Some(self.system.clone()),
            messages,
            tools: self.tool_specs(),
            max_tokens: self.config.max_tokens.min(8_192),
            effort: self.config.effort,
        };
        self.check_budget(events, cancel).await?;
        let summarize = async {
            let stream = self.provider.stream(request).await?;
            let assembled = ak_providers::collect(stream, self.provider.id()).await?;
            self.record_usage(events, assembled.usage);
            Ok::<_, ak_providers::ProviderError>(assembled)
        };
        let assembled = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TurnOutcome::Cancelled),
            a = summarize => a,
        };
        let summary = match assembled.map(|a| a.message) {
            Ok(Message::Assistant { text, .. }) if !text.trim().is_empty() => text,
            Ok(_) => {
                tracing::warn!("compaction skipped: the summary was empty");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(error = %e, "compaction skipped: the summary call failed");
                return Ok(());
            }
        };
        conversation.push_compaction(summary, keep_from);
        emit(events, AgentEvent::ContextCompacted);
        self.store.save(conversation).map_err(save_failed)
    }

    /// The `propose_plan` tool: show the plan, wait for the user's decision.
    async fn review_plan(
        &self,
        call: &ToolCall,
        step_id: &str,
        events: &Events,
        ctx: &ToolContext,
        state: &mut TurnState,
    ) -> Option<ToolOutput> {
        let args = &call.arguments;
        let steps: Vec<String> = args["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                s.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .collect();
        let title = args["title"].as_str().unwrap_or_default().trim().to_owned();
        if call.invalid_arguments.is_some() || steps.is_empty() || title.is_empty() {
            return Some(ToolOutput::error(
                "A plan needs a `title` and 3 to 5 `steps`. Call propose_plan again.",
            ));
        }
        let plan = PlanProposal {
            title,
            steps,
            estimate: args["estimate"].as_str().map(str::to_owned),
        };
        let request_id = format!("{step_id}-plan");
        emit(
            events,
            AgentEvent::PlanProposed {
                request_id: request_id.clone(),
                plan: plan.clone(),
            },
        );
        let decision = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => return None,
            d = self.reviewer.review(request_id, plan) => d,
        };
        Some(match decision {
            PlanDecision::Start => {
                state.plan_approved = true;
                ToolOutput::text("The user approved the plan. Carry it out now.")
            }
            PlanDecision::Revise { feedback } => ToolOutput::text(format!(
                "The user wants changes to the plan: {feedback}\nPropose a revised plan."
            )),
            PlanDecision::Cancel => ToolOutput::text(
                "The user cancelled the plan. Make no changes; acknowledge briefly and ask \
                 what they would like instead.",
            ),
        })
    }

    /// One model call, streamed to `events`. A cancelled call is not appended.
    async fn model_call(
        &self,
        request: ChatRequest,
        events: &Events,
        cancel: &CancellationToken,
    ) -> Result<ak_providers::Assembled, TurnOutcome> {
        self.check_budget(events, cancel).await?;
        // `biased`: when both are ready, cancellation wins (no random pick).
        let started = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(TurnOutcome::Cancelled),
            r = self.provider.stream(request) => r,
        };
        let mut stream = started.map_err(failed)?;
        let mut assembler = MessageAssembler::new(self.provider.id());
        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(TurnOutcome::Cancelled),
                n = stream.next() => n,
            };
            match next {
                None => break,
                Some(Err(e)) => return Err(failed(e)),
                Some(Ok(ev)) => {
                    forward(events, &ev);
                    assembler.push(ev).map_err(failed)?;
                }
            }
        }
        assembler.finish().map_err(failed)
    }

    /// Runs one tool call. `None` means the user cancelled.
    async fn run_tool(
        &self,
        call: &ToolCall,
        step_id: &str,
        events: &Events,
        ctx: &ToolContext,
        state: &mut TurnState,
    ) -> Option<ToolOutput> {
        if call.name == PLAN_TOOL && self.config.plan_mode != PlanMode::NeverAsk {
            return self.review_plan(call, step_id, events, ctx, state).await;
        }
        let (label, args) = split_label(call.arguments.clone());
        let label = label.unwrap_or_else(|| call.name.clone());
        emit(
            events,
            AgentEvent::StepStarted {
                step_id: step_id.to_owned(),
                tool: call.name.clone(),
                label: label.clone(),
                detail: step_detail(&args),
            },
        );
        let output = if call.invalid_arguments.is_some() {
            ToolOutput::error(
                "The arguments were not valid JSON (probably cut off by the output limit). \
                 Call the tool again with complete, smaller arguments.",
            )
        } else if let Some(tool) = self.tools.get(&call.name) {
            let needs_plan = self.config.plan_mode == PlanMode::AlwaysAsk
                && tool.changes_project()
                && !state.plan_approved;
            let allowed = match self.gate.check(&call.name, &args, ctx) {
                _ if needs_plan => Err(ToolOutput::error(format!(
                    "Before changing the project, call {PLAN_TOOL} and wait for the user's approval."
                ))),
                Decision::Allow => Ok(()),
                Decision::Deny { reason } => Err(ToolOutput::error(format!(
                    "Not allowed because {} ({}). Find another way that stays inside \
                         the project, or explain to the user why it is needed.",
                    reason.kind.for_model(),
                    reason.detail
                ))),
                Decision::Ask { reason } => {
                    let request = ConfirmRequest {
                        request_id: format!("{step_id}-confirm"),
                        tool: call.name.clone(),
                        label: label.clone(),
                        reason: reason.clone(),
                    };
                    emit(
                        events,
                        AgentEvent::NeedsConfirmation {
                            request_id: request.request_id.clone(),
                            tool: request.tool.clone(),
                            label: request.label.clone(),
                            reason,
                        },
                    );
                    let yes = tokio::select! {
                        biased;
                        _ = ctx.cancel.cancelled() => return None,
                        yes = self.confirmer.confirm(request) => yes,
                    };
                    if yes {
                        Ok(())
                    } else {
                        Err(ToolOutput::error(
                            "The user declined this step. Ask what they would prefer instead.",
                        ))
                    }
                }
            };
            match allowed {
                Ok(()) => {
                    let out = tokio::select! {
                        biased;
                        _ = ctx.cancel.cancelled() => return None,
                        out = tool.call(args, ctx) => out,
                    };
                    // A tool that stopped because of the cancel is still a cancel.
                    if ctx.cancel.is_cancelled() {
                        return None;
                    }
                    out
                }
                Err(refusal) => refusal,
            }
        } else {
            ToolOutput::error(format!("There is no tool named `{}`.", call.name))
        };
        emit(
            events,
            AgentEvent::StepFinished {
                step_id: step_id.to_owned(),
                ok: !output.is_error,
                output: details_text(&output),
            },
        );
        Some(output)
    }
}

fn failed(e: ProviderError) -> TurnOutcome {
    TurnOutcome::Failed {
        message: e.to_string(),
        retryable: e.is_retryable(),
    }
}

fn forward(events: &Events, ev: &StreamEvent) {
    let out = match ev {
        StreamEvent::TextDelta(t) => AgentEvent::TextDelta { text: t.clone() },
        StreamEvent::ThinkingDelta(t) => AgentEvent::ThinkingDelta { text: t.clone() },
        StreamEvent::ThinkingEnd => AgentEvent::ThinkingEnd,
        _ => return,
    };
    emit(events, out);
}

fn save_failed(e: String) -> TurnOutcome {
    TurnOutcome::Failed {
        message: format!("could not save the conversation: {e}"),
        retryable: true,
    }
}

/// Tool calls in the last assistant message that have no result yet.
fn dangling_tool_calls(conversation: &Conversation) -> Vec<String> {
    let messages = conversation.messages();
    let Some(pos) = messages
        .iter()
        .rposition(|m| matches!(m, Message::Assistant { .. }))
    else {
        return Vec::new();
    };
    let Message::Assistant { tool_calls, .. } = &messages[pos] else {
        return Vec::new();
    };
    let answered: Vec<&str> = messages[pos + 1..]
        .iter()
        .filter_map(|m| match m {
            Message::ToolResult { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    tool_calls
        .iter()
        .filter(|c| !answered.contains(&c.id.as_str()))
        .map(|c| c.id.clone())
        .collect()
}

fn emit(events: &Events, event: AgentEvent) {
    // A closed channel means nobody is watching; the turn still completes.
    let _ = events.send(event);
}

fn finish(events: &Events, outcome: TurnOutcome) -> TurnOutcome {
    emit(
        events,
        AgentEvent::TurnFinished {
            outcome: outcome.clone(),
        },
    );
    outcome
}

fn token_usage(u: Usage) -> TokenUsage {
    TokenUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cache_read_tokens,
        cache_write_tokens: u.cache_write_tokens,
    }
}

/// The text of a tool result, shortened for [`TurnOutcome::Stuck`].
fn output_text(out: &ToolOutput) -> String {
    let text: String = out
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(text.as_str()),
            ContentPart::Image { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let chars = text.chars().count();
    // The end of a build log carries the error.
    text.chars().skip(chars.saturating_sub(2_000)).collect()
}

fn plain_text(m: &Message) -> String {
    match m {
        Message::User { content } => content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Characters of a step's arguments and output kept for the UI's "Details".
const DETAIL_CHARS: usize = 4_000;

fn shorten(text: &str) -> String {
    if text.chars().count() <= DETAIL_CHARS {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(DETAIL_CHARS).collect();
    out.push_str("\n…");
    out
}

/// The command or path of a step when it has one, else its arguments as JSON.
fn step_detail(args: &serde_json::Value) -> String {
    for key in ["command", "path", "query", "ak_id"] {
        if let Some(v) = args.get(key).and_then(|v| v.as_str()) {
            return shorten(v);
        }
    }
    match args {
        serde_json::Value::Object(m) if m.is_empty() => String::new(),
        serde_json::Value::Null => String::new(),
        other => shorten(&other.to_string()),
    }
}

fn details_text(output: &ToolOutput) -> String {
    let text: Vec<&str> = output
        .content
        .iter()
        .map(|c| match c {
            ContentPart::Text { text } => text.as_str(),
            ContentPart::Image { .. } => "[image]",
        })
        .collect();
    shorten(&text.join("\n"))
}
