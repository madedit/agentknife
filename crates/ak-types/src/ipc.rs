//! What AgentKnife's window (ak-shell) and the app (ak-app) say to each other.
//! Every command takes one argument named `request` (or none) and answers with
//! the type named next to it.

use serde::{Deserialize, Serialize};

use crate::agent::PlanDecision;
use crate::chat::Entry;
use crate::doctor::{DoctorReport, GuideStep, SystemGuide};
use crate::preview::Picked;
use crate::settings::AiConnection;

/// Command names.
pub mod cmd {
    /// `()` → [`HomeState`](super::HomeState)
    pub const HOME_STATE: &str = "home_state";
    /// [`NewProject`](super::NewProject) → [`OpenedProject`](super::OpenedProject)
    pub const CREATE_PROJECT: &str = "create_project";
    /// `String` (project id) → [`OpenedProject`](super::OpenedProject)
    pub const OPEN_PROJECT: &str = "open_project";
    /// `()` → `Option<OpenedProject>`: asks for a folder (the system's folder
    /// picker) and opens it as a project; `None` when the user cancels.
    pub const OPEN_FOLDER: &str = "open_folder";
    /// [`DeleteProject`](super::DeleteProject) → [`HomeState`](super::HomeState):
    /// removes a project from the list, and with `folder` also moves its folder
    /// to the system's trash.
    pub const DELETE_PROJECT: &str = "delete_project";
    /// [`ConnectRequest`](super::ConnectRequest) → `Vec<ModelChoice>`
    pub const LIST_MODELS: &str = "list_models";
    /// [`ConnectRequest`](super::ConnectRequest) with a model → [`ModelCheck`](super::ModelCheck)
    pub const CHECK_MODEL: &str = "check_model";
    /// [`ConnectRequest`](super::ConnectRequest) → `()`: the key goes to the
    /// system keychain, the connection to the settings.
    pub const SAVE_CONNECTION: &str = "save_connection";
    /// [`SendMessage`](super::SendMessage) → `()`: starts a turn in the open
    /// project; its progress arrives as [`AGENT_EVENT`](super::AGENT_EVENT).
    pub const SEND_MESSAGE: &str = "send_message";
    /// [`Answer<bool>`](super::Answer) (allow the step?) → `()`
    pub const ANSWER_CONFIRMATION: &str = "answer_confirmation";
    /// [`Answer<PlanDecision>`](super::Answer) → `()`
    pub const ANSWER_PLAN: &str = "answer_plan";
    /// [`Answer<String>`](super::Answer) → `()`
    pub const ANSWER_QUESTION: &str = "answer_question";
    /// `()` → `()`: stops the running turn.
    pub const CANCEL_TURN: &str = "cancel_turn";
    /// `()` → `()`: goes on after the step limit or a cut-off answer.
    pub const CONTINUE_TURN: &str = "continue_turn";
    /// [`StuckChoice`](super::StuckChoice) → `()`
    pub const STUCK_CHOICE: &str = "stuck_choice";
    /// [`InterruptionChoice`](super::InterruptionChoice) → `()`
    pub const RESOLVE_INTERRUPTION: &str = "resolve_interruption";
    /// `()` → [`PreviewStatus`](crate::preview::PreviewStatus) of the open
    /// project; changes then arrive as [`PREVIEW_EVENT`](super::PREVIEW_EVENT).
    pub const PREVIEW_STATUS: &str = "preview_status";
    /// `()` → `()`: starts the open project's app in its own window (not the
    /// preview), for a last check.
    pub const RUN_APP: &str = "run_app";
    /// `()` → `Vec<FileEntry>`: the open project's files (not build output or
    /// AgentKnife's own folder), sorted by path.
    pub const LIST_FILES: &str = "list_files";
    /// `String` (project-relative path) → [`FileText`](super::FileText)
    pub const READ_FILE: &str = "read_file";
    /// [`SaveFile`](super::SaveFile) → `()`: writes the file and tells the
    /// preview (it rebuilds).
    pub const WRITE_FILE: &str = "write_file";
    /// `()` → `Vec<TimelineRow>`, newest first.
    pub const TIMELINE: &str = "timeline";
    /// `String` (entry id) → `()`: the project goes back to that version; the
    /// current work is set aside as an "abandoned" entry first.
    pub const RESTORE: &str = "restore";
    /// [`MarkVersion`](super::MarkVersion) → `()`
    pub const MARK_VERSION: &str = "mark_version";
    /// `()` → [`DoctorCheck`](super::DoctorCheck): checks the development tools.
    pub const DOCTOR_CHECK: &str = "doctor_check";
    /// `()` → [`DoctorCheck`](super::DoctorCheck): installs the missing tools
    /// that AgentKnife keeps in its own folder, then checks again. Progress
    /// arrives as [`DOCTOR_EVENT`](super::DOCTOR_EVENT).
    pub const DOCTOR_INSTALL: &str = "doctor_install";
    /// [`SystemAction`](crate::doctor::SystemAction) → `()`: starts an official
    /// installer or opens a page. The UI asks the user first.
    pub const DOCTOR_ACTION: &str = "doctor_action";
    /// `()` → [`BundleResult`](super::BundleResult), or a
    /// [`BundleFailure`](super::BundleFailure) as the error. Progress arrives as
    /// [`BUNDLE_EVENT`](super::BUNDLE_EVENT).
    pub const BUNDLE: &str = "bundle";
    /// `String` (a file's full path) → `()`: shows it in the file manager.
    pub const REVEAL: &str = "reveal";
    /// `String` (an `https://` address) → `()`: opens it in the user's browser
    /// (links inside AgentKnife's window do not navigate it).
    pub const OPEN_URL: &str = "open_url";
    /// `()` → [`AppSettings`](crate::settings::AppSettings)
    pub const GET_SETTINGS: &str = "get_settings";
    /// [`AppSettings`](crate::settings::AppSettings) → `()` (values are clamped).
    pub const SAVE_SETTINGS: &str = "save_settings";
    /// `()` → [`CrashReports`](super::CrashReports)
    pub const CRASH_REPORTS: &str = "crash_reports";
    /// Lets the user choose a folder (the settings' readable folders): its
    /// absolute path, or `None` when cancelled.
    pub const PICK_FOLDER: &str = "pick_folder";
    /// `()` → `Vec<LogLine>`: the open project's log so far; new lines then
    /// arrive as [`LOG_EVENT`](super::LOG_EVENT).
    pub const GET_LOG: &str = "get_log";
    /// `()` → `()`: empties the open project's log.
    pub const CLEAR_LOG: &str = "clear_log";

    /// Every command, for checking that the app handles them all.
    pub const ALL: &[&str] = &[
        HOME_STATE,
        CREATE_PROJECT,
        OPEN_PROJECT,
        DELETE_PROJECT,
        OPEN_FOLDER,
        LIST_MODELS,
        CHECK_MODEL,
        SAVE_CONNECTION,
        SEND_MESSAGE,
        ANSWER_CONFIRMATION,
        ANSWER_PLAN,
        ANSWER_QUESTION,
        CANCEL_TURN,
        CONTINUE_TURN,
        STUCK_CHOICE,
        RESOLVE_INTERRUPTION,
        PREVIEW_STATUS,
        RUN_APP,
        LIST_FILES,
        READ_FILE,
        WRITE_FILE,
        TIMELINE,
        RESTORE,
        MARK_VERSION,
        DOCTOR_CHECK,
        DOCTOR_INSTALL,
        DOCTOR_ACTION,
        BUNDLE,
        REVEAL,
        OPEN_URL,
        GET_SETTINGS,
        SAVE_SETTINGS,
        CRASH_REPORTS,
        PICK_FOLDER,
        GET_LOG,
        CLEAR_LOG,
    ];
}

/// Tauri event carrying [`DoctorProgress`](crate::doctor::DoctorProgress)
/// while tools install.
pub const DOCTOR_EVENT: &str = "doctor-progress";
/// Tauri event carrying [`BundleProgress`] while the app is packaged.
pub const BUNDLE_EVENT: &str = "bundle-progress";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub report: DoctorReport,
    /// How to get each missing system dependency.
    #[serde(default)]
    pub guides: Vec<SystemGuide>,
}

/// One update while packaging: a new stage (fluent `bundle-…`) or a line of
/// build output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BundleProgress {
    Stage { message: String },
    Output { line: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleResult {
    /// Full path of the packed file.
    pub path: String,
    /// Its file name (`My App_0.1.0_x64.zip`): `$file` in the steps.
    pub file: String,
    /// The app's folder and program name: `$app` in the steps.
    pub app: String,
    /// The system it was made for (`windows`, `macos`, `linux`).
    pub os: String,
    /// How to install and start it (`ak_bundle::guide::install_guide`).
    pub steps: Vec<GuideStep>,
}

/// Why packaging failed; the UI shows fluent `bundle-error-<kind>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleFailure {
    /// `build-failed`, `preview-code`, `unsupported`, `app-info`, `cancelled`,
    /// `other`.
    pub kind: String,
    /// Technical detail (compiler output) for "Details".
    #[serde(default)]
    pub detail: String,
}

/// Tauri event carrying the open project's
/// [`PreviewStatus`](crate::preview::PreviewStatus) whenever it changes.
pub const PREVIEW_EVENT: &str = "preview-status";
/// Tauri event carrying new lines of the open project's log
/// (`Vec<`[`LogLine`](crate::log::LogLine)`>`).
pub const LOG_EVENT: &str = "project-log";

/// A file or folder of the open project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Project-relative, `/` separators.
    pub path: String,
    pub dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileText {
    pub path: String,
    /// Empty when `binary`.
    pub text: String,
    /// Not text (an image, …): shown, not edited.
    #[serde(default)]
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveFile {
    pub path: String,
    pub text: String,
}

/// A timeline entry for the UI (`ak_project::TimelineEntry` without the git ids).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineRow {
    pub id: String,
    /// Fluent `timeline-<kind>` (`turn`, `task`, `bundle`, `marked`, …).
    pub kind: String,
    /// Plain language, usually the user's request.
    pub label: String,
    /// Unix seconds.
    pub created_at: u64,
    #[serde(default)]
    pub changed_files: Vec<String>,
    /// The project is at this version now (the newest entry).
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkVersion {
    /// The user's name for it ("Before the redesign").
    pub label: String,
}

/// Tauri event carrying each [`AgentEvent`](crate::agent::AgentEvent) of the
/// open project's running turn.
pub const AGENT_EVENT: &str = "agent-event";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenedProject {
    pub project: ProjectRow,
    /// The request of a turn that was interrupted (crash, force-quit…): the
    /// user chooses to continue or go back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<String>,
    /// The saved conversation (context notes and checkpoints included; the chat
    /// shows messages only).
    #[serde(default)]
    pub history: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendMessage {
    pub text: String,
    /// What the user pointed at in the preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Picked>,
}

/// The user's answer to a request the agent is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer<T> {
    pub request_id: String,
    pub answer: T,
}

pub type PlanAnswer = Answer<PlanDecision>;

/// After the automatic fixes gave up (`TurnOutcome::Stuck`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StuckChoice {
    TryAnotherWay,
    RollBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptionChoice {
    /// The AI picks up where it stopped.
    Continue,
    /// Back to the last checkpoint; the half-done work is set aside (restorable).
    GoBack,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteProject {
    pub id: String,
    /// Also move the project's folder to the trash (else it stays on disk).
    pub folder: bool,
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

/// Crash reports kept on this computer (never sent by AgentKnife).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReports {
    /// Where they are (for "Show in folder").
    pub folder: String,
    /// Newest first.
    pub reports: Vec<CrashReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    pub path: String,
    /// Unix seconds.
    pub created_at: u64,
    /// The report itself (for "Copy").
    pub text: String,
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
