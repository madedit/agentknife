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
        reason: String,
    },
    Usage {
        usage: TokenUsage,
    },
    /// A message was added to the conversation (persist it).
    MessageAppended {
        message: Message,
    },
    TurnFinished {
        outcome: TurnOutcome,
    },
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
        assert!(!TurnOutcome::Cancelled.ended_normally());
        assert!(TurnOutcome::StepLimit.ended_normally());
    }
}
