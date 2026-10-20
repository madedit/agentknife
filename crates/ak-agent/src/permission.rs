//! Permission checks and user confirmation hooks. The default policy is
//! [`crate::policy::ProjectPolicy`].

use ak_types::agent::ConfirmReason;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::tool::ToolContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Ask the user first.
    Ask {
        reason: ConfirmReason,
    },
    /// Refuse; the model is told why and can choose another way.
    Deny {
        reason: ConfirmReason,
    },
}

pub trait PermissionGate: Send + Sync {
    fn check(&self, tool: &str, args: &Value, ctx: &ToolContext) -> Decision;
}

/// Allows every call. Only for tests.
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
    pub reason: ConfirmReason,
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
