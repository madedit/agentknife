//! Tools the model can call.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use ak_types::chat::{ContentPart, ToolSpec};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// Argument every tool call carries: a short plain-language description of the step,
/// written by the model in the user's language and shown in the step list.
pub const LABEL_ARG: &str = "label";

pub struct ToolContext {
    pub project_dir: Option<PathBuf>,
    /// Cancelled when the user stops the turn; long-running tools should watch it.
    pub cancel: CancellationToken,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub content: Vec<ContentPart>,
    pub is_error: bool,
    /// Project-relative paths the tool changed (recorded in `turn.lock`).
    pub touched: Vec<String>,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentPart::Text { text: text.into() }],
            is_error: false,
            touched: Vec::new(),
        }
    }

    /// An error the model should read and react to (not a failure of the turn).
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::text(text)
        }
    }
}

pub trait Tool: Send + Sync {
    /// Name, description and JSON schema, without the `label` argument.
    fn spec(&self) -> ToolSpec;

    /// Runs the tool. `args` no longer contains `label`.
    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput>;
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.insert(tool.spec().name, tool);
        self
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Specs as sent to the model, in a stable order (a changing tool list would
    /// break prompt caching and reasoning replay), each with the `label` argument.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| with_label(t.spec())).collect()
    }
}

fn with_label(mut spec: ToolSpec) -> ToolSpec {
    if !spec.input_schema.is_object() {
        spec.input_schema = json!({ "type": "object" });
    }
    let schema = &mut spec.input_schema;
    if !schema["properties"].is_object() {
        schema["properties"] = json!({});
    }
    schema["properties"][LABEL_ARG] = json!({
        "type": "string",
        "description": "A few plain words, in the user's language, saying what this step \
                        does for someone who does not program, e.g. \"Reading the login page\".",
    });
    let mut required = vec![json!(LABEL_ARG)];
    if let Some(existing) = schema["required"].as_array() {
        required.extend(existing.iter().filter(|r| *r != LABEL_ARG).cloned());
    }
    schema["required"] = Value::Array(required);
    spec
}

/// Splits the label off a call's arguments.
pub fn split_label(mut args: Value) -> (Option<String>, Value) {
    let label = args
        .as_object_mut()
        .and_then(|o| o.remove(LABEL_ARG))
        .and_then(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });
    (label, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_injected_first_and_required() {
        let spec = with_label(ToolSpec {
            name: "read_file".into(),
            description: "Read".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"],
            }),
        });
        assert_eq!(spec.input_schema["required"], json!(["label", "path"]));
        assert_eq!(spec.input_schema["properties"]["path"]["type"], "string");
        assert_eq!(spec.input_schema["properties"]["label"]["type"], "string");
        let bare = with_label(ToolSpec {
            name: "t".into(),
            description: String::new(),
            input_schema: Value::Null,
        });
        assert_eq!(bare.input_schema["required"], json!(["label"]));
    }

    #[test]
    fn split_label_removes_it() {
        let (label, args) = split_label(json!({ "label": " Reading app.rs ", "path": "a" }));
        assert_eq!(label.as_deref(), Some("Reading app.rs"));
        assert_eq!(args, json!({ "path": "a" }));
        assert_eq!(split_label(json!({ "label": "" })).0, None);
        assert_eq!(split_label(json!(null)).0, None);
    }
}
