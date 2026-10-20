//! What the preview frontend and the bridge server say to each other, and how
//! AgentKnife starts the backend. Shared by both sides; JSON text frames.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Port to bind on 127.0.0.1; `0` picks a free one.
pub const ENV_PORT: &str = "AK_BRIDGE_PORT";
/// The secret every connection must present as `?token=`.
pub const ENV_TOKEN: &str = "AK_BRIDGE_TOKEN";
/// When set, connections must come from this origin (the preview's dev server).
pub const ENV_ORIGIN: &str = "AK_BRIDGE_ORIGIN";
/// Printed on stdout once the server listens: `AK_BRIDGE_READY {"port":1234}`.
pub const READY_PREFIX: &str = "AK_BRIDGE_READY ";
/// Preview URL parameter with the full WebSocket URL (token included).
pub const URL_PARAM: &str = "ak_bridge";
/// Where the frontend keeps that URL for the rest of the session.
pub const STORAGE_KEY: &str = "ak_bridge";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Call a Tauri command, like `invoke(cmd, args)` in the real app.
    Invoke {
        id: u64,
        cmd: String,
        #[serde(default)]
        args: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The answer to the `Invoke` with the same id: the command's return value,
    /// or (`ok: false`) its error value.
    Result { id: u64, ok: bool, value: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ready {
    pub port: u16,
}

// ---- Inspector (point-and-select) ----
//
// AgentKnife shows the preview in an iframe and talks to it with `postMessage`.
// Every message is a JSON string `{"ak_inspector": …}`: an [`InspectorCommand`]
// from AgentKnife, an [`InspectorEvent`] from the preview.

/// The key that marks inspector messages.
pub const INSPECTOR_KEY: &str = "ak_inspector";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum InspectorCommand {
    /// Start highlighting on hover; clicks select instead of acting.
    Enable,
    Disable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InspectorEvent {
    /// The inspector is listening (sent once, on start).
    Ready,
    Hover {
        target: Picked,
    },
    Select {
        target: Picked,
    },
    /// The user pressed Escape.
    Cancel,
}

/// What the user pointed at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picked {
    /// `data-ak-id` of the element, or of its closest ancestor that has one.
    pub ak_id: Option<String>,
    /// Whether `ak_id` is the pointed element's own (not an ancestor's).
    pub exact: bool,
    /// CSS path of the highlighted element, e.g. `body > main > form > button:nth-of-type(2)`.
    pub path: String,
    /// Lower-case tag name of the highlighted element.
    pub tag: String,
    /// Its visible text, shortened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Its box in viewport CSS pixels (match it against a preview screenshot).
    pub rect: Rect,
    pub viewport: Size,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<CanvasPoint>,
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

/// A point on a `<canvas>`, in the canvas's own pixels (what drawing code uses).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasPoint {
    pub x: f64,
    pub y: f64,
    /// What the app's hit test said is there, if the app registered one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit: Option<Value>,
}

/// One step of a CSS path, as read from the DOM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStep {
    pub tag: String,
    pub id: Option<String>,
    /// 1-based position among same-tag siblings, when there is more than one.
    pub nth_of_type: Option<usize>,
}

/// `body > main > button:nth-of-type(2)`; stops early at an element with an id.
pub fn css_path(steps_from_root: &[PathStep]) -> String {
    let start = steps_from_root
        .iter()
        .rposition(|s| s.id.is_some())
        .unwrap_or(0);
    steps_from_root[start..]
        .iter()
        .map(|s| match (&s.id, s.nth_of_type) {
            (Some(id), _) => format!("{}#{id}", s.tag),
            (None, Some(n)) => format!("{}:nth-of-type({n})", s.tag),
            (None, None) => s.tag.clone(),
        })
        .collect::<Vec<_>>()
        .join(" > ")
}

/// Collapses whitespace and cuts to `max` characters (with an ellipsis).
pub fn short_text(text: &str, max: usize) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let joined = words.join(" ");
    if joined.chars().count() <= max {
        return Some(joined);
    }
    let cut: String = joined.chars().take(max.saturating_sub(1)).collect();
    Some(format!("{}…", cut.trim_end()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn step(tag: &str, id: Option<&str>, nth: Option<usize>) -> PathStep {
        PathStep {
            tag: tag.into(),
            id: id.map(Into::into),
            nth_of_type: nth,
        }
    }

    #[test]
    fn css_paths() {
        let steps = [
            step("body", None, None),
            step("div", Some("app"), None),
            step("form", None, None),
            step("button", None, Some(2)),
        ];
        assert_eq!(css_path(&steps), "div#app > form > button:nth-of-type(2)");
        assert_eq!(
            css_path(&[step("body", None, None), step("p", None, Some(3))]),
            "body > p:nth-of-type(3)"
        );
    }

    #[test]
    fn text_is_shortened() {
        assert_eq!(short_text("  \n ", 10), None);
        assert_eq!(short_text(" Add \n item ", 20).as_deref(), Some("Add item"));
        assert_eq!(
            short_text("buy milk and eggs", 10).as_deref(),
            Some("buy milk…")
        );
    }

    #[test]
    fn inspector_messages() {
        let cmd: InspectorCommand = serde_json::from_str(r#"{"cmd":"enable"}"#).unwrap();
        assert_eq!(cmd, InspectorCommand::Enable);
        let ev = InspectorEvent::Select {
            target: Picked {
                ak_id: Some("add-button".into()),
                exact: true,
                path: "form > button".into(),
                tag: "button".into(),
                text: Some("Add".into()),
                rect: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 80.0,
                    height: 32.0,
                },
                viewport: Size {
                    width: 390.0,
                    height: 844.0,
                },
                canvas: None,
            },
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["kind"], "select");
        assert_eq!(v["target"]["ak_id"], "add-button");
        assert!(v["target"].get("canvas").is_none());
        assert_eq!(serde_json::from_value::<InspectorEvent>(v).unwrap(), ev);
    }

    #[test]
    fn wire_format() {
        let m: ClientMessage = serde_json::from_str(
            r#"{"type":"invoke","id":7,"cmd":"add_todo","args":{"text":"milk"}}"#,
        )
        .unwrap();
        assert_eq!(
            m,
            ClientMessage::Invoke {
                id: 7,
                cmd: "add_todo".into(),
                args: json!({ "text": "milk" })
            }
        );
        let no_args: ClientMessage =
            serde_json::from_str(r#"{"type":"invoke","id":1,"cmd":"list"}"#).unwrap();
        assert!(matches!(no_args, ClientMessage::Invoke { args, .. } if args.is_null()));
        assert_eq!(
            serde_json::to_value(ServerMessage::Result {
                id: 7,
                ok: false,
                value: json!("empty text")
            })
            .unwrap(),
            json!({ "type": "result", "id": 7, "ok": false, "value": "empty text" })
        );
    }
}
