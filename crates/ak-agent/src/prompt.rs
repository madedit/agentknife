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
         symbols), use {}.\n\n",
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
    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
