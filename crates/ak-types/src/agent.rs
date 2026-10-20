//! Events the agent loop emits for the UI (ak-app forwards them to ak-shell).

use serde::{Deserialize, Serialize};

use crate::chat::Message;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    TurnStarted {
        turn_id: String,
    },
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        text: String,
    },
    ThinkingEnd,
    /// A tool call starts. `label` is written by the model in the user's language
    /// ("Reading the login page…").
    StepStarted {
        step_id: String,
        tool: String,
        label: String,
    },
    StepFinished {
        step_id: String,
        ok: bool,
    },
    /// The agent waits for the user to allow or decline this step.
    NeedsConfirmation {
        request_id: String,
        tool: String,
        label: String,
        reason: ConfirmReason,
    },
    /// Before a big change the agent proposes a plan and waits for "Start".
    PlanProposed {
        request_id: String,
        plan: PlanProposal,
    },
    /// The agent asks the user to choose (`ask_user` tool).
    QuestionAsked {
        request_id: String,
        question: String,
        options: Vec<String>,
        /// The user may type their own answer instead of picking an option.
        allow_other: bool,
    },
    Usage {
        usage: TokenUsage,
    },
    /// Older messages were summarized to make room (the chat itself is unchanged).
    ContextCompacted,
    /// A message was added to the conversation.
    MessageAppended {
        message: Message,
    },
    TurnFinished {
        outcome: TurnOutcome,
    },
}

/// A plan for a big change, written by the model in the user's language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanProposal {
    pub title: String,
    /// 3–5 plain-language steps.
    pub steps: Vec<String>,
    /// Rough time estimate in the model's words, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<String>,
}

/// The user's answer to a [`PlanProposal`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PlanDecision {
    Start,
    /// Change the plan as described.
    Revise {
        feedback: String,
    },
    Cancel,
}

/// Contents of `.agentknife/turn.lock`, written while a turn runs. A leftover lock
/// means the turn was interrupted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnLockData {
    pub turn_id: String,
    pub started_at_unix: u64,
    /// The user's request, for "Last time I stopped halfway (doing: …)".
    pub request: String,
    pub touched_files: Vec<String>,
    /// Model calls made so far in this turn.
    pub step: u32,
}

/// Project-relative path of the turn lock.
pub const TURN_LOCK_FILE: &str = ".agentknife/turn.lock";

/// Why a step needs the user's OK (or was refused). The UI shows it through fluent
/// (`confirm-<kind>` with `$detail`), so it reads in the user's language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmReason {
    pub kind: ConfirmKind,
    /// What triggered it: the command or path, shown verbatim.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmKind {
    /// Downloads or installs software.
    Download,
    /// Deletes files or discards changes.
    Delete,
    /// Reads or changes something outside the project folder.
    OutsideProject,
    /// Asks for administrator rights.
    Elevated,
    /// A command whose effect cannot be checked (e.g. encoded PowerShell).
    Unrecognized,
    /// Any change, because the user chose to approve every change.
    Change,
    /// Refused: the path leaves the project.
    PathEscape,
    /// Refused: AgentKnife's own history (`.git`) must not be edited directly.
    ProtectedPath,
    /// Refused: could damage the computer (formatting disks, shutting down…).
    Dangerous,
}

impl ConfirmKind {
    /// Fluent message id for the UI.
    pub fn message_id(self) -> &'static str {
        match self {
            ConfirmKind::Download => "confirm-download",
            ConfirmKind::Delete => "confirm-delete",
            ConfirmKind::OutsideProject => "confirm-outside-project",
            ConfirmKind::Elevated => "confirm-elevated",
            ConfirmKind::Unrecognized => "confirm-unrecognized",
            ConfirmKind::Change => "confirm-change",
            ConfirmKind::PathEscape => "denied-path-escape",
            ConfirmKind::ProtectedPath => "denied-protected-path",
            ConfirmKind::Dangerous => "denied-dangerous",
        }
    }

    /// English explanation for the model (not shown to the user).
    pub fn for_model(self) -> &'static str {
        match self {
            ConfirmKind::Download => "it downloads or installs software",
            ConfirmKind::Delete => "it deletes files or discards changes",
            ConfirmKind::OutsideProject => "it touches files outside the project folder",
            ConfirmKind::Elevated => "it asks for administrator rights",
            ConfirmKind::Unrecognized => "its effect cannot be checked",
            ConfirmKind::Change => "the user approves every change",
            ConfirmKind::PathEscape => "the path is outside the project",
            ConfirmKind::ProtectedPath => "`.git` is managed by AgentKnife and must not be edited",
            ConfirmKind::Dangerous => "it could damage the computer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    /// The answer was cut off by the output limit.
    MaxTokens,
    /// The model or a safety system declined.
    Refused {
        category: Option<String>,
    },
    /// The step limit was reached; the user can let it continue.
    StepLimit,
    /// Building or running kept failing after the automatic fix attempts. The UI
    /// offers: try another way, go back to the last checkpoint, show details.
    Stuck {
        attempts: u32,
        last_error: String,
    },
    Cancelled,
    Failed {
        message: String,
        retryable: bool,
    },
}

impl TurnOutcome {
    /// Turns that ended on their own get a checkpoint; cancelled or failed turns keep
    /// their `turn.lock` for interruption recovery.
    pub fn ended_normally(&self) -> bool {
        !matches!(self, TurnOutcome::Cancelled | TurnOutcome::Failed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_with_tags() {
        let e = AgentEvent::StepStarted {
            step_id: "s1".into(),
            tool: "read_file".into(),
            label: "正在讀取登入頁面".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""event":"step_started""#));
        assert_eq!(serde_json::from_str::<AgentEvent>(&json).unwrap(), e);
        let i18n = crate::i18n::I18n::new(crate::i18n::Locale::ZhTw).unwrap();
        for kind in [
            ConfirmKind::Download,
            ConfirmKind::Delete,
            ConfirmKind::OutsideProject,
            ConfirmKind::Elevated,
            ConfirmKind::Unrecognized,
            ConfirmKind::Change,
            ConfirmKind::PathEscape,
            ConfirmKind::ProtectedPath,
            ConfirmKind::Dangerous,
        ] {
            let args = crate::i18n::args([("detail", "x")]);
            let shown = i18n.t_args(kind.message_id(), &args);
            assert_ne!(
                shown,
                kind.message_id(),
                "missing fluent message for {kind:?}"
            );
        }
        assert!(!TurnOutcome::Cancelled.ended_normally());
        assert!(TurnOutcome::StepLimit.ended_normally());
    }
}
