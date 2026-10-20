//! What AgentKnife's window (ak-shell) and the app (ak-app) say to each other.
//! Every command takes one argument named `request` (or none) and answers with
//! the type named next to it.

use serde::{Deserialize, Serialize};

use crate::settings::AiConnection;

/// Command names.
pub mod cmd {
    /// `()` → [`HomeState`](super::HomeState)
    pub const HOME_STATE: &str = "home_state";
    /// [`NewProject`](super::NewProject) → [`ProjectRow`](super::ProjectRow)
    pub const CREATE_PROJECT: &str = "create_project";
    /// `String` (project id) → [`ProjectRow`](super::ProjectRow)
    pub const OPEN_PROJECT: &str = "open_project";
    /// `()` → `Option<ProjectRow>`: asks for a folder (the system's folder picker)
    /// and opens it as a project; `None` when the user cancels.
    pub const OPEN_FOLDER: &str = "open_folder";
    /// [`ConnectRequest`](super::ConnectRequest) → `Vec<ModelChoice>`
    pub const LIST_MODELS: &str = "list_models";
    /// [`ConnectRequest`](super::ConnectRequest) with a model → [`ModelCheck`](super::ModelCheck)
    pub const CHECK_MODEL: &str = "check_model";
    /// [`ConnectRequest`](super::ConnectRequest) → `()`: the key goes to the
    /// system keychain, the connection to the settings.
    pub const SAVE_CONNECTION: &str = "save_connection";
}

/// A home-screen row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    /// Shown as is.
    pub path: String,
    /// Unix seconds.
    pub last_opened_at: u64,
    /// False when the folder was moved or deleted outside AgentKnife.
    pub exists: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeState {
    /// Most recently opened first.
    pub projects: Vec<ProjectRow>,
    /// `None` until an AI is connected.
    pub ai: Option<AiConnection>,
    /// Where new projects go (shown in the new-project dialog).
    pub projects_folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub name: String,
}

/// A connection being set up. `connection.model` may be empty when listing
/// models. The key is only sent, never sent back.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectRequest {
    pub connection: AiConnection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl std::fmt::Debug for ConnectRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectRequest")
            .field("connection", &self.connection)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// What the chosen model can do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCheck {
    /// Without it the AI cannot change files: AgentKnife warns.
    pub tool_calling: bool,
    /// Without it the AI reads a page description instead of screenshots.
    pub vision: bool,
}

/// Why connecting failed, in words for the user (fluent `connect-error-<key>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectError {
    /// Nothing answered at the address (local server not running, no internet).
    #[error("nothing answered at that address")]
    Unreachable { detail: String },
    #[error("the API key was not accepted")]
    WrongKey,
    /// Something answered, but not an AI service (wrong path or port).
    #[error("that address is not an AI service")]
    WrongAddress { detail: String },
    #[error("the service has no models")]
    NoModels,
    #[error("too many requests; try again in a moment")]
    RateLimited,
    #[error("{detail}")]
    Other { detail: String },
}

impl ConnectError {
    pub fn key(&self) -> &'static str {
        match self {
            ConnectError::Unreachable { .. } => "unreachable",
            ConnectError::WrongKey => "wrong-key",
            ConnectError::WrongAddress { .. } => "wrong-address",
            ConnectError::NoModels => "no-models",
            ConnectError::RateLimited => "rate-limited",
            ConnectError::Other { .. } => "other",
        }
    }

    /// Technical detail for "Details", if any.
    pub fn detail(&self) -> Option<&str> {
        match self {
            ConnectError::Unreachable { detail }
            | ConnectError::WrongAddress { detail }
            | ConnectError::Other { detail } => Some(detail),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetKind {
    /// A service on the internet (needs an account and a key; may cost money).
    Cloud,
    /// A program on this computer (free, private; needs a capable computer).
    Local,
}

/// A choice in the connect wizard. Names and descriptions are fluent
/// `connect-preset-<id>` and `connect-preset-<id>-about`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    /// [`AiConnection::provider`].
    pub id: &'static str,
    pub kind: PresetKind,
    /// `None`: the user types the address.
    pub base_url: Option<&'static str>,
    pub needs_key: bool,
    /// Where to get a key, or the program.
    pub page: Option<&'static str>,
    /// Picked first when the service offers it.
    pub suggested_model: Option<&'static str>,
}

pub const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "anthropic",
        kind: PresetKind::Cloud,
        base_url: None,
        needs_key: true,
        page: Some("https://console.anthropic.com/settings/keys"),
        suggested_model: Some("claude-opus-5-5"),
    },
    ProviderPreset {
        id: "openai",
        kind: PresetKind::Cloud,
        base_url: None,
        needs_key: true,
        page: Some("https://platform.openai.com/api-keys"),
        suggested_model: None,
    },
    ProviderPreset {
        id: "openrouter",
        kind: PresetKind::Cloud,
        base_url: None,
        needs_key: true,
        page: Some("https://openrouter.ai/keys"),
        suggested_model: None,
    },
    ProviderPreset {
        id: "ollama",
        kind: PresetKind::Local,
        base_url: Some("http://localhost:11434/v1"),
        needs_key: false,
        page: Some("https://ollama.com/download"),
        suggested_model: None,
    },
    ProviderPreset {
        id: "lmstudio",
        kind: PresetKind::Local,
        base_url: Some("http://localhost:1234/v1"),
        needs_key: false,
        page: Some("https://lmstudio.ai"),
        suggested_model: None,
    },
    ProviderPreset {
        id: "openai-compatible",
        kind: PresetKind::Cloud,
        base_url: None,
        needs_key: false,
        page: None,
        suggested_model: None,
    },
];

pub fn preset(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

impl ProviderPreset {
    /// The user must type an address (any other OpenAI-compatible service).
    pub fn asks_address(&self) -> bool {
        self.id == "openai-compatible"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_never_show_in_debug_and_errors_round_trip() {
        let req = ConnectRequest {
            connection: AiConnection {
                provider: "anthropic".into(),
                base_url: None,
                model: String::new(),
                effort: None,
            },
            api_key: Some("sk-secret".into()),
        };
        assert!(!format!("{req:?}").contains("sk-secret"));
        let e = ConnectError::Unreachable {
            detail: "connection refused".into(),
        };
        let json = serde_json::to_string(&e).unwrap_or_default();
        assert_eq!(serde_json::from_str::<ConnectError>(&json).ok(), Some(e));
    }

    #[test]
    fn presets_are_consistent() {
        for p in PRESETS {
            assert_eq!(preset(p.id), Some(p));
            if p.kind == PresetKind::Local {
                assert!(p.base_url.is_some() && !p.needs_key, "{}", p.id);
            }
        }
        assert!(preset("openai-compatible").is_some_and(ProviderPreset::asks_address));
    }
}
