//! What the app keeps between commands: the projects list, the settings, the
//! usage ledger, and the open project with its preview and running turn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ak_agent::usage::UsageLedger;
use ak_preview::log::LogBook;
use ak_preview::{PreviewSession, Tools};
use ak_project::{Project, Registry};
use ak_providers::Provider;
use ak_types::agent::PlanDecision;
use ak_types::chat::Conversation;
use ak_types::settings::AppSettings;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

/// Loads and saves [`AppSettings`] in `<data>/AgentKnife/settings.json`.
pub trait SettingsFile: Sized {
    fn load(state_dir: &Path) -> Self;
    fn save(&self, state_dir: &Path) -> anyhow::Result<()>;
}

impl SettingsFile for AppSettings {
    fn load(state_dir: &Path) -> AppSettings {
        std::fs::read_to_string(state_dir.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<AppSettings>(&t).ok())
            .unwrap_or_default()
            .clamped()
    }

    fn save(&self, state_dir: &Path) -> anyhow::Result<()> {
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
    /// Everything the project's programs printed (the Log tab).
    pub log: LogBook,
}

pub struct Inner {
    pub registry: Registry,
    pub settings: AppSettings,
    pub open: Option<OpenProject>,
}

/// AgentKnife's data folder (`<data>/AgentKnife`).
pub fn state_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dirs::data_dir().unwrap_or(home).join("AgentKnife")
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
    /// Used instead of the connection's AI (tests: a scripted one).
    pub provider: Option<Arc<dyn Provider>>,
    /// Other AI tools operating AgentKnife (MCP).
    pub mcp: crate::mcp::McpState,
}

impl AppState {
    pub fn load() -> anyhow::Result<AppState> {
        AppState::new(
            state_dir(),
            Registry::load_default()?,
            ak_doctor::Toolchain::default_root(),
        )
    }

    /// The state kept in `state_dir`, with the projects of `registry` and the
    /// toolchain in `toolchain_root`.
    pub fn new(
        state_dir: PathBuf,
        registry: Registry,
        toolchain_root: PathBuf,
    ) -> anyhow::Result<AppState> {
        let pinned = ak_doctor::Pinned::builtin().map_err(anyhow::Error::msg)?;
        let toolchain = ak_doctor::Toolchain::new(toolchain_root, &pinned);
        Ok(AppState {
            inner: Mutex::new(Inner {
                registry,
                settings: AppSettings::load(&state_dir),
                open: None,
            }),
            pending: Arc::new(std::sync::Mutex::new(Pending::default())),
            ledger: Arc::new(UsageLedger::load(&state_dir)),
            tools: Tools::from_toolchain(&toolchain),
            toolchain,
            pinned,
            state_dir,
            provider: None,
            mcp: crate::mcp::McpState::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use ak_types::settings::{AiConnection, Permissions};

    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_a_missing_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(AppSettings::load(dir.path()), AppSettings::default());
        let s = AppSettings {
            ai: Some(AiConnection {
                provider: "ollama".into(),
                base_url: Some("http://localhost:11434/v1".into()),
                model: "qwen3:8b".into(),
                effort: None,
            }),
            permissions: Permissions::AskEveryChange,
            daily_cap_usd: Some(2.5),
            ..AppSettings::default()
        };
        s.save(dir.path()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(AppSettings::load(dir.path()), s);
        let text = std::fs::read_to_string(dir.path().join(SETTINGS_FILE)).unwrap_or_default();
        assert!(!text.contains("key"), "no keys in settings: {text}");
    }
}
