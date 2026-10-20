//! Settings shared by the UI and the core crates.

use serde::{Deserialize, Serialize};

/// Which AI to use. API keys are never stored here (OS keychain, P1-PROV-004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConnection {
    /// Provider id: `anthropic`, `openai`, `ollama`, `lmstudio`, `openrouter`,
    /// `openai-compatible`.
    pub provider: String,
    /// `None` uses the provider's default endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    pub model: String,
    /// `low` … `max`; `None` uses the model default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Per-project changes to the global [`AiConnection`]; unset fields inherit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl AiConnection {
    /// The connection a project actually uses. Switching provider drops the global
    /// endpoint, since it belongs to the global provider.
    pub fn with_override(&self, o: &AiOverride) -> AiConnection {
        let provider_changed = o.provider.as_ref().is_some_and(|p| *p != self.provider);
        AiConnection {
            provider: o.provider.clone().unwrap_or_else(|| self.provider.clone()),
            base_url: o.base_url.clone().or_else(|| {
                if provider_changed {
                    None
                } else {
                    self.base_url.clone()
                }
            }),
            model: o.model.clone().unwrap_or_else(|| self.model.clone()),
            effort: o.effort.clone().or_else(|| self.effort.clone()),
        }
    }
}

/// How freely the AI changes the project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permissions {
    /// Changes inside the project happen; risky steps ask first.
    #[default]
    Standard,
    /// Every change and command asks first.
    AskEveryChange,
}

/// When the AI shows a plan and waits for "Start".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plans {
    /// For big changes (the AI decides).
    #[default]
    BigChanges,
    /// Before every change.
    Always,
    Never,
}

/// Everything the settings page changes (`<data>/AgentKnife/settings.json`).
/// API keys are never here (keychain).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub ai: Option<AiConnection>,
    /// `None` follows the system.
    pub language: Option<crate::i18n::Locale>,
    pub permissions: Permissions,
    pub plans: Plans,
    /// Automatic fixes after a failed build before the AI stops and asks.
    pub max_fix_attempts: u32,
    /// Recent changes kept one by one in the version history (older ones fold
    /// into their task; tasks, packaging and marked versions are kept forever).
    pub keep_turns: u32,
    /// Daily AI spending limit in US dollars, across all projects.
    pub daily_cap_usd: Option<f64>,
}

impl AppSettings {
    pub const FIX_ATTEMPTS: std::ops::RangeInclusive<u32> = 1..=10;
    pub const KEEP_TURNS: std::ops::RangeInclusive<u32> = 10..=500;

    /// Values kept in their ranges; a limit of 0 or less means none.
    pub fn clamped(mut self) -> AppSettings {
        self.max_fix_attempts = self
            .max_fix_attempts
            .clamp(*Self::FIX_ATTEMPTS.start(), *Self::FIX_ATTEMPTS.end());
        self.keep_turns = self
            .keep_turns
            .clamp(*Self::KEEP_TURNS.start(), *Self::KEEP_TURNS.end());
        self.daily_cap_usd = self.daily_cap_usd.filter(|c| c.is_finite() && *c > 0.0);
        self
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            ai: None,
            language: None,
            permissions: Permissions::default(),
            plans: Plans::default(),
            max_fix_attempts: 3,
            keep_turns: 50,
            daily_cap_usd: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_from_older_files_and_out_of_range_values() {
        // The APP-001 file had only the connection.
        let old: AppSettings =
            serde_json::from_str(r#"{"ai":{"provider":"ollama","model":"qwen3:8b"}}"#).unwrap();
        assert_eq!(old.max_fix_attempts, 3);
        assert_eq!(old.keep_turns, 50);
        assert_eq!(old.permissions, Permissions::Standard);
        assert!(old.ai.is_some() && old.language.is_none());

        let wild = AppSettings {
            max_fix_attempts: 0,
            keep_turns: 100_000,
            daily_cap_usd: Some(-1.0),
            ..AppSettings::default()
        }
        .clamped();
        assert_eq!((wild.max_fix_attempts, wild.keep_turns), (1, 500));
        assert_eq!(wild.daily_cap_usd, None);
        let json = serde_json::to_string(&AppSettings {
            language: Some(crate::i18n::Locale::ZhTw),
            ..AppSettings::default()
        })
        .unwrap();
        assert!(json.contains(r#""language":"zh-TW""#), "{json}");
    }

    fn global() -> AiConnection {
        AiConnection {
            provider: "ollama".into(),
            base_url: Some("http://localhost:11434/v1".into()),
            model: "qwen3:8b".into(),
            effort: Some("medium".into()),
        }
    }

    #[test]
    fn override_merges_field_by_field() {
        assert_eq!(global().with_override(&AiOverride::default()), global());
        let model_only = global().with_override(&AiOverride {
            model: Some("qwen3:32b".into()),
            ..AiOverride::default()
        });
        assert_eq!(model_only.model, "qwen3:32b");
        assert_eq!(model_only.base_url, global().base_url);

        let other_provider = global().with_override(&AiOverride {
            provider: Some("anthropic".into()),
            model: Some("claude-opus-5-5".into()),
            ..AiOverride::default()
        });
        assert_eq!(
            other_provider.base_url, None,
            "Ollama's URL is not Anthropic's"
        );
        assert_eq!(other_provider.effort.as_deref(), Some("medium"));
    }
}
