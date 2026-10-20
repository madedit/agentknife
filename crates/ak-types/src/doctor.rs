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
