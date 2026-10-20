//! Tools to look at and use the running app (P1-PREV-002): its build state, what
//! is on screen, clicking and typing, what the user pointed at, screenshots.

use std::sync::Arc;
use std::time::Duration;

use ak_preview::PreviewSession;
use ak_types::chat::{ContentPart, ToolSpec};
use ak_types::preview::{ControlAction, DomSummary, Picked, PreviewStatus, ServerState};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::{MAX_OUTPUT_CHARS, cap};
use crate::tool::{Tool, ToolContext, ToolOutput, ToolRegistry};

/// Tool names; the permission policy allows them (they never touch project files).
pub const PREVIEW_TOOLS: &[&str] = &[
    "build_status",
    "dom_summary",
    "interact",
    "get_selection",
    "screenshot_preview",
];

/// How long a tool waits for a running build before answering.
const BUILD_WAIT: Duration = Duration::from_secs(180);

/// What the agent needs from the running preview. `PreviewSession` implements
/// it; tests use fakes.
pub trait PreviewAccess: Send + Sync {
    fn status(&self) -> PreviewStatus;
    /// The state once nothing is building (or after `timeout`).
    fn settled(&self, timeout: Duration) -> BoxFuture<'_, PreviewStatus>;
    /// Project files changed (relative paths): the backend may need a restart.
    fn files_changed<'a>(&'a self, paths: &'a [String]) -> BoxFuture<'a, ()>;
    /// Runs an action in the preview page.
    fn act(&self, action: ControlAction) -> BoxFuture<'_, Result<Value, String>>;
    /// What the user last pointed at.
    fn selection(&self) -> Option<Picked>;
    fn can_capture(&self) -> bool;
    /// A screenshot (image content part) of the page or of one element.
    fn screenshot(&self, ak_id: Option<String>) -> BoxFuture<'_, Result<ContentPart, String>>;
}

impl PreviewAccess for PreviewSession {
    fn status(&self) -> PreviewStatus {
        PreviewSession::status(self)
    }

    fn settled(&self, timeout: Duration) -> BoxFuture<'_, PreviewStatus> {
        Box::pin(PreviewSession::settled(self, timeout))
    }

    fn files_changed<'a>(&'a self, paths: &'a [String]) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _ = PreviewSession::files_changed(self, paths).await;
        })
    }

    fn act(&self, action: ControlAction) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move { PreviewSession::act(self, &action).await })
    }

    fn selection(&self) -> Option<Picked> {
        PreviewSession::selection(self)
    }

    fn can_capture(&self) -> bool {
        PreviewSession::can_capture(self)
    }

    fn screenshot(&self, ak_id: Option<String>) -> BoxFuture<'_, Result<ContentPart, String>> {
        Box::pin(async move {
            let (image, _) = PreviewSession::screenshot(self, ak_id).await?;
            Ok(ContentPart::Image {
                media_type: image.media_type,
                data_base64: image.data_base64,
            })
        })
    }
}

/// The preview tools. `screenshot_preview` only for models that can see images
/// and when screenshots are available; otherwise the agent uses `dom_summary`.
pub fn with_preview_tools(
    registry: ToolRegistry,
    preview: Arc<dyn PreviewAccess>,
    vision: bool,
) -> ToolRegistry {
    let mut r = registry
        .with(Arc::new(BuildStatus(preview.clone())))
        .with(Arc::new(DomSummaryTool(preview.clone())))
        .with(Arc::new(Interact(preview.clone())))
        .with(Arc::new(GetSelection(preview.clone())));
    if vision && preview.can_capture() {
        r = r.with(Arc::new(Screenshot(preview)));
    }
    r
}

fn state_text(name: &str, s: &ServerState) -> String {
    match s {
        ServerState::Off => format!("{name}: not running"),
        ServerState::Starting | ServerState::Building => format!("{name}: still building"),
        ServerState::Ready => format!("{name}: ok"),
        ServerState::Error { .. } => format!("{name}: FAILED"),
    }
}

/// The build state in words, with the compiler output of a failure.
pub fn describe_status(s: &PreviewStatus) -> String {
    let mut text = format!(
        "{}\n{}\n",
        state_text("Screens", &s.frontend),
        state_text("Backend", &s.backend)
    );
    if let Some(err) = s.error() {
        text.push_str("\nCompiler output:\n");
        text.push_str(&cap(err, MAX_OUTPUT_CHARS));
    } else if s.ready() {
        text.push_str("The app builds and runs.");
    }
    text
}

fn not_open(s: &PreviewStatus) -> Option<ToolOutput> {
    if s.frontend.is_ready() || s.url.is_some() {
        return None;
    }
    Some(ToolOutput::error(format!(
        "The preview is not showing the app yet.\n{}",
        describe_status(s)
    )))
}

fn no_args_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

struct BuildStatus(Arc<dyn PreviewAccess>);

impl Tool for BuildStatus {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "build_status".into(),
            description: "Whether the app currently builds and runs in the preview, with the \
                          compiler errors if not. Waits for a running build to finish."
                .into(),
            input_schema: no_args_schema(),
        }
    }

    fn call<'a>(&'a self, _: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let s = self.0.settled(BUILD_WAIT).await;
            ToolOutput::text(describe_status(&s))
        })
    }
}

struct DomSummaryTool(Arc<dyn PreviewAccess>);

impl Tool for DomSummaryTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "dom_summary".into(),
            description: "What the app shows right now in the preview: its elements (with \
                          their data-ak-id), texts and field values, one per line. Use it \
                          to check a change worked."
                .into(),
            input_schema: no_args_schema(),
        }
    }

    fn call<'a>(&'a self, _: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let s = self.0.settled(BUILD_WAIT).await;
            if let Some(out) = not_open(&s) {
                return out;
            }
            match self.0.act(ControlAction::DomSummary).await {
                Ok(v) => match serde_json::from_value::<DomSummary>(v) {
                    Ok(summary) => ToolOutput::text(cap(&summary.render(), MAX_OUTPUT_CHARS)),
                    Err(e) => ToolOutput::error(format!("Unexpected answer from the page: {e}")),
                },
                Err(e) => ToolOutput::error(e),
            }
        })
    }
}

struct Interact(Arc<dyn PreviewAccess>);

impl Tool for Interact {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "interact".into(),
            description: "Use the running app in the preview, like the user would: click an \
                          element, type into a field (optionally pressing Enter), or read \
                          the app's saved state (localStorage and what the app reports). \
                          Elements are named by data-ak-id. Check the result with \
                          dom_summary afterwards."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["click", "type", "read_state"] },
                    "ak_id": { "type": "string", "description": "data-ak-id of the element (click, type)" },
                    "text": { "type": "string", "description": "What to type (replaces the field's text)" },
                    "enter": { "type": "boolean", "description": "Press Enter after typing" },
                },
                "required": ["action"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let ak_id = args["ak_id"].as_str().map(str::to_owned);
            let action = match (args["action"].as_str(), ak_id) {
                (Some("click"), Some(ak_id)) => ControlAction::Click { ak_id },
                (Some("type"), Some(ak_id)) => ControlAction::Type {
                    ak_id,
                    text: args["text"].as_str().unwrap_or_default().to_owned(),
                    enter: args["enter"].as_bool().unwrap_or(false),
                },
                (Some("read_state"), _) => ControlAction::AppState,
                (Some("click" | "type"), None) => {
                    return ToolOutput::error("`ak_id` is required for click and type.");
                }
                _ => return ToolOutput::error("`action` must be click, type or read_state."),
            };
            let s = self.0.settled(BUILD_WAIT).await;
            if let Some(out) = not_open(&s) {
                return out;
            }
            match self.0.act(action).await {
                Ok(v) => ToolOutput::text(cap(
                    &serde_json::to_string_pretty(&v).unwrap_or_default(),
                    MAX_OUTPUT_CHARS,
                )),
                Err(e) => ToolOutput::error(e),
            }
        })
    }
}

struct GetSelection(Arc<dyn PreviewAccess>);

impl Tool for GetSelection {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "get_selection".into(),
            description: "The element the user last pointed at in the preview (\"this \
                          button\"): its data-ak-id, tag, text and position. Search the \
                          code for the data-ak-id to find it."
                .into(),
            input_schema: no_args_schema(),
        }
    }

    fn call<'a>(&'a self, _: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        let out = match self.0.selection() {
            Some(p) => {
                let mut text = serde_json::to_string_pretty(&p).unwrap_or_default();
                if !p.exact {
                    text.push_str(
                        "\n(The user pointed inside this element; it has no data-ak-id of \
                         its own. Add one if you change it.)",
                    );
                }
                ToolOutput::text(text)
            }
            None => ToolOutput::text("The user has not pointed at anything in the preview."),
        };
        Box::pin(async move { out })
    }
}

struct Screenshot(Arc<dyn PreviewAccess>);

impl Tool for Screenshot {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "screenshot_preview".into(),
            description: "A picture of the app in the preview, to check how it looks. Give \
                          an ak_id to show one element (scrolled into view)."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "ak_id": { "type": "string", "description": "data-ak-id of an element (optional)" },
                },
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let s = self.0.settled(BUILD_WAIT).await;
            if let Some(out) = not_open(&s) {
                return out;
            }
            let ak_id = args["ak_id"].as_str().map(str::to_owned);
            match self.0.screenshot(ak_id).await {
                Ok(image) => ToolOutput {
                    content: vec![image],
                    is_error: false,
                    touched: Vec::new(),
                },
                Err(e) => ToolOutput::error(e),
            }
        })
    }
}
