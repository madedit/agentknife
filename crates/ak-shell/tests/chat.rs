//! The chat pane for a given conversation state, rendered to HTML natively.
//! The streaming and the buttons are checked in a browser with the demo.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]
// Leptos views nest deeply in types (the chat pane).
#![recursion_limit = "256"]

use ak_shell::backend::Backend;
use ak_shell::chat::log::{ChatLog, Item};
use ak_shell::chat::{Chat, ChatPane, selection_label, usage_text};
use ak_shell::i18n;
use ak_types::agent::{
    AgentEvent, ConfirmKind, ConfirmReason, PlanDecision, PlanProposal, TokenUsage, TurnOutcome,
    UsageSummary,
};
use ak_types::i18n::Locale;
use ak_types::preview::{Picked, Rect, Size};
use leptos::prelude::*;

fn render(locale: Locale, log: ChatLog, selection: Option<Picked>) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::demo());
        let chat = Chat::new(log);
        chat.selection.set(selection);
        provide_context(chat);
        view! { <ChatPane /> }.into_view().to_html()
    })
}

fn picked() -> Picked {
    Picked {
        ak_id: Some("greet-button".into()),
        exact: true,
        path: "main > button".into(),
        tag: "button".into(),
        text: Some("Greet".into()),
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
        viewport: Size {
            width: 100.0,
            height: 100.0,
        },
        canvas: None,
    }
}

fn usage(cost: Option<f64>, free: bool) -> UsageSummary {
    UsageSummary {
        tokens: TokenUsage::default(),
        cost_usd: cost,
        cost_is_lower_bound: false,
        free,
    }
}

/// A turn with every kind of item, waiting on a confirmation.
fn busy_log() -> ChatLog {
    let mut log = ChatLog::default();
    log.user_sent("make it blue", Some(selection_label(&picked())));
    for e in [
        AgentEvent::ThinkingDelta {
            text: "They want blue.".into(),
        },
        AgentEvent::TextDelta {
            text: "Sure.".into(),
        },
        AgentEvent::StepStarted {
            step_id: "s1".into(),
            tool: "run_command".into(),
            label: "Checking the build".into(),
            detail: "cargo check".into(),
        },
        AgentEvent::StepFinished {
            step_id: "s1".into(),
            ok: false,
            output: "error[E0308]: mismatched types".into(),
        },
        AgentEvent::BuildChecking,
        AgentEvent::PlanProposed {
            request_id: "p1".into(),
            plan: PlanProposal {
                title: "Save the list".into(),
                steps: vec!["Store".into(), "Load".into(), "Save".into()],
                estimate: Some("about 3 minutes".into()),
            },
        },
        AgentEvent::NeedsConfirmation {
            request_id: "c1".into(),
            tool: "run_command".into(),
            label: "Installing a library".into(),
            reason: ConfirmReason {
                kind: ConfirmKind::Download,
                detail: "cargo add serde_json".into(),
            },
        },
        AgentEvent::UsageTotals {
            session: usage(Some(0.0123), false),
            today: usage(Some(0.42), false),
        },
    ] {
        log.apply(e);
    }
    log
}

#[test]
fn a_running_turn_shows_words_steps_and_cards() {
    let html = render(Locale::En, busy_log(), Some(picked()));
    // `AK_SHELL_DUMP=<file>`: write the pane out, to look at it with the real CSS.
    if let Some(path) = std::env::var_os("AK_SHELL_DUMP") {
        let mut stuck = busy_log();
        stuck.answer_plan("p1", PlanDecision::Start);
        stuck.apply(AgentEvent::TurnFinished {
            outcome: TurnOutcome::Stuck {
                attempts: 3,
                last_error: "error[E0308]: mismatched types".into(),
            },
        });
        let zh = render(Locale::ZhTw, busy_log(), Some(picked()));
        let done = render(Locale::ZhTw, stuck, None);
        std::fs::write(path, format!("{zh}<hr>{done}")).unwrap();
    }
    for item in ["user", "assistant", "step", "build"] {
        assert!(html.contains(&format!(r#"data-item="{item}""#)), "{item}");
    }
    assert!(html.contains("Selected: &lt;button&gt; “Greet”"), "{html}");
    // Thinking is there but collapsed.
    assert!(html.contains("data-thinking") && !html.contains("<details open"));
    assert!(html.contains("cargo check") && html.contains("error[E0308]"));
    assert!(html.contains(r#"data-state="failed""#));
    // Plan and confirmation cards with their buttons.
    assert!(html.contains(r#"data-card="plan""#) && html.contains(r#"data-action="start""#));
    assert!(html.contains("Estimate: about 3 minutes"));
    assert!(html.contains(r#"data-card="confirm""#) && html.contains(r#"data-action="allow""#));
    assert!(html.contains("cargo add serde_json"));
    // Running: "Stop" instead of "Send", and the selection chip waits on the input.
    assert!(html.contains(r#"data-action="stop""#) && !html.contains(r#"data-action="send""#));
    assert!(html.contains(r#"data-chip="selection""#));
    assert!(
        html.contains("This session US$0.01 · today US$0.42"),
        "{html}"
    );
}

#[test]
fn answered_cards_keep_the_choice_without_buttons() {
    let mut log = busy_log();
    log.answer_plan("p1", PlanDecision::Start);
    log.answer_confirm("c1", false);
    log.apply(AgentEvent::TurnFinished {
        outcome: TurnOutcome::Completed,
    });
    let html = render(Locale::En, log, None);
    assert!(!html.contains(r#"data-action="start""#) && !html.contains(r#"data-action="allow""#));
    assert!(
        html.contains("You started this plan.") && html.contains("You did not allow this step.")
    );
    assert!(html.contains(r#"data-action="send""#) && !html.contains(r#"data-chip="#));

    // Cards the ended turn left unanswered say so instead of offering buttons.
    let mut left = busy_log();
    left.apply(AgentEvent::TurnFinished {
        outcome: TurnOutcome::Cancelled,
    });
    let html = render(Locale::En, left, None);
    assert_eq!(html.matches("data-expired").count(), 2, "{html}");
    assert!(!html.contains(r#"data-action="allow""#) && !html.contains("Checking that the app"));
}

#[test]
fn outcomes_offer_the_ways_on() {
    let ended = |outcome: TurnOutcome| {
        let mut log = ChatLog::default();
        log.user_sent("go", None);
        log.apply(AgentEvent::TurnFinished { outcome });
        log
    };
    let stuck = render(
        Locale::En,
        ended(TurnOutcome::Stuck {
            attempts: 3,
            last_error: "error[E0599]".into(),
        }),
        None,
    );
    assert!(stuck.contains(r#"data-outcome="stuck""#));
    assert!(
        stuck.contains(r#"data-action="try-another""#)
            && stuck.contains(r#"data-action="roll-back""#)
    );
    assert!(stuck.contains("Show technical details") && stuck.contains("error[E0599]"));
    assert!(stuck.contains("3 times"));

    let limit = render(Locale::En, ended(TurnOutcome::StepLimit), None);
    assert!(
        limit.contains(r#"data-outcome="step-limit""#)
            && limit.contains(r#"data-action="continue""#)
    );

    let failed = render(
        Locale::En,
        ended(TurnOutcome::Failed {
            message: "network error".into(),
            retryable: true,
        }),
        None,
    );
    assert!(failed.contains(r#"data-action="retry""#) && failed.contains("network error"));

    // After the user picked a way on, the buttons go.
    let mut handled = ended(TurnOutcome::StepLimit);
    handled.outcome_handled();
    assert!(!render(Locale::En, handled, None).contains(r#"data-action="continue""#));
}

#[test]
fn an_interrupted_turn_asks_to_continue_or_go_back() {
    let log = ChatLog::from_history(&[], Some("Add a delete button".into()));
    let html = render(Locale::ZhTw, log, None);
    assert!(html.contains(r#"data-card="interrupted""#));
    assert!(
        html.contains("上次做到一半中斷了（正在：「Add a delete button」）"),
        "{html}"
    );
    assert!(html.contains(r#"data-action="resume""#) && html.contains(r#"data-action="go-back""#));
    // No sending until the user decides.
    assert!(html.contains(r#"data-action="send""#) && html.contains("disabled"));
    assert!(!html.contains(">chat-"), "raw fluent id in {html}");
}

#[test]
fn the_chip_names_the_element_briefly() {
    let mut p = picked();
    assert_eq!(selection_label(&p), "<button> “Greet”");
    p.tag = "main".into();
    p.ak_id = Some("home-screen".into());
    p.text = Some("Bundle Check Say hello Your name Greet and more".into());
    assert_eq!(selection_label(&p), "<main> home-screen");
    p.ak_id = None;
    assert_eq!(
        selection_label(&p),
        "<main> “Bundle Check Say hello Your na…”"
    );
    p.text = None;
    assert_eq!(selection_label(&p), "<main>");
}

#[test]
fn usage_reads_free_unknown_or_cost() {
    assert_eq!(
        usage_text(&usage(None, true), &usage(None, true)).0,
        "usage-free"
    );
    assert_eq!(
        usage_text(&usage(None, false), &usage(Some(1.0), false)).0,
        "usage-unknown"
    );
    let (id, args) = usage_text(&usage(Some(0.5), false), &usage(Some(2.0), false));
    assert_eq!(id, "chat-usage");
    assert_eq!(
        args,
        [("session", "0.50".to_owned()), ("today", "2.00".to_owned())]
    );
    assert!(matches!(busy_log().items[0], Item::User { .. }));
}
