//! The turn loop: model -> tool calls -> results -> model, until the model is done.

use std::path::PathBuf;
use std::sync::Arc;

use ak_providers::{
    ChatRequest, Effort, MessageAssembler, Provider, ProviderError, StopReason, StreamEvent, Usage,
};
use ak_types::agent::{AgentEvent, PlanDecision, PlanProposal, TokenUsage, TurnOutcome};
use ak_types::chat::{ContentPart, Conversation, Message, ToolCall};
use ak_types::i18n::Locale;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::lock::TurnLock;
use crate::new_id;
use crate::permission::{ConfirmRequest, Confirmer, Decision, DeclineAll, PermissionGate};
use crate::plan::{CancelPlans, PlanReviewer};
use crate::policy::{PermissionMode, ProjectPolicy};
use crate::prompt::{PLAN_TOOL, PlanMode, plan_tool_spec, system_prompt};
use crate::tool::{ToolContext, ToolOutput, ToolRegistry, split_label};

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

pub struct Agent {
    provider: Arc<dyn Provider>,
    tools: ToolRegistry,
    gate: Arc<dyn PermissionGate>,
    confirmer: Arc<dyn Confirmer>,
    checkpointer: Arc<dyn Checkpointer>,
    reviewer: Arc<dyn PlanReviewer>,
    config: AgentConfig,
    /// Built once: never changes during a conversation.
    system: String,
}

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
            config,
            system,
        }
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
        append(conversation, events, user);

        let mut touched = Vec::new();
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
                Ok(Some((id, label))) => conversation.push_checkpoint(id, label),
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
            emit(
                events,
                AgentEvent::Usage {
                    usage: token_usage(assembled.usage),
                },
            );
            if let StopReason::Refusal { category } = assembled.stop_reason {
                // Partial output of a declined answer is discarded, not kept.
                return TurnOutcome::Refused { category };
            }
            let calls = match &assembled.message {
                Message::Assistant { tool_calls, .. } => tool_calls.clone(),
                _ => Vec::new(),
            };
            append(conversation, events, assembled.message);
            if calls.is_empty() {
                return match assembled.stop_reason {
                    StopReason::MaxTokens => TurnOutcome::MaxTokens,
                    _ => TurnOutcome::Completed,
                };
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
                append(
                    conversation,
                    events,
                    Message::ToolResult {
                        call_id: call.id.clone(),
                        content: output.content,
                        is_error: output.is_error,
                    },
                );
            }
            for t in &step_touched {
                if !touched.contains(t) {
                    touched.push(t.clone());
                }
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

fn append(conversation: &mut Conversation, events: &Events, message: Message) {
    conversation.push_message(message.clone());
    emit(events, AgentEvent::MessageAppended { message });
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
