//! Hooks for permission checks and user confirmation. The policy itself (what is
//! automatic, what needs asking) is P1-AGENT-003.

use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::tool::ToolContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Ask the user; `reason` is plain language ("This downloads 300 MB").
    Ask {
        reason: String,
    },
    Deny {
        reason: String,
    },
}

pub trait PermissionGate: Send + Sync {
    fn check(&self, tool: &str, args: &Value, ctx: &ToolContext) -> Decision;
}

/// Allows every call. Only for tests and until P1-AGENT-003.
pub struct AllowAll;

impl PermissionGate for AllowAll {
    fn check(&self, _: &str, _: &Value, _: &ToolContext) -> Decision {
        Decision::Allow
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub request_id: String,
    pub tool: String,
    pub label: String,
    pub reason: String,
}

/// Asks the user and resolves with their answer. The UI implements it.
pub trait Confirmer: Send + Sync {
    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'_, bool>;
}

/// Declines everything: the safe default when no UI is attached.
pub struct DeclineAll;

impl Confirmer for DeclineAll {
    fn confirm(&self, _: ConfirmRequest) -> BoxFuture<'_, bool> {
        Box::pin(async { false })
    }
}
