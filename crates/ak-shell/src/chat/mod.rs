//! The chat pane: the conversation, the agent's steps and questions, and the
//! input with the selected-element chip.

pub mod log;
mod tools;

pub use tools::{bundle, check_tools, check_tools_quietly};

use ak_types::agent::{PlanDecision, TurnOutcome, UsageSummary};
use ak_types::ipc::{InterruptionChoice, SendMessage, StuckChoice};
use ak_types::preview::Picked;
use leptos::prelude::*;

use crate::backend::{Backend, CallError};
use crate::i18n::{t, t_with};
use log::{BuildLine, ChatLog, Item, StepState};

/// The open project's chat, shared by the chat pane and the preview (which sets
/// the selection).
#[derive(Clone, Copy)]
pub struct Chat {
    pub log: RwSignal<ChatLog>,
    /// What the user pointed at in the preview; goes with the next message.
    pub selection: RwSignal<Option<Picked>>,
}

impl Chat {
    pub fn new(log: ChatLog) -> Chat {
        Chat {
            log: RwSignal::new(log),
            selection: RwSignal::new(None),
        }
    }
}

/// The chip's words for a picked element: its own short text (`<button>
/// “Greet”`), else its id for big containers whose text is everything inside
/// (`<main> home-screen`), else the start of its text, else just its tag.
pub fn selection_label(p: &Picked) -> String {
    const SHORT: usize = 30;
    let text = p.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
    match (text, &p.ak_id) {
        (Some(t), _) if t.chars().count() <= SHORT => format!("<{}> “{t}”", p.tag),
        (_, Some(id)) => format!("<{}> {id}", p.tag),
        (Some(t), None) => {
            let short: String = t.chars().take(SHORT).collect();
            format!("<{}> “{short}…”", p.tag)
        }
        (None, None) => format!("<{}>", p.tag),
    }
}

fn money(usd: f64) -> String {
    format!("{usd:.2}")
}

/// The usage line: free, unknown, or this session's and today's cost.
pub fn usage_text(
    session: &UsageSummary,
    today: &UsageSummary,
) -> (&'static str, Vec<(&'static str, String)>) {
    if session.free && today.free {
        return ("usage-free", Vec::new());
    }
    match (session.cost_usd, today.cost_usd) {
        (Some(s), Some(d)) => (
            if session.cost_is_lower_bound || today.cost_is_lower_bound {
                "chat-usage-at-least"
            } else {
                "chat-usage"
            },
            vec![("session", money(s)), ("today", money(d))],
        ),
        _ => ("usage-unknown", Vec::new()),
    }
}

fn report(log: RwSignal<ChatLog>, e: CallError) {
    let message = match e {
        CallError::Connect(c) => c.to_string(),
        CallError::Other(d) => d,
    };
    log.update(|l| {
        l.apply(ak_types::agent::AgentEvent::TurnFinished {
            outcome: TurnOutcome::Failed {
                message,
                retryable: false,
            },
        });
    });
}

/// Runs a backend call in the background; failures appear in the chat.
fn call<F>(log: RwSignal<ChatLog>, f: impl FnOnce(Backend) -> F + 'static)
where
    F: std::future::Future<Output = Result<(), CallError>> + 'static,
{
    let backend = expect_context::<Backend>();
    leptos::task::spawn_local(async move {
        if let Err(e) = f(backend).await {
            report(log, e);
        }
    });
}

pub(crate) const PRIMARY: &str = "rounded-lg bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50";
pub(crate) const SECONDARY: &str = "rounded-lg border border-gray-300 px-3 py-1.5 text-sm hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800";
pub(crate) const CARD: &str = "rounded-xl border border-gray-200 bg-white p-3 text-sm shadow-sm dark:border-gray-700 dark:bg-gray-900";

/// Sends `message` (with the current selection) and shows it at once.
pub fn send_message(chat: Chat, message: String) {
    let selection = chat.selection.get_untracked();
    chat.log
        .update(|l| l.user_sent(&message, selection.as_ref().map(selection_label)));
    chat.selection.set(None);
    call(chat.log, move |b| async move {
        b.send_message(&SendMessage {
            text: message,
            selection,
        })
        .await
    });
}

#[component]
pub fn ChatPane() -> impl IntoView {
    let chat = expect_context::<Chat>();
    let backend = StoredValue::new(expect_context::<Backend>());
    let list = NodeRef::<leptos::html::Div>::new();
    // Keep the newest message in view (browser only: effects need its executor).
    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        chat.log.track();
        if let Some(el) = list.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });
    let usage = move || {
        chat.log.with(|l| l.usage).map(|(s, d)| {
            let (id, args) = usage_text(&s, &d);
            let i18n = expect_context::<crate::i18n::Tr>().0;
            let a = ak_types::i18n::args(args);
            i18n.with(|i| i.t_args(id, &a))
        })
    };
    let budget = move || {
        chat.log.with(|l| l.budget_warning).map(|(spent, cap)| {
            view! {
                <p data-notice="budget" class="mx-3 mb-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-100">
                    {t_with("budget-warning", [("spent", money(spent)), ("cap", money(cap))])}
                </p>
            }
        })
    };
    view! {
        <div class="flex items-baseline justify-between gap-2 px-4 pt-3 pb-2">
            <h2 class="text-xs font-semibold tracking-wide text-gray-500 uppercase dark:text-gray-400">
                {t("shell-chat")}
            </h2>
            <span data-usage="" class="truncate text-xs text-gray-500 dark:text-gray-400">{usage}</span>
        </div>
        <div class="flex gap-2 px-3 pb-2">
            <button
                type="button"
                data-action="check-tools"
                class="rounded-full border border-gray-300 px-3 py-0.5 text-xs hover:bg-gray-100 disabled:opacity-50 dark:border-gray-700 dark:hover:bg-gray-800"
                disabled=move || chat.log.with(|l| l.items.iter().any(|i| matches!(i, Item::Doctor(c) if c.busy)))
                on:click=move |_| check_tools(chat, backend.get_value(), true)
            >
                {t("chat-check-tools")}
            </button>
            <button
                type="button"
                data-action="bundle"
                title=t("chat-bundle-hint")
                class="rounded-full border border-gray-300 px-3 py-0.5 text-xs hover:bg-gray-100 disabled:opacity-50 dark:border-gray-700 dark:hover:bg-gray-800"
                disabled=move || chat.log.with(|l| l.running || l.bundling())
                on:click=move |_| bundle(chat, backend.get_value())
            >
                {t("chat-bundle")}
            </button>
        </div>
        {budget}
        <div node_ref=list data-chat="log" class="flex min-h-0 flex-1 flex-col gap-3 overflow-auto px-3 pb-3">
            <Show when=move || chat.log.with(|l| l.items.is_empty() && l.interrupted.is_none())>
                <p class="px-1 text-sm text-gray-500 dark:text-gray-400">{t("chat-empty")}</p>
            </Show>
            <For
                each=move || chat.log.with(|l| (0..l.items.len()).collect::<Vec<_>>())
                key=|i| *i
                let:index
            >
                <ItemView index />
            </For>
            {move || chat.log.with(|l| l.interrupted.clone()).map(|request| view! { <Interrupted request /> })}
        </div>
        <Composer />
    }
}

/// One chat item, re-rendered when it changes.
#[component]
fn ItemView(index: usize) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let item = Memo::new(move |_| chat.log.with(|l| l.items.get(index).cloned()));
    let last = move || chat.log.with(|l| l.running && index + 1 == l.items.len());
    move || {
        item.get().map(|i| match i {
            Item::User { text, selection } => view! {
                <div data-item="user" class="ml-8 self-end rounded-2xl rounded-br-sm bg-blue-600 px-3 py-2 text-sm text-white">
                    {selection.map(|s| view! {
                        <span class="mb-1 block text-xs text-blue-100">{t_with("chat-selected", [("what", s)])}</span>
                    })}
                    <p class="whitespace-pre-wrap">{text}</p>
                </div>
            }.into_any(),
            Item::Assistant { text, thinking } => {
                let empty = text.is_empty();
                view! {
                    <div data-item="assistant" class="mr-8 self-start rounded-2xl rounded-bl-sm bg-gray-100 px-3 py-2 text-sm dark:bg-gray-800">
                        {(!thinking.is_empty()).then(|| view! {
                            <details data-thinking="" class="mb-1 text-xs text-gray-500 dark:text-gray-400">
                                <summary class="cursor-pointer select-none">{t("chat-thinking")}</summary>
                                <p class="mt-1 whitespace-pre-wrap">{thinking}</p>
                            </details>
                        })}
                        {move || (empty && last()).then(|| view! {
                            <p class="text-gray-500 dark:text-gray-400">{t("chat-thinking-now")}</p>
                        })}
                        <p class="whitespace-pre-wrap">{text}</p>
                    </div>
                }.into_any()
            }
            Item::Step { label, detail, output, state, .. } => {
                let (mark, tone) = match state {
                    StepState::Running => ("◌", "animate-pulse text-blue-600"),
                    StepState::Ok => ("✓", "text-green-600"),
                    StepState::Failed => ("✕", "text-red-600"),
                    StepState::Stopped => ("–", "text-gray-400"),
                };
                let has_details = !detail.is_empty() || !output.is_empty();
                view! {
                    <div data-item="step" data-state=format!("{state:?}").to_lowercase() class="px-1 text-sm">
                        <div class="flex items-start gap-2">
                            <span aria-hidden="true" class=format!("w-4 shrink-0 text-center {tone}")>{mark}</span>
                            <span class="min-w-0 flex-1 text-gray-700 dark:text-gray-300">{label}</span>
                        </div>
                        {has_details.then(|| view! {
                            <details class="ml-6 text-xs text-gray-500 dark:text-gray-400">
                                <summary class="cursor-pointer select-none">{t("common-details")}</summary>
                                {(!detail.is_empty()).then(|| view! {
                                    <pre class="mt-1 overflow-auto rounded bg-gray-100 p-2 whitespace-pre-wrap dark:bg-gray-800">{detail.clone()}</pre>
                                })}
                                {(!output.is_empty()).then(|| view! {
                                    <pre class="mt-1 max-h-48 overflow-auto rounded bg-gray-100 p-2 whitespace-pre-wrap dark:bg-gray-800">{output.clone()}</pre>
                                })}
                            </details>
                        })}
                    </div>
                }.into_any()
            }
            Item::Build(line) => {
                let text = match line {
                    BuildLine::Checking => t("agent-build-checking")(),
                    BuildLine::Ok => t("agent-build-ok")(),
                    BuildLine::Fixing { attempt, max } => {
                        t_with("agent-build-fixing", [("attempt", attempt.to_string()), ("max", max.to_string())])()
                    }
                };
                view! { <p data-item="build" class="px-1 text-xs text-gray-500 dark:text-gray-400">{text}</p> }.into_any()
            }
            Item::Compacted => view! {
                <p data-item="compacted" class="px-1 text-xs text-gray-400">{t("chat-compacted")}</p>
            }.into_any(),
            Item::Confirm { request_id, label, reason, answered, expired } => {
                view! { <ConfirmCard request_id label reason answered expired /> }.into_any()
            }
            Item::Plan { request_id, plan, answered, expired } => {
                view! { <PlanCard request_id plan answered expired /> }.into_any()
            }
            Item::Question { request_id, question, options, allow_other, answered, expired } => {
                view! { <QuestionCard request_id question options allow_other answered expired /> }.into_any()
            }
            Item::Outcome { outcome, handled } => view! { <OutcomeCard outcome handled /> }.into_any(),
            Item::Doctor(card) => view! { <tools::DoctorCardView card /> }.into_any(),
            Item::Bundle(card) => view! { <tools::BundleCardView card /> }.into_any(),
        })
    }
}

#[component]
fn ConfirmCard(
    request_id: String,
    label: String,
    reason: ak_types::agent::ConfirmReason,
    answered: Option<bool>,
    expired: bool,
) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let answer = move |allow: bool| {
        let id = request_id.clone();
        chat.log.update(|l| l.answer_confirm(&id, allow));
        call(chat.log, move |b| async move {
            b.answer_confirmation(&id, allow).await
        });
    };
    let deny = answer.clone();
    view! {
        <div data-card="confirm" role="alert" class=format!("{CARD} border-amber-300 dark:border-amber-700")>
            <p class="font-medium">{label}</p>
            <p class="mt-1 text-gray-700 dark:text-gray-300">
                {t_with(reason.kind.message_id(), [("detail", reason.detail.clone())])}
            </p>
            {match answered {
                None if expired => view! { <Expired /> }.into_any(),
                None => view! {
                    <div class="mt-3 flex gap-2">
                        <button type="button" data-action="allow" class=PRIMARY on:click=move |_| answer(true)>
                            {t("chat-allow")}
                        </button>
                        <button type="button" data-action="deny" class=SECONDARY on:click=move |_| deny(false)>
                            {t("chat-deny")}
                        </button>
                    </div>
                }.into_any(),
                Some(true) => view! { <p class="mt-2 text-xs text-gray-500">{t("chat-allowed")}</p> }.into_any(),
                Some(false) => view! { <p class="mt-2 text-xs text-gray-500">{t("chat-denied")}</p> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn PlanCard(
    request_id: String,
    plan: ak_types::agent::PlanProposal,
    answered: Option<PlanDecision>,
    expired: bool,
) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let revising = RwSignal::new(false);
    let feedback = RwSignal::new(String::new());
    let decide = move |d: PlanDecision| {
        let id = request_id.clone();
        chat.log.update(|l| l.answer_plan(&id, d.clone()));
        call(
            chat.log,
            move |b| async move { b.answer_plan(&id, d).await },
        );
    };
    let (start, cancel, revise) = (decide.clone(), decide.clone(), decide);
    let steps = plan
        .steps
        .iter()
        .map(|s| view! { <li>{s.clone()}</li> })
        .collect_view();
    view! {
        <div data-card="plan" class=CARD>
            <p class="text-xs font-semibold text-gray-500 uppercase dark:text-gray-400">{t("plan-title")}</p>
            <p class="mt-1 font-medium">{plan.title.clone()}</p>
            <ol class="mt-2 list-decimal space-y-1 pl-5 text-gray-700 dark:text-gray-300">{steps}</ol>
            {plan.estimate.clone().map(|e| view! {
                <p class="mt-2 text-xs text-gray-500 dark:text-gray-400">{t_with("chat-plan-estimate", [("estimate", e)])}</p>
            })}
            {match answered {
                None if expired => view! { <Expired /> }.into_any(),
                None => view! {
                    <Show
                        when=move || revising.get()
                        fallback=move || {
                            let (start, cancel) = (start.clone(), cancel.clone());
                            view! {
                                <div class="mt-3 flex flex-wrap gap-2">
                                    <button type="button" data-action="start" class=PRIMARY on:click=move |_| start(PlanDecision::Start)>
                                        {t("plan-start")}
                                    </button>
                                    <button type="button" data-action="revise" class=SECONDARY on:click=move |_| revising.set(true)>
                                        {t("plan-revise")}
                                    </button>
                                    <button type="button" data-action="cancel-plan" class=SECONDARY on:click=move |_| cancel(PlanDecision::Cancel)>
                                        {t("plan-cancel")}
                                    </button>
                                </div>
                            }
                        }
                    >
                        <textarea
                            rows="2"
                            placeholder=t("chat-plan-feedback")
                            aria-label=t("chat-plan-feedback")
                            class="mt-3 w-full rounded-lg border border-gray-300 bg-white px-2 py-1 text-sm dark:border-gray-700 dark:bg-gray-950"
                            prop:value=move || feedback.get()
                            on:input=move |e| feedback.set(event_target_value(&e))
                        ></textarea>
                        <button
                            type="button"
                            data-action="send-revision"
                            class=format!("mt-2 {PRIMARY}")
                            disabled=move || feedback.with(|f| f.trim().is_empty())
                            on:click={
                                let revise = revise.clone();
                                move |_| revise(PlanDecision::Revise { feedback: feedback.get_untracked().trim().to_owned() })
                            }
                        >
                            {t("chat-send")}
                        </button>
                    </Show>
                }.into_any(),
                Some(PlanDecision::Start) => view! { <p class="mt-2 text-xs text-gray-500">{t("chat-plan-started")}</p> }.into_any(),
                Some(PlanDecision::Revise { .. }) => view! { <p class="mt-2 text-xs text-gray-500">{t("chat-plan-revised")}</p> }.into_any(),
                Some(PlanDecision::Cancel) => view! { <p class="mt-2 text-xs text-gray-500">{t("chat-plan-cancelled")}</p> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn QuestionCard(
    request_id: String,
    question: String,
    options: Vec<String>,
    allow_other: bool,
    answered: Option<String>,
    expired: bool,
) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let other = RwSignal::new(String::new());
    let reply = move |text: String| {
        let id = request_id.clone();
        chat.log.update(|l| l.answer_question(&id, &text));
        call(chat.log, move |b| async move {
            b.answer_question(&id, &text).await
        });
    };
    let body = match answered {
        None if expired => view! { <Expired /> }.into_any(),
        Some(a) => view! { <p class="mt-2 text-xs text-gray-500">{t_with("chat-answered", [("answer", a)])}</p> }.into_any(),
        None => {
            let buttons = options
                .into_iter()
                .map(|o| {
                    let reply = reply.clone();
                    let label = o.clone();
                    view! {
                        <button type="button" data-option="" class=SECONDARY on:click=move |_| reply(o.clone())>
                            {label}
                        </button>
                    }
                })
                .collect_view();
            let send_other = reply.clone();
            view! {
                <div class="mt-3 flex flex-wrap gap-2">{buttons}</div>
                {allow_other.then(|| view! {
                    <div class="mt-2 flex gap-2">
                        <input
                            type="text"
                            placeholder=t("chat-other-answer")
                            aria-label=t("chat-other-answer")
                            class="min-w-0 flex-1 rounded-lg border border-gray-300 bg-white px-2 py-1 text-sm dark:border-gray-700 dark:bg-gray-950"
                            prop:value=move || other.get()
                            on:input=move |e| other.set(event_target_value(&e))
                        />
                        <button
                            type="button"
                            class=PRIMARY
                            disabled=move || other.with(|o| o.trim().is_empty())
                            on:click=move |_| send_other(other.get_untracked().trim().to_owned())
                        >
                            {t("chat-send")}
                        </button>
                    </div>
                })}
            }.into_any()
        }
    };
    view! {
        <div data-card="question" class=CARD>
            <p class="font-medium">{question}</p>
            {body}
        </div>
    }
}

/// A card whose turn ended before the user answered.
#[component]
fn Expired() -> impl IntoView {
    view! { <p data-expired="" class="mt-2 text-xs text-gray-500">{t("chat-expired")}</p> }
}

#[component]
fn OutcomeCard(outcome: TurnOutcome, handled: bool) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let go_on = move |what: Option<StuckChoice>| {
        chat.log.update(|l| {
            l.outcome_handled();
            l.running = true;
        });
        call(chat.log, move |b| async move {
            match what {
                Some(c) => b.stuck_choice(c).await,
                None => b.continue_turn().await,
            }
        });
    };
    let continue_button = move |label: &'static str| {
        (!handled).then(|| view! {
            <div class="mt-3">
                <button type="button" data-action="continue" class=PRIMARY on:click=move |_| go_on(None)>
                    {t(label)}
                </button>
            </div>
        })
    };
    let (kind, body) = match outcome {
        TurnOutcome::Completed => ("completed", ().into_any()),
        TurnOutcome::Stuck { attempts, last_error } => ("stuck", view! {
            <p class="font-medium">{t_with("stuck-title", [("attempts", attempts.to_string())])}</p>
            {(!handled).then(|| view! {
                <div class="mt-3 flex flex-wrap gap-2">
                    <button type="button" data-action="try-another" class=PRIMARY on:click=move |_| go_on(Some(StuckChoice::TryAnotherWay))>
                        {t("stuck-try-another")}
                    </button>
                    <button type="button" data-action="roll-back" class=SECONDARY on:click=move |_| go_on(Some(StuckChoice::RollBack))>
                        {t("stuck-roll-back")}
                    </button>
                </div>
            })}
            <details class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                <summary class="cursor-pointer select-none">{t("stuck-show-details")}</summary>
                <pre class="mt-1 overflow-auto rounded bg-gray-100 p-2 whitespace-pre-wrap dark:bg-gray-800">{last_error}</pre>
            </details>
        }.into_any()),
        TurnOutcome::StepLimit => ("step-limit", view! {
            <p>{t("chat-step-limit")}</p>
            {continue_button("chat-continue")}
        }.into_any()),
        TurnOutcome::MaxTokens => ("max-tokens", view! {
            <p>{t("chat-max-tokens")}</p>
            {continue_button("chat-continue")}
        }.into_any()),
        TurnOutcome::Refused { .. } => ("refused", view! { <p>{t("chat-refused")}</p> }.into_any()),
        TurnOutcome::OverBudget { spent_usd, cap_usd } => ("over-budget", view! {
            <p>{t_with("chat-over-budget", [("spent", money(spent_usd)), ("cap", money(cap_usd))])}</p>
        }.into_any()),
        TurnOutcome::Cancelled => ("cancelled", view! {
            <p class="text-gray-500 dark:text-gray-400">{t("chat-cancelled")}</p>
        }.into_any()),
        TurnOutcome::Failed { message, retryable } => ("failed", view! {
            <p>{t("chat-failed")}</p>
            {(retryable && !handled).then(|| view! {
                <div class="mt-3">
                    <button type="button" data-action="retry" class=PRIMARY on:click=move |_| go_on(None)>
                        {t("common-retry")}
                    </button>
                </div>
            })}
            <details class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                <summary class="cursor-pointer select-none">{t("common-details")}</summary>
                <pre class="mt-1 whitespace-pre-wrap">{message}</pre>
            </details>
        }.into_any()),
    };
    view! { <div data-card="outcome" data-outcome=kind class=CARD>{body}</div> }
}

#[component]
fn Interrupted(request: String) -> impl IntoView {
    let chat = expect_context::<Chat>();
    let choose = move |c: InterruptionChoice| {
        chat.log.update(|l| {
            l.interrupted = None;
            l.running = c == InterruptionChoice::Continue;
        });
        call(
            chat.log,
            move |b| async move { b.resolve_interruption(c).await },
        );
    };
    let back = choose;
    view! {
        <div data-card="interrupted" role="alert" class=format!("{CARD} border-amber-300 dark:border-amber-700")>
            <p>{t_with("chat-interrupted", [("request", request)])}</p>
            <div class="mt-3 flex flex-wrap gap-2">
                <button type="button" data-action="resume" class=PRIMARY on:click=move |_| choose(InterruptionChoice::Continue)>
                    {t("chat-interrupted-continue")}
                </button>
                <button type="button" data-action="go-back" class=SECONDARY on:click=move |_| back(InterruptionChoice::GoBack)>
                    {t("chat-interrupted-go-back")}
                </button>
            </div>
        </div>
    }
}

#[component]
fn Composer() -> impl IntoView {
    let chat = expect_context::<Chat>();
    let text = RwSignal::new(String::new());
    let busy = move || {
        chat.log
            .with(|l| l.running || l.waiting() || l.interrupted.is_some())
    };
    let can_send = move || !busy() && text.with(|t| !t.trim().is_empty());
    let send = move || {
        if !can_send() {
            return;
        }
        send_message(chat, text.get_untracked().trim().to_owned());
        text.set(String::new());
    };
    let stop = move |_| call(chat.log, |b| async move { b.cancel_turn().await });
    view! {
        <div class="border-t border-gray-200 p-3 dark:border-gray-800">
            {move || chat.selection.get().map(|p| view! {
                <div data-chip="selection" class="mb-2 inline-flex max-w-full items-center gap-1 rounded-full bg-blue-50 py-0.5 pr-1 pl-3 text-xs text-blue-900 dark:bg-blue-950 dark:text-blue-100">
                    <span class="truncate">{t_with("chat-selected", [("what", selection_label(&p))])}</span>
                    <button
                        type="button"
                        aria-label=t("chat-selected-remove")
                        title=t("chat-selected-remove")
                        class="rounded-full px-1.5 hover:bg-blue-100 dark:hover:bg-blue-900"
                        on:click=move |_| chat.selection.set(None)
                    >
                        "×"
                    </button>
                </div>
            })}
            <textarea
                rows="3"
                data-input="chat"
                placeholder=t("shell-chat-placeholder")
                aria-label=t("shell-chat-placeholder")
                class="w-full resize-none rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm dark:border-gray-700 dark:bg-gray-900"
                prop:value=move || text.get()
                on:input=move |e| text.set(event_target_value(&e))
                on:keydown=move |e| {
                    if e.key() == "Enter" && !e.shift_key() && !e.is_composing() {
                        e.prevent_default();
                        send();
                    }
                }
            ></textarea>
            <div class="mt-2 flex justify-end">
                <Show
                    when=move || chat.log.with(|l| l.running)
                    fallback=move || view! {
                        <button type="button" data-action="send" class=PRIMARY disabled=move || !can_send() on:click=move |_| send()>
                            {t("chat-send")}
                        </button>
                    }
                >
                    <button type="button" data-action="stop" class=SECONDARY on:click=stop>
                        {t("chat-stop")}
                    </button>
                </Show>
            </div>
        </div>
    }
}
