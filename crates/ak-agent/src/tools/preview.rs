//! Tools to look at and use the running app (P1-PREV-002): its build state, what
//! is on screen, clicking and typing, what the user pointed at, screenshots.

use std::sync::Arc;
use std::time::Duration;

use ak_preview::PreviewSession;
use ak_types::chat::{ContentPart, ToolSpec};
use ak_types::preview::{
    ControlAction, DomSummary, MAX_HOLD_MS, PAGE_CLOSED, PAGE_RELOADED_STUCK, PAGE_TIMEOUT, Picked,
    PreviewStatus, ServerState,
};
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
    /// The backend's crash reports since it last started, oldest first.
    fn backend_crashes(&self) -> Vec<String> {
        Vec::new()
    }
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

    fn backend_crashes(&self) -> Vec<String> {
        PreviewSession::backend_crashes(self)
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
        text.push_str(&cap(&first_errors(err, SHOWN_ERRORS), MAX_OUTPUT_CHARS));
    } else if s.ready() {
        text.push_str(
            "The app builds and runs. (This does not tell whether the page responds: \
             dom_summary does.)",
        );
    }
    text
}

/// Compiler errors shown in full; the rest are only counted.
const SHOWN_ERRORS: usize = 2;

/// The first `keep` compiler errors of `output` and how many more there are, or
/// `output` as it is when it has no more than that.
fn first_errors(output: &str, keep: usize) -> String {
    let starts = |l: &str| {
        (l.starts_with("error[") || l.starts_with("error:"))
            && !l.starts_with("error: could not compile")
            && !l.starts_with("error: aborting")
    };
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    for line in output.lines() {
        if starts(line) {
            blocks.push(vec![line]);
            open = true;
        } else if open && !line.trim().is_empty() && !line.starts_with("warning") {
            if let Some(b) = blocks.last_mut() {
                b.push(line);
            }
        } else {
            open = false;
        }
    }
    if blocks.len() <= keep {
        return output.to_owned();
    }
    let shown: Vec<String> = blocks[..keep].iter().map(|b| b.join("\n")).collect();
    // The others in one line each: what and where.
    let rest: Vec<String> = blocks[keep..]
        .iter()
        .take(LISTED_ERRORS)
        .map(|b| {
            let at = b.iter().find_map(|l| l.trim_start().strip_prefix("--> "));
            match at {
                Some(at) => format!("- {} ({at})", b[0]),
                None => format!("- {}", b[0]),
            }
        })
        .collect();
    let unlisted = (blocks.len() - keep).saturating_sub(LISTED_ERRORS);
    format!(
        "{}\n\n… and {} more errors:\n{}{}\nFix the first ones (later errors often \
         follow from them), then call build_status again.",
        shown.join("\n\n"),
        blocks.len() - keep,
        rest.join("\n"),
        if unlisted > 0 {
            format!("\n- … {unlisted} more")
        } else {
            String::new()
        }
    )
}

/// Errors after the shown ones that are listed by their first line and place.
const LISTED_ERRORS: usize = 10;

/// The backend's crashes, for the agent ("" when there are none).
pub fn describe_crashes(crashes: &[String]) -> String {
    if crashes.is_empty() {
        return String::new();
    }
    let mut text = "\nThe backend crashed while running (Rust panic; the file and line \
                    are in each report). The command that crashed failed on the page. \
                    Fix the cause, often an `unwrap()`, `expect()` or an index out of \
                    range:\n"
        .to_owned();
    for c in crashes {
        text.push_str(&format!("- {}\n", c.replace('\n', "\n  ")));
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
            ToolOutput::text(describe_status(&s) + &describe_crashes(&self.0.backend_crashes()))
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
                    Ok(summary) => ToolOutput::text(
                        cap(&summary.render(), MAX_OUTPUT_CHARS)
                            + &describe_crashes(&self.0.backend_crashes()),
                    ),
                    Err(e) => ToolOutput::error(format!("Unexpected answer from the page: {e}")),
                },
                Err(e) => {
                    ToolOutput::error(page_error(e) + &describe_crashes(&self.0.backend_crashes()))
                }
            }
        })
    }
}

struct Interact(Arc<dyn PreviewAccess>);

impl Tool for Interact {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "interact".into(),
            description: "Use the running app in the preview, like the user would. Elements \
                          are named by data-ak-id. `click`: pointer and mouse down and up, \
                          then click, like a mouse. `hold`: press an element for `ms` then \
                          let go (buttons that act while held). `key`: press a key on the \
                          focused element or the page (`key` like \"ArrowLeft\", \" \", \"a\"), \
                          held for `ms`. `type`: replace a field's text (optionally pressing \
                          Enter). `scroll`: bring `ak_id` into view, or with `by` scroll the \
                          content of `ak_id` (a scrolling list; without `ak_id` the page) by \
                          that many pixels (negative: up); it answers where it is now. \
                          `drag`: drag `ak_id` onto the element `to` (HTML5 drag and drop \
                          for draggable elements, else pointer down, move, up); it answers \
                          whether the target accepted the drop. \
                          `read_state`: the app's saved state (localStorage and what \
                          the app reports with set_state_reader); with `wait_ms` it waits \
                          first, with `samples` it reads several times spread over `wait_ms` \
                          (to watch a game run: no need to sleep with run_command). Check \
                          the result with dom_summary or read_state afterwards."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["click", "hold", "key", "type", "scroll", "drag", "read_state"] },
                    "ak_id": { "type": "string", "description": "data-ak-id of the element (click, hold, type, drag: what is dragged; scroll: optional)" },
                    "to": { "type": "string", "description": "drag: data-ak-id of the element to drop on" },
                    "by": { "type": "integer", "description": "scroll: pixels to scroll down (negative: up)" },
                    "text": { "type": "string", "description": "What to type (replaces the field's text)" },
                    "enter": { "type": "boolean", "description": "Press Enter after typing" },
                    "key": { "type": "string", "description": "The key to press (key), as KeyboardEvent.key: \"ArrowLeft\", \"a\", \"Enter\"; space is \" \" or \"Space\"" },
                    "ms": { "type": "integer", "description": "How long to hold (hold: default 1000; key: default 0, a tap), at most 10000" },
                    "wait_ms": { "type": "integer", "description": "read_state: wait this long first, at most 10000" },
                    "samples": { "type": "integer", "description": "read_state: read this many times (2 to 10), the first now and the last after wait_ms" },
                },
                "required": ["action"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, _: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let ak_id = args["ak_id"].as_str().map(str::to_owned);
            let ms = |default: u32| {
                args["ms"]
                    .as_u64()
                    .map_or(default, |n| u32::try_from(n).unwrap_or(u32::MAX))
                    .min(MAX_HOLD_MS)
            };
            let action = match (args["action"].as_str(), ak_id) {
                (Some("click"), Some(ak_id)) => ControlAction::Click { ak_id },
                (Some("hold"), Some(ak_id)) => ControlAction::Hold {
                    ak_id,
                    ms: ms(1000),
                },
                (Some("key"), _) => match args["key"].as_str().and_then(key_name) {
                    Some(key) => ControlAction::Key { key, ms: ms(0) },
                    None => return ToolOutput::error("`key` is required for key."),
                },
                (Some("type"), Some(ak_id)) => ControlAction::Type {
                    ak_id,
                    text: args["text"].as_str().unwrap_or_default().to_owned(),
                    enter: args["enter"].as_bool().unwrap_or(false),
                },
                (Some("scroll"), ak_id) => {
                    let by = args["by"]
                        .as_i64()
                        .map(|n| n.clamp(-100_000, 100_000) as i32);
                    if ak_id.is_none() && by.is_none() {
                        return ToolOutput::error("scroll needs `ak_id`, `by` or both.");
                    }
                    ControlAction::Scroll { ak_id, by }
                }
                (Some("drag"), Some(ak_id)) => match args["to"].as_str() {
                    Some(to) => ControlAction::Drag {
                        ak_id,
                        to: to.to_owned(),
                    },
                    None => {
                        return ToolOutput::error(
                            "drag needs `to`: the data-ak-id of the element to drop on.",
                        );
                    }
                },
                (Some("read_state"), _) => ControlAction::AppState,
                (Some("click" | "hold" | "type" | "drag"), None) => {
                    return ToolOutput::error(
                        "`ak_id` is required for click, hold, type and drag.",
                    );
                }
                _ => {
                    return ToolOutput::error(
                        "`action` must be click, hold, key, type, scroll, drag or read_state.",
                    );
                }
            };
            let s = self.0.settled(BUILD_WAIT).await;
            if let Some(out) = not_open(&s) {
                return out;
            }
            let shown = |v: &Value| {
                ToolOutput::text(cap(
                    &serde_json::to_string_pretty(v).unwrap_or_default(),
                    MAX_OUTPUT_CHARS,
                ))
            };
            if action == ControlAction::AppState {
                let wait = args["wait_ms"]
                    .as_u64()
                    .map_or(0, |n| n.min(u64::from(MAX_HOLD_MS)));
                let samples = args["samples"].as_u64().map_or(1, |n| n.clamp(1, 10));
                return match read_over_time(&*self.0, wait, samples).await {
                    Ok(v) => shown(&v),
                    Err(e) => ToolOutput::error(page_error(e)),
                };
            }
            match self.0.act(action).await {
                Ok(v) => shown(&v),
                Err(e) => ToolOutput::error(page_error(e)),
            }
        })
    }
}

/// The app's state after `wait` ms; with several `samples`, a list of
/// `{ after_ms, state }` spread evenly from now to `wait`.
async fn read_over_time(
    preview: &dyn PreviewAccess,
    wait: u64,
    samples: u64,
) -> Result<Value, String> {
    if samples <= 1 {
        tokio::time::sleep(Duration::from_millis(wait)).await;
        return preview.act(ControlAction::AppState).await;
    }
    let step = wait / (samples - 1);
    let mut out = Vec::new();
    for i in 0..samples {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(step)).await;
        }
        let state = preview.act(ControlAction::AppState).await?;
        out.push(json!({ "after_ms": i * step, "state": state }));
    }
    Ok(Value::Array(out))
}

/// `KeyboardEvent.key` for what the model wrote: quotes it added around the
/// key are dropped (`"\" \""` is a space), and `Space` is the space key.
fn key_name(raw: &str) -> Option<String> {
    let k = match raw.chars().count() {
        3.. if (raw.starts_with('"') && raw.ends_with('"'))
            || (raw.starts_with('\'') && raw.ends_with('\'')) =>
        {
            &raw[1..raw.len() - 1]
        }
        _ => raw,
    };
    match k {
        "" => None,
        "Space" | "space" | "Spacebar" => Some(" ".into()),
        k => Some(k.to_owned()),
    }
}

/// The page reloads after each rebuild; an action caught by that may or may
/// not have happened, so say what to do instead of the bare bridge answer.
fn page_error(e: String) -> String {
    if e == PAGE_CLOSED {
        "The preview page reloaded (the app was rebuilt) before this finished, so it may \
         not have happened. Call dom_summary or read_state to see the page now, then try \
         again."
            .into()
    } else if e == PAGE_RELOADED_STUCK || e == PAGE_TIMEOUT {
        let after = if e == PAGE_RELOADED_STUCK {
            "AgentKnife reloaded the page, so the app started over"
        } else {
            "Reload the preview to continue"
        };
        format!(
            "The preview page did not answer for 15 seconds: the screens are probably stuck \
             in an endless loop (a `loop`/`while` that never ends, or a signal update that \
             triggers itself). A rebuild cannot reach a stuck page. {after}. Find and fix \
             the loop before testing again."
        )
    } else if e.starts_with("no element with data-ak-id") {
        // Usually a fresh page after a rebuild, which starts the app over.
        format!(
            "{e}. If you just changed a file, the page reloaded and the app started over \
             (a game is back at its start screen). Call dom_summary to see the page now."
        )
    } else {
        e
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

#[cfg(test)]
mod tests {
    use super::first_errors;

    #[test]
    fn only_the_first_errors_are_shown() {
        let one = "error[E0425]: cannot find value `x`\n --> src/main.rs:3:5\n  |\n\n";
        let output = format!(
            "   Compiling app-ui v0.1.0\n{one}{one}warning: unused\n --> a\n\n{one}\
             error: could not compile `app-ui` due to 3 previous errors\n"
        );
        let short = first_errors(&output, 2);
        assert_eq!(
            short.matches(" --> src/main.rs:3:5\n").count(),
            2,
            "{short}"
        );
        assert!(short.contains("src/main.rs:3:5") && short.contains("… and 1 more errors"));
        // The rest: one line each, with the place.
        assert!(
            short.contains("- error[E0425]: cannot find value `x` (src/main.rs:3:5)"),
            "{short}"
        );
        assert!(!short.contains("warning") && !short.contains("Compiling"));
        assert_eq!(first_errors(one, 2), one, "few errors: the output as it is");
    }
}
