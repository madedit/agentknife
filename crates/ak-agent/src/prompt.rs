//! The system prompt: built once per agent and never changed mid-conversation
//! (providers bind reasoning signatures and the prompt cache to it).

use ak_types::chat::ToolSpec;
use ak_types::i18n::Locale;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// When the agent must show a plan before changing the project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanMode {
    /// The AI decides: plans for big changes, small edits go straight ahead.
    #[default]
    Auto,
    /// A plan must be approved before any change in a turn.
    AlwaysAsk,
    /// Never propose plans.
    NeverAsk,
}

/// Loop-owned tool for plan-before-big-change.
pub const PLAN_TOOL: &str = "propose_plan";

pub fn plan_tool_spec() -> ToolSpec {
    ToolSpec {
        name: PLAN_TOOL.into(),
        description: "Show the user a short plan and wait for their answer before a big \
                      change. The answer comes back as this tool's result."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "What will be built, in a few words" },
                "steps": { "type": "array", "items": { "type": "string" },
                           "description": "3 to 5 short steps in plain language" },
                "estimate": { "type": "string", "description": "Rough time, e.g. \"about 5 minutes\"" },
            },
            "required": ["title", "steps"],
        }),
    }
}

fn language_name(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "English",
        Locale::ZhTw => "Traditional Chinese (繁體中文)",
    }
}

/// The product instructions, versioned in `prompts/system.md`.
const PRODUCT_PROMPT: &str = include_str!("../prompts/system.md");
const VERSION_TAG: &str = "<!-- prompt-version: ";

/// The product instructions to pass as `AgentConfig::system` (without the version
/// header).
pub fn product_prompt() -> &'static str {
    PRODUCT_PROMPT
        .split_once('\n')
        .filter(|(first, _)| first.starts_with(VERSION_TAG))
        .map_or(PRODUCT_PROMPT, |(_, rest)| rest)
        .trim()
}

/// `prompt-version` of `prompts/system.md`; bump it whenever the prompt changes, so
/// behaviour changes can be traced to a prompt version.
pub fn product_prompt_version() -> u32 {
    PRODUCT_PROMPT
        .strip_prefix(VERSION_TAG)
        .and_then(|r| r.split_once(' '))
        .and_then(|(v, _)| v.parse().ok())
        .unwrap_or(0)
}

/// What the agent is asked to say when a user first connects an AI.
pub fn greeting_note(ui_locale: Locale) -> String {
    format!(
        "The user just connected AgentKnife to an AI and has not written anything yet. \
         Write in {lang}. Greet them warmly in one sentence. Then explain in 3 or 4 short \
         points what AgentKnife can do for them: build a desktop app from a description, \
         show it live in the preview while you work, let them point at things to change, \
         and package it at the end so they can install and share it. Then suggest the \
         first step: checking that the development tools are installed on this computer \
         (AgentKnife does it for them and asks before installing anything). End by asking \
         whether to start that check. Keep it short and friendly; do not call any tools.",
        lang = language_name(ui_locale)
    )
}

/// `base` (product instructions, P1-AGENT-007) followed by the behaviour rules.
pub fn system_prompt(base: Option<&str>, ui_locale: Locale, plan_mode: PlanMode) -> String {
    let mut s = String::new();
    if let Some(b) = base.map(str::trim).filter(|b| !b.is_empty()) {
        s.push_str(b);
        s.push_str("\n\n");
    }
    // First and explicit: small models drift to the conversation's earlier language.
    s.push_str(&format!(
        "## Language (most important rule)\n\
         Before you write anything, look at the user's most recent message and use its \
         language for everything you write: replies, step `label`s, plans and questions. \
         Do this even if earlier messages used another language; if the user switches \
         language, you switch too. If that message has no words (only code, an image or \
         symbols), use {}. Messages starting with \"[AgentKnife\" are notes from \
         AgentKnife, not from the user: ignore them when choosing the language.\n\n",
        language_name(ui_locale)
    ));
    s.push_str("## Working with the user\n");
    s.push_str(
        "- The user does not program. Use plain words, avoid jargon, and keep technical \
         detail short unless they ask for it.\n",
    );
    match plan_mode {
        PlanMode::Auto => s.push_str(&format!(
            "- Small changes (a color, some text, one component): just do them. Big changes \
             (a new feature or page, changes across many files, installing anything): first \
             call `{PLAN_TOOL}` with 3 to 5 short steps and a rough time estimate, and wait \
             for the answer before changing anything.\n"
        )),
        PlanMode::AlwaysAsk => s.push_str(&format!(
            "- Before changing any file or running any command in a turn, call `{PLAN_TOOL}` \
             and wait for the user's approval.\n"
        )),
        PlanMode::NeverAsk => {}
    }
    s.push_str(
        "- When building or running fails, read the error and fix the cause. Do not repeat \
         the same fix. If it keeps failing, AgentKnife stops and asks the user.\n",
    );
    s.push_str(
        "- Project memory: after a meaningful change, update `.agentknife/PROJECT.md` (what \
         the app is for, its features, decisions made) and `.agentknife/plan/STATUS.md` / \
         `.agentknife/plan/tasks.md` (what is done, doing, blocked), in the user's \
         language. They are how you remember the project next time.\n",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_prompt_is_versioned_and_header_free() {
        assert!(product_prompt_version() >= 1);
        let p = product_prompt();
        assert!(p.starts_with("You are AgentKnife"));
        assert!(!p.contains("prompt-version"));
        assert!(p.contains("data-ak-id") && p.contains("Node.js"));
    }

    #[test]
    fn greeting_names_the_ui_language() {
        assert!(greeting_note(Locale::ZhTw).contains("Write in Traditional Chinese"));
        assert!(greeting_note(Locale::En).contains("development tools"));
    }

    #[test]
    fn rules_follow_locale_and_plan_mode() {
        let zh = system_prompt(Some("You are AgentKnife."), Locale::ZhTw, PlanMode::Auto);
        assert!(zh.starts_with("You are AgentKnife.\n\n"));
        assert!(
            zh.contains("most recent message") && zh.contains("Traditional Chinese (繁體中文)")
        );
        assert!(zh.contains(PLAN_TOOL));
        let en = system_prompt(None, Locale::En, PlanMode::NeverAsk);
        assert!(en.contains("use English"));
        assert!(!en.contains(PLAN_TOOL));
        assert!(
            system_prompt(None, Locale::En, PlanMode::AlwaysAsk)
                .contains("Before changing any file")
        );
    }
}
