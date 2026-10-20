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

/// Control answers when no page could run the action: none is open (never sent),
/// or the page went away before answering (e.g. it reloaded after a rebuild).
pub const PAGE_NOT_OPEN: &str = "the preview page is not open";
pub const PAGE_CLOSED: &str = "the preview page closed";

/// Query parameter that marks a connection as AgentKnife's controller (the
/// agent), not the preview page: `?token=…&role=controller`.
pub const CONTROLLER_ROLE: &str = "role=controller";

/// From the preview page or a controller.
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
    /// Controller only: act on the open preview page.
    Control { id: u64, action: ControlAction },
    /// Page only: the outcome of a [`ServerMessage::Control`].
    ControlResult { id: u64, ok: bool, value: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The answer to the `Invoke` with the same id: the command's return value,
    /// or (`ok: false`) its error value.
    Result { id: u64, ok: bool, value: Value },
    /// To the page: run this action (ids are the server's own).
    Control { id: u64, action: ControlAction },
    /// To the controller: the page's answer to its `Control` with this id.
    ControlResult { id: u64, ok: bool, value: Value },
}

// ---- Control channel (the agent checks its work) ----

/// What the agent can do in the preview page. Answers: `click` and `type` give
/// the element's tag; `dom_summary` a [`DomSummary`]; `app_state` an
/// [`AppState`]; `prepare_screenshot` a [`ScreenshotArea`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ControlAction {
    /// Click the element with this `data-ak-id` (scrolled into view first).
    Click {
        ak_id: String,
    },
    /// Replace the text of an input, textarea or editable element, firing
    /// `input` and `change`; `enter` then presses Enter (submitting its form).
    Type {
        ak_id: String,
        text: String,
        #[serde(default)]
        enter: bool,
    },
    DomSummary,
    AppState,
    /// Scroll `ak_id` (or nothing) into view, let the page paint, and report the
    /// area to capture. AgentKnife takes the pixels itself.
    PrepareScreenshot {
        #[serde(default)]
        ak_id: Option<String>,
    },
}

/// One element worth telling the agent about, in document order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomItem {
    /// Nesting among the reported items (not raw DOM depth).
    pub depth: usize,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ak_id: Option<String>,
    /// `role`, input `type`, link `href`, and similar hints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Current value of form fields.
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
    /// Items left out to stay within the size limit.
    #[serde(default)]
    pub omitted: usize,
}

/// The summary as compact text for a model, e.g.
/// `button #add-button "Add item"` / `  input #todo-input type=text value="milk"`.
pub fn render_dom_summary(s: &DomSummary) -> String {
    let mut out = format!("Page: {} ({})\n", s.title, s.url);
    for item in &s.items {
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
    if s.omitted > 0 {
        out.push_str(&format!("({} more elements not shown)\n", s.omitted));
    }
    out
}

/// `url` without the `ak_bridge` parameter: it carries the bridge token, which
/// must not reach the model through page reports.
pub fn strip_bridge_param(url: &str) -> String {
    let (base, fragment) = match url.split_once('#') {
        Some((b, f)) => (b, Some(f)),
        None => (url, None),
    };
    let mut out = match base.split_once('?') {
        Some((path, query)) => {
            let kept: Vec<&str> = query
                .split('&')
                .filter(|kv| kv.split('=').next() != Some(URL_PARAM) && !kv.is_empty())
                .collect();
            if kept.is_empty() {
                path.to_owned()
            } else {
                format!("{path}?{}", kept.join("&"))
            }
        }
        None => base.to_owned(),
    };
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    pub url: String,
    pub title: String,
    /// The page's localStorage (where simple apps keep their data).
    pub local_storage: serde_json::Map<String, Value>,
    /// What the app reports via `ak_bridge::set_state_reader`, if it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<Value>,
}

/// Where to capture, in viewport CSS pixels; multiply by `device_pixel_ratio`
/// for screen pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotArea {
    pub rect: Rect,
    pub viewport: Size,
    pub device_pixel_ratio: f64,
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
    fn dom_summary_renders_compactly() {
        let item = |depth, tag: &str, ak_id: Option<&str>| DomItem {
            depth,
            tag: tag.into(),
            ak_id: ak_id.map(Into::into),
            attrs: vec![],
            text: None,
            value: None,
            checked: None,
            disabled: false,
        };
        let summary = DomSummary {
            title: "Todos".into(),
            url: "http://127.0.0.1:8080/".into(),
            items: vec![
                DomItem {
                    text: Some("Todo".into()),
                    ..item(0, "h1", Some("title"))
                },
                item(0, "form", Some("add-form")),
                DomItem {
                    attrs: vec![("type".into(), "text".into())],
                    value: Some("milk".into()),
                    ..item(1, "input", Some("todo-input"))
                },
                DomItem {
                    text: Some("Add".into()),
                    disabled: true,
                    ..item(1, "button", Some("add-button"))
                },
                DomItem {
                    checked: Some(true),
                    ..item(1, "input", None)
                },
            ],
            omitted: 3,
        };
        assert_eq!(
            render_dom_summary(&summary),
            "Page: Todos (http://127.0.0.1:8080/)\n\
             h1 #title \"Todo\"\n\
             form #add-form\n\
             \x20 input #todo-input type=text value=\"milk\"\n\
             \x20 button #add-button disabled \"Add\"\n\
             \x20 input checked\n\
             (3 more elements not shown)\n"
        );
    }

    #[test]
    fn bridge_token_never_leaves_in_urls() {
        let u = "http://127.0.0.1:8080/index.html?ak_bridge=ws%3A%2F%2Fx%3Ftoken%3Dsecret";
        assert_eq!(strip_bridge_param(u), "http://127.0.0.1:8080/index.html");
        assert_eq!(
            strip_bridge_param("http://h/?a=1&ak_bridge=s&b=2#/todos"),
            "http://h/?a=1&b=2#/todos"
        );
        assert_eq!(strip_bridge_param("http://h/#/x"), "http://h/#/x");
        assert_eq!(
            strip_bridge_param("http://h/?ak_bridgex=1"),
            "http://h/?ak_bridgex=1"
        );
    }

    #[test]
    fn control_messages() {
        let m: ClientMessage = serde_json::from_str(
            r#"{"type":"control","id":4,"action":{"action":"type","ak_id":"todo-input","text":"milk"}}"#,
        )
        .unwrap();
        assert_eq!(
            m,
            ClientMessage::Control {
                id: 4,
                action: ControlAction::Type {
                    ak_id: "todo-input".into(),
                    text: "milk".into(),
                    enter: false
                }
            }
        );
        let shot: ControlAction =
            serde_json::from_str(r#"{"action":"prepare_screenshot"}"#).unwrap();
        assert_eq!(shot, ControlAction::PrepareScreenshot { ak_id: None });
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
