//! The live preview's state, as the UI and the agent see it.

use serde::{Deserialize, Serialize};

/// One preview process (the screens' dev server, or the app's backend).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerState {
    /// Not running (or not used by this stack).
    #[default]
    Off,
    /// Started, nothing built yet.
    Starting,
    /// Compiling (the first build, or after a change).
    Building,
    Ready,
    /// The build failed or the process stopped. `output` is the end of what it
    /// printed (compiler errors), for the agent to fix.
    Error {
        output: String,
    },
}

impl ServerState {
    pub fn is_ready(&self) -> bool {
        *self == ServerState::Ready
    }

    pub fn is_error(&self) -> bool {
        matches!(self, ServerState::Error { .. })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewStatus {
    pub frontend: ServerState,
    pub backend: ServerState,
    /// The page to show (with the bridge address), once the screens are built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl PreviewStatus {
    /// The app can be used: screens built, backend (if any) running.
    pub fn ready(&self) -> bool {
        self.frontend.is_ready() && matches!(self.backend, ServerState::Ready | ServerState::Off)
    }

    /// The first build error to show or fix.
    pub fn error(&self) -> Option<&str> {
        [&self.frontend, &self.backend]
            .into_iter()
            .find_map(|s| match s {
                ServerState::Error { output } => Some(output.as_str()),
                _ => None,
            })
    }
}

// ---- What the preview page reports (same JSON as `ak_bridge::protocol`) ----
//
// ak-bridge ships inside generated apps and cannot be a dependency here, so these
// are copies; ak-preview's tests check both directions against ak-bridge.

/// The bridge's answers when no page ran an action (`ak_bridge::protocol`).
pub const PAGE_NOT_OPEN: &str = "the preview page is not open";
pub const PAGE_CLOSED: &str = "the preview page closed";
/// The page took too long to answer (AgentKnife's own; the bridge never sends it).
pub const PAGE_TIMEOUT: &str = "the preview page did not answer in time";
/// [`PAGE_TIMEOUT`], after which AgentKnife loaded the page again.
pub const PAGE_RELOADED_STUCK: &str = "the preview page did not answer in time and was reloaded";
/// The longest a `hold` or `key` action keeps something pressed.
pub const MAX_HOLD_MS: u32 = 10_000;

/// An action the agent performs in the preview page (control channel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ControlAction {
    Click {
        ak_id: String,
    },
    Hold {
        ak_id: String,
        ms: u32,
    },
    Key {
        key: String,
        #[serde(default)]
        ms: u32,
    },
    Type {
        ak_id: String,
        text: String,
        #[serde(default)]
        enter: bool,
    },
    DomSummary,
    AppState,
    PrepareScreenshot {
        #[serde(default)]
        ak_id: Option<String>,
    },
}

impl ControlAction {
    /// Reads the page without changing it (safe to repeat).
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            ControlAction::DomSummary
                | ControlAction::AppState
                | ControlAction::PrepareScreenshot { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomItem {
    pub depth: usize,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ak_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomSummary {
    pub title: String,
    pub url: String,
    pub items: Vec<DomItem>,
    #[serde(default)]
    pub omitted: usize,
    /// Uncaught script errors on the page since it loaded, oldest first (the
    /// newest few). A blank canvas often means one of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl DomSummary {
    /// Compact text for a model, one element per line, indented by nesting:
    /// `button #add-button "Add item"`.
    pub fn render(&self) -> String {
        let mut out = format!("Page: {} ({})\n", self.title, self.url);
        for item in &self.items {
            let mut line = format!("{}{}", "  ".repeat(item.depth), item.tag);
            if let Some(id) = &item.ak_id {
                line.push_str(&format!(" #{id}"));
            }
            for (k, v) in &item.attrs {
                line.push_str(&format!(" {k}={v}"));
            }
            if let Some(v) = &item.value {
                line.push_str(&format!(" value={v:?}"));
            }
            if let Some(c) = item.checked {
                line.push_str(if c { " checked" } else { " unchecked" });
            }
            if item.disabled {
                line.push_str(" disabled");
            }
            if let Some(t) = &item.text {
                line.push_str(&format!(" {t:?}"));
            }
            out.push_str(&line);
            out.push('\n');
        }
        if self.omitted > 0 {
            out.push_str(&format!("({} more elements not shown)\n", self.omitted));
        }
        if !self.errors.is_empty() {
            out.push_str("Errors on the page:\n");
            for e in &self.errors {
                out.push_str(&format!("- {e}\n"));
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    pub url: String,
    pub title: String,
    pub local_storage: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotArea {
    pub rect: Rect,
    pub viewport: Size,
    pub device_pixel_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit: Option<serde_json::Value>,
}

/// What the user pointed at in the preview (inspector).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picked {
    pub ak_id: Option<String>,
    pub exact: bool,
    pub path: String,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub rect: Rect,
    pub viewport: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<CanvasPoint>,
}

// ---- Point-and-select (same JSON as `ak_bridge::protocol`'s inspector) ----
//
// AgentKnife shows the preview in an iframe and talks to it with `postMessage`.
// Every message is a JSON string `{"ak_inspector": …}`.

/// The key that marks inspector messages.
pub const INSPECTOR_KEY: &str = "ak_inspector";

/// From AgentKnife to the preview page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum InspectorCommand {
    /// Highlight on hover; clicks select instead of acting.
    Enable,
    Disable,
}

/// From the preview page to AgentKnife.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InspectorEvent {
    /// The inspector is listening (sent once, when the page starts).
    Ready,
    Hover {
        target: Picked,
    },
    Select {
        target: Picked,
    },
    /// The user pressed Escape in the page.
    Cancel,
}

impl InspectorCommand {
    /// The string to `postMessage` to the page.
    pub fn message(self) -> String {
        serde_json::json!({ INSPECTOR_KEY: self }).to_string()
    }
}

impl InspectorEvent {
    /// Reads a message from the page; `None` for anything else.
    pub fn parse(message: &str) -> Option<InspectorEvent> {
        let v: serde_json::Value = serde_json::from_str(message).ok()?;
        serde_json::from_value(v.get(INSPECTOR_KEY)?.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_messages_round_trip() {
        assert_eq!(
            InspectorCommand::Enable.message(),
            r#"{"ak_inspector":{"cmd":"enable"}}"#
        );
        assert_eq!(
            InspectorEvent::parse(r#"{"ak_inspector":{"kind":"cancel"}}"#),
            Some(InspectorEvent::Cancel)
        );
        assert_eq!(InspectorEvent::parse(r#"{"other":1}"#), None);
        assert_eq!(InspectorEvent::parse("not json"), None);
    }

    #[test]
    fn readiness_and_errors() {
        let mut s = PreviewStatus {
            frontend: ServerState::Ready,
            ..PreviewStatus::default()
        };
        assert!(s.ready(), "no backend needed");
        s.backend = ServerState::Building;
        assert!(!s.ready());
        s.backend = ServerState::Error {
            output: "error[E0425]".into(),
        };
        assert_eq!(s.error(), Some("error[E0425]"));
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["frontend"]["state"], "ready");
        assert_eq!(json["backend"]["state"], "error");
    }
}
