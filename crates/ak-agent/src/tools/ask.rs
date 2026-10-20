//! ask_user: a multiple-choice question for the user.

use std::sync::Arc;

use ak_types::agent::AgentEvent;
use ak_types::chat::ToolSpec;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::str_arg;
use crate::new_id;
use crate::tool::{Tool, ToolContext, ToolOutput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub request_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub allow_other: bool,
}

/// Shows a question in the UI and resolves with the answer (`None` = dismissed).
pub trait Asker: Send + Sync {
    fn ask(&self, question: Question) -> BoxFuture<'_, Option<String>>;
}

pub struct AskUser {
    asker: Arc<dyn Asker>,
}

impl AskUser {
    pub fn new(asker: Arc<dyn Asker>) -> Self {
        Self { asker }
    }
}

impl Tool for AskUser {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ask_user".into(),
            description: "Ask the user to choose between options when a decision is theirs \
                          (style, wording, which feature first). Keep it short and in plain \
                          language; don't ask about things you can decide or look up yourself."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "options": { "type": "array", "items": { "type": "string" },
                                 "description": "2 to 6 short choices; put the recommended one first" },
                    "allow_other": { "type": "boolean", "description": "Let the user type their own answer. Default true." },
                },
                "required": ["question", "options"],
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let question = match str_arg(&args, "question") {
                Ok(q) => q.to_owned(),
                Err(e) => return e,
            };
            let options: Vec<String> = args["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    o.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .collect();
            if !(2..=6).contains(&options.len()) {
                return ToolOutput::error("Give 2 to 6 options.");
            }
            let q = Question {
                request_id: new_id("ask"),
                question,
                options,
                allow_other: args["allow_other"].as_bool().unwrap_or(true),
            };
            if let Some(events) = &ctx.events {
                let _ = events.send(AgentEvent::QuestionAsked {
                    request_id: q.request_id.clone(),
                    question: q.question.clone(),
                    options: q.options.clone(),
                    allow_other: q.allow_other,
                });
            }
            let answer = tokio::select! {
                a = self.asker.ask(q) => a,
                _ = ctx.cancel.cancelled() => return ToolOutput::error("Cancelled by the user."),
            };
            match answer {
                Some(a) => ToolOutput::text(format!("The user answered: {a}")),
                None => ToolOutput::error("The user closed the question without answering."),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Pick(Option<&'static str>, Mutex<Vec<Question>>);

    impl Asker for Pick {
        fn ask(&self, q: Question) -> BoxFuture<'_, Option<String>> {
            self.1.lock().unwrap().push(q);
            let a = self.0.map(str::to_owned);
            Box::pin(async move { a })
        }
    }

    #[tokio::test]
    async fn asks_emits_event_and_returns_answer() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = ToolContext {
            events: Some(tx),
            ..ToolContext::for_project(".")
        };
        let asker = Arc::new(Pick(Some("Blue"), Mutex::new(Vec::new())));
        let tool = AskUser::new(asker.clone());
        let out = tool
            .call(
                json!({ "question": "Which color?", "options": ["Blue", "Green"] }),
                &ctx,
            )
            .await;
        assert_eq!(out, ToolOutput::text("The user answered: Blue"));
        assert!(
            matches!(rx.recv().await, Some(AgentEvent::QuestionAsked { options, allow_other: true, .. }) if options == ["Blue", "Green"])
        );
        assert_eq!(asker.1.lock().unwrap()[0].question, "Which color?");

        let dismissed = AskUser::new(Arc::new(Pick(None, Mutex::new(Vec::new()))));
        assert!(
            dismissed
                .call(json!({ "question": "?", "options": ["a", "b"] }), &ctx)
                .await
                .is_error
        );
        assert!(
            tool.call(json!({ "question": "?", "options": ["only one"] }), &ctx)
                .await
                .is_error
        );
    }
}
