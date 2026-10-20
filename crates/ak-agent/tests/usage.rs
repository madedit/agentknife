//! Usage, cost and the daily cap (P1-AGENT-006).

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use ak_agent::permission::{ConfirmRequest, Confirmer};
use ak_agent::scripted::{ScriptedProvider, text};
use ak_agent::tool::ToolRegistry;
use ak_agent::usage::{CostBasis, Price, UsageLedger};
use ak_agent::{Agent, AgentConfig};
use ak_types::agent::{AgentEvent, ConfirmKind, TurnOutcome};
use ak_types::chat::{Conversation, Message};
use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

/// Each scripted call uses 10 input + 5 output tokens: at these prices, $0.20.
fn price() -> CostBasis {
    CostBasis::Priced(Price::standard(10_000.0, 20_000.0))
}

struct Answer(bool, Mutex<Vec<ConfirmRequest>>);

impl Confirmer for Answer {
    fn confirm(&self, r: ConfirmRequest) -> BoxFuture<'_, bool> {
        self.1.lock().unwrap().push(r);
        let yes = self.0;
        Box::pin(async move { yes })
    }
}

fn agent(
    p: Arc<ScriptedProvider>,
    cap: Option<f64>,
    basis: CostBasis,
    ledger: Arc<UsageLedger>,
) -> Agent {
    let mut config = AgentConfig::new("m");
    config.cost_basis = basis;
    config.daily_cap_usd = cap;
    Agent::new(p, ToolRegistry::new(), config).with_usage_ledger(ledger)
}

async fn turns(a: &Agent, n: usize) -> (Vec<TurnOutcome>, Vec<AgentEvent>) {
    let mut conv = Conversation::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut outcomes = Vec::new();
    for i in 0..n {
        outcomes.push(
            a.run_turn(
                &mut conv,
                Message::user_text(format!("q{i}")),
                &tx,
                &CancellationToken::new(),
            )
            .await,
        );
    }
    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (outcomes, events)
}

fn last_totals(events: &[AgentEvent]) -> (f64, f64) {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            AgentEvent::UsageTotals { session, today } => {
                Some((session.cost_usd.unwrap(), today.cost_usd.unwrap()))
            }
            _ => None,
        })
        .unwrap()
}

#[tokio::test]
async fn warns_at_80_percent_and_stops_at_the_cap_when_declined() {
    let p = Arc::new(ScriptedProvider::new((0..5).map(|_| text("ok")).collect()));
    let a = agent(
        p.clone(),
        Some(0.5),
        price(),
        Arc::new(UsageLedger::in_memory()),
    );
    let (outcomes, events) = turns(&a, 4).await;

    assert_eq!(
        &outcomes[..3],
        &[
            TurnOutcome::Completed,
            TurnOutcome::Completed,
            TurnOutcome::Completed
        ]
    );
    assert!(
        matches!(outcomes[3], TurnOutcome::OverBudget { cap_usd, .. } if (cap_usd - 0.5).abs() < 1e-9)
    );
    assert_eq!(
        p.requests().len(),
        3,
        "no model call past the cap without consent"
    );
    let warnings: Vec<f64> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::BudgetWarning { spent_usd, .. } => Some(*spent_usd),
            _ => None,
        })
        .collect();
    assert_eq!(warnings.len(), 1, "warned once");
    assert!((warnings[0] - 0.4).abs() < 1e-9);
    let (session, today) = last_totals(&events);
    assert!((session - 0.6).abs() < 1e-9 && (today - 0.6).abs() < 1e-9);
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::NeedsConfirmation { reason, .. } if reason.kind == ConfirmKind::DailyCap && reason.detail == "US$0.60 / US$0.50")));
}

#[tokio::test]
async fn approving_continues_and_is_not_asked_again_today() {
    let p = Arc::new(ScriptedProvider::new((0..6).map(|_| text("ok")).collect()));
    let answer = Arc::new(Answer(true, Mutex::new(Vec::new())));
    let a = agent(
        p.clone(),
        Some(0.5),
        price(),
        Arc::new(UsageLedger::in_memory()),
    )
    .with_confirmer(answer.clone());
    let (outcomes, _) = turns(&a, 6).await;
    assert!(outcomes.iter().all(|o| *o == TurnOutcome::Completed));
    assert_eq!(answer.1.lock().unwrap().len(), 1);
    assert_eq!(p.requests().len(), 6);
}

#[tokio::test]
async fn today_is_shared_across_projects_but_sessions_are_not() {
    let ledger = Arc::new(UsageLedger::in_memory());
    let a1 = agent(
        Arc::new(ScriptedProvider::new(vec![text("a")])),
        None,
        price(),
        ledger.clone(),
    );
    let a2 = agent(
        Arc::new(ScriptedProvider::new(vec![text("b")])),
        None,
        price(),
        ledger.clone(),
    );
    turns(&a1, 1).await;
    let (_, events) = turns(&a2, 1).await;
    let (session, today) = last_totals(&events);
    assert!((session - 0.2).abs() < 1e-9, "this project's session only");
    assert!(
        (today - 0.4).abs() < 1e-9,
        "today includes the other project"
    );
}

#[tokio::test]
async fn free_and_unknown_costs() {
    let a = agent(
        Arc::new(ScriptedProvider::new(vec![text("a")])),
        Some(0.01),
        CostBasis::Free,
        Arc::new(UsageLedger::in_memory()),
    );
    let (outcomes, events) = turns(&a, 1).await;
    assert_eq!(outcomes[0], TurnOutcome::Completed);
    let free = events.iter().find_map(|e| match e {
        AgentEvent::UsageTotals { session, .. } => Some(*session),
        _ => None,
    });
    let free = free.unwrap();
    assert!(free.free && free.cost_usd == Some(0.0));
    assert_eq!(free.tokens.input_tokens, 10);

    let a = agent(
        Arc::new(ScriptedProvider::new(vec![text("a"), text("b")])),
        Some(0.01),
        CostBasis::Unknown,
        Arc::new(UsageLedger::in_memory()),
    );
    let (outcomes, events) = turns(&a, 2).await;
    assert!(
        outcomes.iter().all(|o| *o == TurnOutcome::Completed),
        "unknown prices never trip the cap"
    );
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::UsageTotals { session, .. } if session.cost_usd.is_none() && session.cost_is_lower_bound)));
}
