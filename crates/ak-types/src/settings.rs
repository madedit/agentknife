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
    /// The connection in use.
    pub ai: Option<AiConnection>,
    /// The last connection saved for each provider (one per provider), so the
    /// connect wizard fills in its address and model again. Only the connect
    /// wizard changes it.
    pub ai_known: Vec<AiConnection>,
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
    /// Folders (absolute paths) whose files the AI reads without asking, like
    /// the toolchain's library sources. Never written to.
    pub read_folders: Vec<String>,
    /// "Let other AI tools operate AgentKnife" (MCP, P1-MCP-001). Only
    /// `set_mcp` changes it.
    pub mcp_enabled: bool,
    /// Tools the user let in (by name, e.g. "Claude Code"); asked once each.
    pub mcp_allowed: Vec<String>,
}

impl AppSettings {
    pub const FIX_ATTEMPTS: std::ops::RangeInclusive<u32> = 1..=10;
    pub const KEEP_TURNS: std::ops::RangeInclusive<u32> = 10..=500;

    /// Uses `c` from now on and remembers it for its provider.
    pub fn connect(&mut self, c: AiConnection) {
        self.ai_known.retain(|k| k.provider != c.provider);
        self.ai_known.push(c.clone());
        self.ai = Some(c);
    }

    /// What is known per provider: the remembered ones, and the one in use
    /// (settings from before `ai_known` only have that).
    pub fn known_connections(&self) -> Vec<AiConnection> {
        let mut known = self.ai_known.clone();
        if let Some(ai) = &self.ai {
            known.retain(|k| k.provider != ai.provider);
            known.push(ai.clone());
        }
        known
    }

    /// Values kept in their ranges; a limit of 0 or less means none.
    pub fn clamped(mut self) -> AppSettings {
        self.max_fix_attempts = self
            .max_fix_attempts
            .clamp(*Self::FIX_ATTEMPTS.start(), *Self::FIX_ATTEMPTS.end());
        self.keep_turns = self
            .keep_turns
            .clamp(*Self::KEEP_TURNS.start(), *Self::KEEP_TURNS.end());
        self.daily_cap_usd = self.daily_cap_usd.filter(|c| c.is_finite() && *c > 0.0);
        let mut folders: Vec<String> = Vec::new();
        for f in self.read_folders.drain(..) {
            let f = f.trim().to_owned();
            if !f.is_empty() && !folders.contains(&f) {
                folders.push(f);
            }
        }
        self.read_folders = folders;
        self
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            ai: None,
            ai_known: Vec::new(),
            language: None,
            permissions: Permissions::default(),
            plans: Plans::default(),
            max_fix_attempts: 3,
            keep_turns: 50,
            daily_cap_usd: None,
            read_folders: Vec::new(),
            mcp_enabled: false,
            mcp_allowed: Vec::new(),
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
        assert!(old.read_folders.is_empty());

        let wild = AppSettings {
            max_fix_attempts: 0,
            keep_turns: 100_000,
            daily_cap_usd: Some(-1.0),
            read_folders: vec![
                " D:\\docs ".into(),
                String::new(),
                "D:\\docs".into(),
                "/srv/a".into(),
            ],
            ..AppSettings::default()
        }
        .clamped();
        assert_eq!((wild.max_fix_attempts, wild.keep_turns), (1, 500));
        assert_eq!(wild.daily_cap_usd, None);
        assert_eq!(wild.read_folders, ["D:\\docs", "/srv/a"]);
        let json = serde_json::to_string(&AppSettings {
            language: Some(crate::i18n::Locale::ZhTw),
            ..AppSettings::default()
        })
        .unwrap();
        assert!(json.contains(r#""language":"zh-TW""#), "{json}");
    }

    #[test]
    fn each_provider_remembers_its_last_connection() {
        let mut s = AppSettings::default();
        s.connect(global());
        let claude = AiConnection {
            provider: "anthropic".into(),
            base_url: None,
            model: "claude-opus-5-5".into(),
            effort: None,
        };
        s.connect(claude.clone());
        s.connect(AiConnection {
            model: "qwen3:32b".into(),
            ..global()
        });
        assert_eq!(s.ai.as_ref().map(|a| a.model.as_str()), Some("qwen3:32b"));
        let known = s.known_connections();
        assert_eq!(known.len(), 2, "{known:?}");
        assert!(known.contains(&claude));
        assert!(known.iter().any(|k| k.model == "qwen3:32b"));

        // A file from before `ai_known`: the one in use is known.
        let old: AppSettings =
            serde_json::from_str(r#"{"ai":{"provider":"ollama","model":"qwen3:8b"}}"#).unwrap();
        assert_eq!(old.known_connections().len(), 1);
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
