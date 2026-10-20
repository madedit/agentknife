//! The user's side of plan-before-big-change.

use ak_types::agent::{PlanDecision, PlanProposal};
use futures_util::future::BoxFuture;

/// Shows a plan and resolves with the user's decision. The UI implements it.
pub trait PlanReviewer: Send + Sync {
    fn review(&self, request_id: String, plan: PlanProposal) -> BoxFuture<'_, PlanDecision>;
}

/// Cancels every plan: the safe default when no UI is attached.
pub struct CancelPlans;

impl PlanReviewer for CancelPlans {
    fn review(&self, _: String, _: PlanProposal) -> BoxFuture<'_, PlanDecision> {
        Box::pin(async { PlanDecision::Cancel })
    }
}
