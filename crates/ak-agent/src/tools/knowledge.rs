//! `read_knowledge`: the stack's docs, written for exactly the versions the
//! project uses (the model's memory may be of other versions).

use ak_types::chat::ToolSpec;
use ak_types::stack::KnowledgePack;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use super::{MAX_OUTPUT_CHARS, cap};
use crate::tool::{Tool, ToolContext, ToolOutput};

pub const READ_KNOWLEDGE: &str = "read_knowledge";

/// At most this many lines in a search's answer.
const MAX_MATCHES: usize = 40;

pub struct ReadKnowledge {
    pack: KnowledgePack,
}

impl ReadKnowledge {
    pub fn new(pack: KnowledgePack) -> Self {
        Self { pack }
    }

    fn index(&self) -> String {
        let mut s = format!(
            "Topics (knowledge pack {} {}):\n",
            self.pack.id, self.pack.version
        );
        for t in self.pack.topics {
            s.push_str(&format!("- {}: {} — {}\n", t.id, t.title, t.summary));
        }
        s
    }

    /// Every line that has all the words of `query` (any case), with its topic
    /// and the heading above it.
    fn search(&self, query: &str) -> String {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut found = Vec::new();
        for t in self.pack.topics {
            let mut heading = "";
            for line in t.body.lines() {
                let text = line.trim();
                if text.starts_with('#') {
                    heading = text.trim_start_matches('#').trim();
                }
                let lower = text.to_lowercase();
                if !text.is_empty() && words.iter().all(|w| lower.contains(w.as_str())) {
                    found.push(format!("- {} › {heading}: {text}", t.id));
                }
            }
        }
        if found.is_empty() {
            return format!(
                "Nothing in the knowledge pack has `{query}`. Try another word, or pick a topic:\n{}",
                self.index()
            );
        }
        let total = found.len();
        found.truncate(MAX_MATCHES);
        let mut s = format!("Lines with `{query}` (read the topic for the whole section):\n");
        s.push_str(&found.join("\n"));
        if total > MAX_MATCHES {
            s.push_str(&format!(
                "\n… and {} more: add a word to narrow it down.",
                total - MAX_MATCHES
            ));
        }
        s
    }
}

impl Tool for ReadKnowledge {
    fn spec(&self) -> ToolSpec {
        let ids: Vec<&str> = self.pack.topics.iter().map(|t| t.id).collect();
        ToolSpec {
            name: READ_KNOWLEDGE.into(),
            description: format!(
                "Read how-to docs for this project's stack, written for the exact versions \
                 it uses. Before writing or changing app code, read the one or two topics the \
                 change needs (not all of them); read `troubleshooting` when a build error is \
                 unclear. Not sure which topic covers something: `search` with a word or two \
                 lists the lines that have them, with their topics.\n{}",
                self.index()
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "enum": ids,
                        "description": "Topic id from the list above",
                    },
                    "search": {
                        "type": "string",
                        "description": "Instead of a topic: words to look for in every topic (e.g. `dark mode`)",
                    },
                },
            }),
        }
    }

    fn call<'a>(&'a self, args: Value, _ctx: &'a ToolContext) -> BoxFuture<'a, ToolOutput> {
        let query = args["search"].as_str().unwrap_or_default().trim();
        if !query.is_empty() {
            let out = ToolOutput::text(cap(&self.search(query), MAX_OUTPUT_CHARS));
            return Box::pin(async move { out });
        }
        let topic = args["topic"].as_str().unwrap_or_default().trim().to_owned();
        let out = match self.pack.topic(&topic) {
            Some(t) => ToolOutput::text(cap(t.body, MAX_OUTPUT_CHARS)),
            None => ToolOutput::error(format!("There is no topic `{topic}`. {}", self.index())),
        };
        Box::pin(async move { out })
    }
}

#[cfg(test)]
mod tests {
    use ak_types::chat::ContentPart;
    use ak_types::stack::KnowledgeTopic;

    use super::*;

    static TOPICS: &[KnowledgeTopic] = &[
        KnowledgeTopic {
            id: "ak-ui",
            title: "ak-ui components",
            summary: "Buttons and lists.",
            body: "# ak-ui\nUse Button.",
        },
        KnowledgeTopic {
            id: "storage",
            title: "Saving data",
            summary: "JSON files.",
            body: "# Saving data\n## Settings\nThe theme (Dark mode) in localStorage.\nA JSON file.",
        },
    ];

    fn tool() -> ReadKnowledge {
        ReadKnowledge::new(KnowledgePack {
            id: "stack",
            version: "1.0.0",
            topics: TOPICS,
            libraries: &[],
        })
    }

    fn text(out: &ToolOutput) -> &str {
        match &out.content[0] {
            ContentPart::Text { text } => text,
            ContentPart::Image { .. } => "",
        }
    }

    #[tokio::test]
    async fn reads_a_topic_or_lists_them() {
        let t = tool();
        let spec = t.spec();
        assert!(
            spec.description
                .contains("- storage: Saving data — JSON files.")
        );
        assert_eq!(
            spec.input_schema["properties"]["topic"]["enum"],
            json!(["ak-ui", "storage"])
        );

        let ctx = ToolContext::for_project(".");
        let out = t.call(json!({ "topic": "ak-ui" }), &ctx).await;
        assert!(!out.is_error);
        assert_eq!(text(&out), "# ak-ui\nUse Button.");
        assert!(!t.changes_project());

        let out = t.call(json!({ "topic": "leptos-router" }), &ctx).await;
        assert!(out.is_error);
        assert!(text(&out).contains("no topic `leptos-router`"));
        assert!(
            text(&out).contains("- ak-ui:"),
            "the index helps the model pick"
        );
    }

    #[tokio::test]
    async fn searches_every_topic() {
        let t = tool();
        assert!(t.spec().input_schema["properties"]["search"].is_object());
        let ctx = ToolContext::for_project(".");
        // Every word, any case, with the topic and the heading above the line.
        let out = t.call(json!({ "search": "dark THEME" }), &ctx).await;
        assert!(!out.is_error);
        assert!(
            text(&out).contains("- storage › Settings: The theme (Dark mode) in localStorage."),
            "{}",
            text(&out)
        );
        assert!(!text(&out).contains("A JSON file"));
        // A search wins over a topic; nothing found lists the topics.
        let out = t
            .call(json!({ "search": "router", "topic": "ak-ui" }), &ctx)
            .await;
        assert!(text(&out).contains("Nothing in the knowledge pack has `router`"));
        assert!(text(&out).contains("- ak-ui:"));
    }
}
