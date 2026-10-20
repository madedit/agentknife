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

#[cfg(test)]
mod tests {
    use super::*;

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
