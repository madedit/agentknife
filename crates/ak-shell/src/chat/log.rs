//! The chat as the user sees it, built from the agent's events (and from the
//! saved conversation when a project opens). No view code: tested directly.

use ak_types::agent::{
    AgentEvent, ConfirmReason, PlanDecision, PlanProposal, TurnOutcome, UsageSummary,
};
use ak_types::chat::{ContentPart, Entry, Message};
use ak_types::doctor::DoctorProgress;
use ak_types::ipc::{BundleFailure, BundleProgress, BundleResult, DoctorCheck};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Running,
    Ok,
    Failed,
    /// The turn ended (cancelled, failed) while it ran.
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildLine {
    Checking,
    Ok,
    Fixing { attempt: u32, max: u32 },
}

/// The tools check, shown in the chat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DoctorCard {
    pub check: Option<DoctorCheck>,
    /// The install step running now.
    pub progress: Option<DoctorProgress>,
    /// Checking or installing.
    pub busy: bool,
    pub error: Option<String>,
}

/// Packaging the app, shown in the chat.
#[derive(Debug, Clone, PartialEq)]
pub enum BundleCard {
    Running {
        /// Fluent id of the stage (`bundle-building`).
        stage: Option<String>,
        /// The last lines of build output.
        lines: Vec<String>,
    },
    Done(BundleResult),
    Failed(BundleFailure),
}

/// Lines of build output kept on the bundle card.
const BUNDLE_LINES: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    User {
        text: String,
        /// What the user pointed at, as shown on the chip.
        selection: Option<String>,
    },
    Assistant {
        text: String,
        /// Shown collapsed.
        thinking: String,
    },
    Step {
        id: String,
        label: String,
        detail: String,
        output: String,
        state: StepState,
    },
    Build(BuildLine),
    /// Older messages were summarized for the AI.
    Compacted,
    Confirm {
        request_id: String,
        label: String,
        reason: ConfirmReason,
        answered: Option<bool>,
        /// The turn ended before an answer: nothing waits for it any more.
        expired: bool,
    },
    Plan {
        request_id: String,
        plan: PlanProposal,
        answered: Option<PlanDecision>,
        expired: bool,
    },
    Question {
        request_id: String,
        question: String,
        options: Vec<String>,
        allow_other: bool,
        answered: Option<String>,
        expired: bool,
    },
    Doctor(DoctorCard),
    Bundle(BundleCard),
    /// How a turn ended, when it did not simply complete.
    Outcome {
        outcome: TurnOutcome,
        /// The user already picked one of the offered ways on.
        handled: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatLog {
    pub items: Vec<Item>,
    /// A turn is running (the input shows "Stop").
    pub running: bool,
    /// Stop was pressed and the turn has not ended yet.
    pub stopping: bool,
    /// (this session, today)
    pub usage: Option<(UsageSummary, UsageSummary)>,
    /// (spent, cap) in US dollars, once today's 80% is reached.
    pub budget_warning: Option<(f64, f64)>,
    /// The request of an interrupted turn, until the user decides.
    pub interrupted: Option<String>,
    /// A turn just started: its words go in a new bubble, even right after the
    /// previous turn's (continuing, resuming).
    pub new_bubble: bool,
}

fn texts(content: &[ContentPart]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            ContentPart::Text { text } => Some(text.as_str()),
            ContentPart::Image { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl ChatLog {
    /// The saved conversation: the user's and the AI's words, and finished steps
    /// (labels from the stored arguments). AgentKnife's own notes are left out.
    pub fn from_history(history: &[Entry], interrupted: Option<String>) -> ChatLog {
        let mut log = ChatLog {
            interrupted,
            ..ChatLog::default()
        };
        for entry in history {
            let Entry::Message(m) = entry else { continue };
            match m {
                Message::User { content } => log.items.push(Item::User {
                    text: texts(content),
                    selection: None,
                }),
                Message::Assistant {
                    text, tool_calls, ..
                } => {
                    if !text.trim().is_empty() {
                        log.items.push(Item::Assistant {
                            text: text.clone(),
                            thinking: String::new(),
                        });
                    }
                    for call in tool_calls {
                        let label = call
                            .arguments
                            .get("label")
                            .and_then(|l| l.as_str())
                            .unwrap_or(&call.name)
                            .to_owned();
                        log.items.push(Item::Step {
                            id: call.id.clone(),
                            label,
                            detail: String::new(),
                            output: String::new(),
                            state: StepState::Ok,
                        });
                    }
                }
                Message::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => {
                    if let Some(Item::Step { output, state, .. }) = log.step(call_id) {
                        *output = texts(content);
                        if *is_error {
                            *state = StepState::Failed;
                        }
                    }
                }
            }
        }
        log
    }

    /// The user sent a message (shown at once, before the turn starts).
    pub fn user_sent(&mut self, text: &str, selection: Option<String>) {
        self.items.push(Item::User {
            text: text.to_owned(),
            selection,
        });
        self.running = true;
        self.interrupted = None;
        for item in &mut self.items {
            if let Item::Outcome { handled, .. } = item {
                *handled = true;
            }
        }
    }

    fn step(&mut self, step_id: &str) -> Option<&mut Item> {
        self.items
            .iter_mut()
            .rev()
            .find(|i| matches!(i, Item::Step { id, .. } if id == step_id))
    }

    /// Adds to the assistant bubble being written, or starts one after other items.
    fn write(&mut self, delta: &str, is_thinking: bool) {
        if !std::mem::take(&mut self.new_bubble)
            && let Some(Item::Assistant { text, thinking }) = self.items.last_mut()
        {
            if is_thinking { thinking } else { text }.push_str(delta);
            return;
        }
        let (text, thinking) = if is_thinking {
            (String::new(), delta.to_owned())
        } else {
            (delta.to_owned(), String::new())
        };
        self.items.push(Item::Assistant { text, thinking });
    }

    pub fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStarted { .. } => {
                self.running = true;
                self.stopping = false;
                self.new_bubble = true;
            }
            AgentEvent::TextDelta { text } => self.write(&text, false),
            AgentEvent::ThinkingDelta { text } => self.write(&text, true),
            AgentEvent::ThinkingEnd
            | AgentEvent::Usage { .. }
            | AgentEvent::MessageAppended { .. } => {}
            AgentEvent::StepStarted {
                step_id,
                label,
                detail,
                ..
            } => self.items.push(Item::Step {
                id: step_id,
                label,
                detail,
                output: String::new(),
                state: StepState::Running,
            }),
            AgentEvent::StepFinished {
                step_id,
                ok,
                output: out,
            } => {
                if let Some(Item::Step { state, output, .. }) = self.step(&step_id) {
                    *state = if ok { StepState::Ok } else { StepState::Failed };
                    *output = out;
                }
            }
            AgentEvent::NeedsConfirmation {
                request_id,
                label,
                reason,
                ..
            } => self.items.push(Item::Confirm {
                request_id,
                label,
                reason,
                answered: None,
                expired: false,
            }),
            AgentEvent::PlanProposed { request_id, plan } => self.items.push(Item::Plan {
                request_id,
                plan,
                answered: None,
                expired: false,
            }),
            AgentEvent::QuestionAsked {
                request_id,
                question,
                options,
                allow_other,
            } => self.items.push(Item::Question {
                request_id,
                question,
                options,
                allow_other,
                answered: None,
                expired: false,
            }),
            AgentEvent::UsageTotals { session, today } => self.usage = Some((session, today)),
            AgentEvent::BudgetWarning { spent_usd, cap_usd } => {
                self.budget_warning = Some((spent_usd, cap_usd));
            }
            AgentEvent::ContextCompacted => self.items.push(Item::Compacted),
            AgentEvent::BuildChecking => self.items.push(Item::Build(BuildLine::Checking)),
            AgentEvent::BuildChecked { ok, attempt, max } => {
                let line = if ok {
                    BuildLine::Ok
                } else {
                    BuildLine::Fixing { attempt, max }
                };
                match self.items.last_mut() {
                    Some(Item::Build(b @ BuildLine::Checking)) => *b = line,
                    _ => self.items.push(Item::Build(line)),
                }
            }
            AgentEvent::TurnFinished { outcome } => {
                self.running = false;
                self.stopping = false;
                for item in &mut self.items {
                    match item {
                        Item::Step { state, .. } if *state == StepState::Running => {
                            *state = StepState::Stopped;
                        }
                        Item::Confirm {
                            answered: None,
                            expired,
                            ..
                        }
                        | Item::Plan {
                            answered: None,
                            expired,
                            ..
                        }
                        | Item::Question {
                            answered: None,
                            expired,
                            ..
                        } => *expired = true,
                        _ => {}
                    }
                }
                // A build check the turn did not wait for says nothing.
                self.items
                    .retain(|i| !matches!(i, Item::Build(BuildLine::Checking)));
                if outcome != TurnOutcome::Completed {
                    self.items.push(Item::Outcome {
                        outcome,
                        handled: false,
                    });
                }
            }
        }
    }

    /// Marks a card answered (its buttons go away; the choice stays visible).
    pub fn answer_confirm(&mut self, id: &str, allow: bool) {
        for item in &mut self.items {
            if let Item::Confirm {
                request_id,
                answered,
                ..
            } = item
                && request_id == id
            {
                *answered = Some(allow);
            }
        }
    }

    pub fn answer_plan(&mut self, id: &str, decision: PlanDecision) {
        for item in &mut self.items {
            if let Item::Plan {
                request_id,
                answered,
                ..
            } = item
                && request_id == id
            {
                *answered = Some(decision.clone());
            }
        }
    }

    pub fn answer_question(&mut self, id: &str, answer: &str) {
        for item in &mut self.items {
            if let Item::Question {
                request_id,
                answered,
                ..
            } = item
                && request_id == id
            {
                *answered = Some(answer.to_owned());
            }
        }
    }

    /// The user picked a way on after an outcome (continue, another way…).
    pub fn outcome_handled(&mut self) {
        for item in &mut self.items {
            if let Item::Outcome { handled, .. } = item {
                *handled = true;
            }
        }
    }

    /// A new tools check at the end of the chat.
    pub fn new_doctor(&mut self) -> &mut DoctorCard {
        self.items.push(Item::Doctor(DoctorCard {
            busy: true,
            ..DoctorCard::default()
        }));
        self.doctor()
    }

    /// The latest tools check (a new one if there is none).
    pub fn doctor(&mut self) -> &mut DoctorCard {
        let found = self
            .items
            .iter()
            .rposition(|i| matches!(i, Item::Doctor(_)));
        let index = match found {
            Some(i) => i,
            None => {
                self.items.push(Item::Doctor(DoctorCard::default()));
                self.items.len() - 1
            }
        };
        match &mut self.items[index] {
            Item::Doctor(card) => card,
            // INVARIANT: `index` points at a Doctor item (found or just pushed).
            _ => unreachable!("a doctor card"),
        }
    }

    pub fn doctor_progress(&mut self, p: DoctorProgress) {
        self.doctor().progress = Some(p);
    }

    /// A check (or install) another AI tool ran: the card shows its result.
    pub fn doctor_checked(&mut self, check: DoctorCheck) {
        let card = self.doctor();
        card.check = Some(check);
        card.progress = None;
        card.busy = false;
        card.error = None;
    }

    pub fn bundle_started(&mut self) {
        self.items.push(Item::Bundle(BundleCard::Running {
            stage: None,
            lines: Vec::new(),
        }));
    }

    pub fn bundle_progress(&mut self, p: BundleProgress) {
        let Some(Item::Bundle(BundleCard::Running { stage, lines })) = self
            .items
            .iter_mut()
            .rev()
            .find(|i| matches!(i, Item::Bundle(_)))
        else {
            return;
        };
        match p {
            BundleProgress::Stage { message } => *stage = Some(message),
            BundleProgress::Output { line } => {
                if lines.len() == BUNDLE_LINES {
                    lines.remove(0);
                }
                lines.push(line);
            }
        }
    }

    pub fn bundle_finished(&mut self, result: Result<BundleResult, BundleFailure>) {
        let card = match result {
            Ok(r) => BundleCard::Done(r),
            Err(f) => BundleCard::Failed(f),
        };
        match self
            .items
            .iter_mut()
            .rev()
            .find(|i| matches!(i, Item::Bundle(_)))
        {
            Some(item) => *item = Item::Bundle(card),
            None => self.items.push(Item::Bundle(card)),
        }
    }

    /// Packaging is running.
    pub fn bundling(&self) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i, Item::Bundle(BundleCard::Running { .. })))
    }

    /// The agent waits for the user (a card without an answer).
    pub fn waiting(&self) -> bool {
        self.items.iter().any(|i| {
            matches!(
                i,
                Item::Confirm {
                    answered: None,
                    expired: false,
                    ..
                } | Item::Plan {
                    answered: None,
                    expired: false,
                    ..
                } | Item::Question {
                    answered: None,
                    expired: false,
                    ..
                }
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use ak_types::agent::ConfirmKind;
    use ak_types::chat::ToolCall;

    use super::*;

    fn step_started(id: &str, label: &str) -> AgentEvent {
        AgentEvent::StepStarted {
            step_id: id.into(),
            tool: "read_file".into(),
            label: label.into(),
            detail: "src/main.rs".into(),
        }
    }

    #[test]
    fn stopping_lasts_until_the_turn_ends() {
        let mut log = ChatLog::default();
        log.apply(AgentEvent::TurnStarted {
            turn_id: "t".into(),
        });
        log.stopping = true;
        log.apply(AgentEvent::ThinkingDelta { text: "Hm".into() });
        assert!(log.running && log.stopping);
        log.apply(AgentEvent::TurnFinished {
            outcome: TurnOutcome::Cancelled,
        });
        assert!(!log.running && !log.stopping);
    }

    #[test]
    fn a_turn_streams_into_bubbles_steps_and_build_lines() {
        let mut log = ChatLog::default();
        log.user_sent("make it blue", Some("Greet button".into()));
        assert!(log.running);
        for e in [
            AgentEvent::TurnStarted {
                turn_id: "t1".into(),
            },
            AgentEvent::ThinkingDelta {
                text: "The user wants".into(),
            },
            AgentEvent::ThinkingDelta {
                text: " blue.".into(),
            },
            AgentEvent::ThinkingEnd,
            AgentEvent::TextDelta {
                text: "Sure".into(),
            },
            AgentEvent::TextDelta { text: "!".into() },
            step_started("s1", "Reading the screen"),
            AgentEvent::StepFinished {
                step_id: "s1".into(),
                ok: true,
                output: "fn main() {}".into(),
            },
            AgentEvent::TextDelta {
                text: "Done.".into(),
            },
            AgentEvent::BuildChecking,
            AgentEvent::BuildChecked {
                ok: false,
                attempt: 1,
                max: 3,
            },
            AgentEvent::BuildChecking,
            AgentEvent::BuildChecked {
                ok: true,
                attempt: 0,
                max: 3,
            },
            AgentEvent::TurnFinished {
                outcome: TurnOutcome::Completed,
            },
        ] {
            log.apply(e);
        }
        assert!(!log.running);
        assert_eq!(
            log.items,
            vec![
                Item::User {
                    text: "make it blue".into(),
                    selection: Some("Greet button".into()),
                },
                Item::Assistant {
                    text: "Sure!".into(),
                    thinking: "The user wants blue.".into(),
                },
                Item::Step {
                    id: "s1".into(),
                    label: "Reading the screen".into(),
                    detail: "src/main.rs".into(),
                    output: "fn main() {}".into(),
                    state: StepState::Ok,
                },
                Item::Assistant {
                    text: "Done.".into(),
                    thinking: String::new(),
                },
                Item::Build(BuildLine::Fixing { attempt: 1, max: 3 }),
                Item::Build(BuildLine::Ok),
            ]
        );
    }

    #[test]
    fn tools_checks_and_bundles_update_their_latest_card() {
        let mut log = ChatLog::default();
        log.new_doctor();
        log.doctor_progress(DoctorProgress {
            item: "trunk".into(),
            step: 1,
            total: 2,
            phase: ak_types::doctor::InstallPhase::Verify,
        });
        assert!(matches!(
            &log.items[0],
            Item::Doctor(DoctorCard {
                busy: true,
                progress: Some(_),
                ..
            })
        ));
        log.doctor().busy = false;
        assert_eq!(log.items.len(), 1, "updates, not a second card");
        // Another AI tool's check replaces what the card shows.
        log.doctor_checked(DoctorCheck {
            report: ak_types::doctor::DoctorReport { items: Vec::new() },
            guides: Vec::new(),
        });
        assert!(matches!(
            &log.items[0],
            Item::Doctor(DoctorCard {
                check: Some(_),
                progress: None,
                busy: false,
                ..
            })
        ));
        assert_eq!(log.items.len(), 1);

        log.bundle_started();
        assert!(log.bundling());
        log.bundle_progress(BundleProgress::Stage {
            message: "bundle-building".into(),
        });
        for n in 0..40 {
            log.bundle_progress(BundleProgress::Output {
                line: format!("line {n}"),
            });
        }
        match &log.items[1] {
            Item::Bundle(BundleCard::Running { stage, lines }) => {
                assert_eq!(stage.as_deref(), Some("bundle-building"));
                assert_eq!(lines.len(), BUNDLE_LINES);
                assert_eq!(lines.last().map(String::as_str), Some("line 39"));
            }
            other => panic!("{other:?}"),
        }
        log.bundle_finished(Err(BundleFailure {
            kind: "build-failed".into(),
            detail: "error".into(),
        }));
        assert!(!log.bundling());
        assert!(matches!(log.items[1], Item::Bundle(BundleCard::Failed(_))));
    }

    #[test]
    fn a_resumed_turn_starts_its_own_bubble() {
        let history = vec![Entry::Message(Message::Assistant {
            thinking: Vec::new(),
            text: "Done before.".into(),
            tool_calls: Vec::new(),
            replay: None,
        })];
        let mut log = ChatLog::from_history(&history, Some("x".into()));
        log.apply(AgentEvent::TurnStarted {
            turn_id: "t2".into(),
        });
        log.apply(AgentEvent::ThinkingDelta { text: "Hm".into() });
        log.apply(AgentEvent::TextDelta {
            text: "Again".into(),
        });
        assert_eq!(log.items.len(), 2);
        assert!(
            matches!(&log.items[1], Item::Assistant { text, thinking } if text == "Again" && thinking == "Hm")
        );
    }

    #[test]
    fn cards_wait_for_answers_and_outcomes_are_kept() {
        let mut log = ChatLog::default();
        log.apply(AgentEvent::NeedsConfirmation {
            request_id: "r1".into(),
            tool: "run_command".into(),
            label: "Installing a library".into(),
            reason: ConfirmReason {
                kind: ConfirmKind::Download,
                detail: "cargo add serde".into(),
            },
        });
        assert!(log.waiting());
        log.answer_confirm("r1", true);
        assert!(!log.waiting());
        assert!(matches!(
            log.items[0],
            Item::Confirm {
                answered: Some(true),
                ..
            }
        ));

        log.apply(AgentEvent::PlanProposed {
            request_id: "p1".into(),
            plan: PlanProposal {
                title: "Add saving".into(),
                steps: vec!["a".into(), "b".into(), "c".into()],
                estimate: None,
            },
        });
        log.answer_plan("p1", PlanDecision::Start);
        log.apply(step_started("s9", "Writing"));
        log.apply(AgentEvent::TurnFinished {
            outcome: TurnOutcome::Stuck {
                attempts: 3,
                last_error: "error[E0308]".into(),
            },
        });
        assert!(matches!(
            log.items[2],
            Item::Step {
                state: StepState::Stopped,
                ..
            }
        ));
        // A card left unanswered when the turn ended no longer waits.
        let mut ended = ChatLog::default();
        ended.apply(AgentEvent::QuestionAsked {
            request_id: "q1".into(),
            question: "Blue?".into(),
            options: vec![],
            allow_other: true,
        });
        ended.apply(AgentEvent::BuildChecking);
        assert!(ended.waiting());
        ended.apply(AgentEvent::TurnFinished {
            outcome: TurnOutcome::Cancelled,
        });
        assert!(!ended.waiting());
        assert!(matches!(
            ended.items[0],
            Item::Question { expired: true, .. }
        ));
        assert!(!ended.items.iter().any(|i| matches!(i, Item::Build(_))));
        assert!(matches!(
            log.items.last(),
            Some(Item::Outcome { handled: false, .. })
        ));
        log.user_sent("try again", None);
        assert!(
            log.items
                .iter()
                .all(|i| !matches!(i, Item::Outcome { handled: false, .. }))
        );
    }

    #[test]
    fn history_shows_words_and_steps_but_not_notes() {
        let history = vec![
            Entry::Context {
                text: "[AgentKnife] project notes".into(),
            },
            Entry::Message(Message::user_text("add a list")),
            Entry::Message(Message::Assistant {
                thinking: Vec::new(),
                text: "On it.".into(),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "write_file".into(),
                    arguments: serde_json::json!({ "label": "Writing the list", "path": "src/main.rs" }),
                    invalid_arguments: None,
                }],
                replay: None,
            }),
            Entry::Message(Message::ToolResult {
                call_id: "c1".into(),
                content: vec![ContentPart::Text {
                    text: "no space left".into(),
                }],
                is_error: true,
            }),
        ];
        let log = ChatLog::from_history(&history, Some("add a list".into()));
        assert_eq!(log.items.len(), 3);
        assert!(
            matches!(&log.items[2], Item::Step { label, state: StepState::Failed, output, .. }
            if label == "Writing the list" && output == "no space left")
        );
        assert_eq!(log.interrupted.as_deref(), Some("add a list"));
        assert!(!log.running);
    }
}
