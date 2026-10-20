//! The development-tools check (ak-doctor) as the UI sees it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub items: Vec<CheckItem>,
}

impl DoctorReport {
    /// Everything needed to build apps is in place.
    pub fn ready(&self) -> bool {
        self.items.iter().all(|i| i.status == CheckStatus::Ok)
    }

    pub fn item(&self, id: &str) -> Option<&CheckItem> {
        self.items.iter().find(|i| i.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckItem {
    /// Stable id; the UI shows fluent `doctor-item-<id>`.
    pub id: String,
    pub scope: CheckScope,
    pub status: CheckStatus,
    /// Version found, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,
    /// Version required (pinned tools only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wanted: Option<String>,
    /// Technical detail for "Details" (raw output, error).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckScope {
    /// Installed by AgentKnife in its own folder.
    Isolated,
    /// Part of the operating system; the user installs it with AgentKnife's help.
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Missing,
    /// Installed, but not the version AgentKnife uses.
    WrongVersion,
    /// The check itself failed (see `detail`).
    Error,
}

/// One step of an install, for the progress display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorProgress {
    /// The doctor item being installed (`doctor-item-<id>`).
    pub item: String,
    /// 1-based position among the items being installed.
    pub step: u32,
    pub total: u32,
    pub phase: InstallPhase,
}

/// The UI shows fluent `doctor-install-<phase>` (kebab-case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallPhase {
    /// `size` is unknown when the server does not say.
    Download {
        done: u64,
        size: Option<u64>,
    },
    Verify,
    Unpack,
    /// Running the official installer (rustup).
    Setup,
    /// No prebuilt download: compiling from source with `cargo install` (slow).
    Compile,
    Done,
    Failed,
}

impl InstallPhase {
    /// The fluent key suffix.
    pub fn key(&self) -> &'static str {
        match self {
            InstallPhase::Download { .. } => "download",
            InstallPhase::Verify => "verify",
            InstallPhase::Unpack => "unpack",
            InstallPhase::Setup => "setup",
            InstallPhase::Compile => "compile",
            InstallPhase::Done => "done",
            InstallPhase::Failed => "failed",
        }
    }
}

/// How to get one missing system dependency (or, on Linux, all missing
/// packages at once). The user always confirms an action first; afterwards the
/// UI offers "Check again".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemGuide {
    /// The doctor item id, or `linux-packages` for the combined Linux guide.
    pub item: String,
    /// The items this guide covers.
    pub covers: Vec<String>,
    pub steps: Vec<GuideStep>,
    /// The official installer AgentKnife can start, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<SystemAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuideStep {
    /// Fluent message id (`doctor-guide-…`).
    pub message: String,
    /// A command the user copies into a terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A page to open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// Official installers AgentKnife can start. The UI shows fluent
/// `doctor-action-<key>` on the button and `doctor-confirm-<key>` in the
/// confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemAction {
    /// `winget install` of Visual Studio Build Tools with the C++ workload.
    WingetBuildTools,
    /// `winget install` of the WebView2 runtime.
    WingetWebView2,
    /// Opens the Visual Studio Installer (to add a missing component).
    OpenVsInstaller,
    /// `xcode-select --install`: Apple's own install dialog.
    XcodeInstall,
    /// Opens an official download page in the browser.
    OpenPage { url: String },
}

impl SystemAction {
    /// The fluent key suffix.
    pub fn key(&self) -> &'static str {
        match self {
            SystemAction::WingetBuildTools => "winget-build-tools",
            SystemAction::WingetWebView2 => "winget-webview2",
            SystemAction::OpenVsInstaller => "open-vs-installer",
            SystemAction::XcodeInstall => "xcode-install",
            SystemAction::OpenPage { .. } => "open-page",
        }
    }
}
