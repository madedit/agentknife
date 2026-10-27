//! The MCP prompt a client offers its user as a command: the whole way from an
//! idea to an installed app with AgentKnife's tools.

/// The prompt's name (`/agentknife:build_app` in Claude Code).
pub const BUILD_APP: &str = "build_app";

/// What the prompt is for, shown in the client's list.
pub const BUILD_APP_DESCRIPTION: &str =
    "Build a desktop app with AgentKnife, from the idea to an installable package.";

/// The prompt's argument: the user's idea, in their words.
pub const IDEA_ARG: &str = "idea";

/// The prompt text; `idea` is the user's request, if they gave one.
pub fn build_app(idea: Option<&str>) -> String {
    let idea = match idea.map(str::trim).filter(|i| !i.is_empty()) {
        Some(i) => format!("The user's idea, in their words:\n\n{i}\n\n"),
        None => "First ask the user what app they want, in a sentence or two.\n\n".to_owned(),
    };
    format!(
        "Build a desktop app with AgentKnife for someone who does not program. They \
         watch it take shape in AgentKnife's preview.\n\n{idea}\
         Work like this, with AgentKnife's tools:\n\
         1. `status`. With no project open, `new_project` (a short name the user agrees \
         to) or `list_projects` and `open_project`.\n\
         2. `check_tools`. If tools are missing, the user confirms the install in \
         AgentKnife's window; wait for it. Some system parts are installed by the user \
         with the steps the result gives: tell them in plain words.\n\
         3. Read the stack's docs with `read_knowledge` (`overview` first, then what the \
         app needs) and the project's notes (the `agentknife://project/PROJECT.md` \
         resource).\n\
         4. Build in small steps. After each change, `build_status`; check the screen \
         with `dom_summary`, `interact` and `screenshot_preview`. Fix build errors before \
         going on.\n\
         5. After each part the user can see working, `checkpoint` with a short label in \
         the user's language, so they can go back to it.\n\
         6. Show the user what changed and ask what to change next. When they say \
         \"this\" or \"here\", call `get_selection`.\n\
         7. When they are happy: `run_app` to try it in a real window, then \
         `package_app`. Give them the install steps it returns, in their language.\n\n\
         Talk to the user in their language, in plain words, without code unless they \
         ask for it."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_idea_is_quoted_or_asked_for() {
        let with = build_app(Some("  一個記帳 App  "));
        assert!(
            with.contains("in their words:\n\n一個記帳 App\n\n"),
            "{with}"
        );
        assert!(!with.contains("First ask"));
        for none in [None, Some(" ")] {
            assert!(build_app(none).contains("First ask the user"));
        }
        assert!(with.contains("`package_app`"));
    }
}
