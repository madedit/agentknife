//! What the app keeps between commands: the projects list, the settings, the
//! usage ledger, and the open project with its preview and running turn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ak_agent::usage::UsageLedger;
use ak_preview::{PreviewSession, Tools};
use ak_project::{Project, Registry};
use ak_types::agent::PlanDecision;
use ak_types::chat::Conversation;
use ak_types::settings::AiConnection;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

/// `<data>/AgentKnife/settings.json`. API keys are never here (keychain).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ai: Option<AiConnection>,
}

impl Settings {
    pub fn load(state_dir: &Path) -> Settings {
        std::fs::read_to_string(state_dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, state_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        let tmp = state_dir.join(format!("{SETTINGS_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, state_dir.join(SETTINGS_FILE))?;
        Ok(())
    }
}

const SETTINGS_FILE: &str = "settings.json";

/// Answers the agent is waiting for, by request id.
#[derive(Default)]
pub struct Pending {
    pub confirmations: HashMap<String, oneshot::Sender<bool>>,
    pub plans: HashMap<String, oneshot::Sender<PlanDecision>>,
    pub questions: HashMap<String, oneshot::Sender<Option<String>>>,
}

/// The project in the window.
pub struct OpenProject {
    pub root: PathBuf,
    pub project: Arc<std::sync::Mutex<Project>>,
    pub preview: Arc<PreviewSession>,
    /// Held by the running turn.
    pub conversation: Arc<Mutex<Conversation>>,
    /// The running turn, if any.
    pub cancel: Option<CancellationToken>,
    /// The newest timeline entry when the last turn started (for "roll back").
    pub before_turn: Option<String>,
    /// `cargo tauri dev` for "Run in a real window".
    pub app_window: Option<tokio::process::Child>,
}

pub struct Inner {
    pub registry: Registry,
    pub settings: Settings,
    pub open: Option<OpenProject>,
}

/// Shared by all commands (`tauri::State`).
pub struct AppState {
    pub inner: Mutex<Inner>,
    pub pending: Arc<std::sync::Mutex<Pending>>,
    pub ledger: Arc<UsageLedger>,
    pub state_dir: PathBuf,
    pub tools: Tools,
    pub toolchain: ak_doctor::Toolchain,
    pub pinned: ak_doctor::Pinned,
}

impl AppState {
    pub fn load() -> anyhow::Result<AppState> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let state_dir = dirs::data_dir()
            .unwrap_or_else(|| home.clone())
            .join("AgentKnife");
        let registry = Registry::load_default()?;
        let pinned = ak_doctor::Pinned::builtin().map_err(anyhow::Error::msg)?;
        let toolchain = ak_doctor::Toolchain::new(ak_doctor::Toolchain::default_root(), &pinned);
        Ok(AppState {
            inner: Mutex::new(Inner {
                registry,
                settings: Settings::load(&state_dir),
                open: None,
            }),
            pending: Arc::new(std::sync::Mutex::new(Pending::default())),
            ledger: Arc::new(UsageLedger::load(&state_dir)),
            tools: Tools::from_toolchain(&toolchain),
            toolchain,
            pinned,
            state_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_a_missing_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let s = Settings {
            ai: Some(AiConnection {
                provider: "ollama".into(),
                base_url: Some("http://localhost:11434/v1".into()),
                model: "qwen3:8b".into(),
                effort: None,
            }),
        };
        s.save(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Settings::load(dir.path()), s);
        let text = std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap_or_default();
        assert!(!text.contains("key"), "no keys in settings: {text}");
    }
}
