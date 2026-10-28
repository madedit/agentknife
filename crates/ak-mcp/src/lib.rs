//! Lets agent CLIs (Claude Code, Codex, Antigravity, …) operate AgentKnife over
//! MCP (P1-MCP-001).
//!
//! The client starts `agentknife-mcp`, which connects its stdio to AgentKnife
//! over a local TCP socket: one line `AGENTKNIFE-MCP/1 <token>` first, then the
//! client's MCP messages unchanged. AgentKnife speaks MCP on that socket
//! ([`Server`]) and runs the tools through an [`McpHost`] (ak-app), so they are
//! the built-in agent's tools with the same permissions.

#![forbid(unsafe_code)]

pub mod endpoint;
mod server;
pub mod workflow;

pub use endpoint::Endpoint;
pub use server::{Client, Document, McpHost, Server, display_name, mcp_tool};

/// First line a connection sends, followed by a space and the token.
pub const HELLO: &str = "AGENTKNIFE-MCP/1";

/// What the server tells every client when it connects (MCP `instructions`):
/// the short version of the built-in agent's working rules.
pub const INSTRUCTIONS: &str = "\
AgentKnife builds desktop apps for people who do not program. Its window shows the \
user's app running (the preview) and every step you take here.

- Use these tools rather than your own file and shell tools: then the user sees your \
steps, AgentKnife asks them before risky steps, keeps versions they can go back to, \
and restarts the app's background part when its code changes. `list_files` and \
`search_files` skip build output (`target/`) and `.git`; a shell listing of the \
project does not, and is huge. Paths in these tools (and `run_command`'s folder) \
are the open project's; your own shell may start in the projects folder, one \
level up: there, use the absolute folder `status` names, never a relative path.
- Start with `status`. If no project is open, use `list_projects` and `open_project`, \
or `new_project`. `check_tools` checks (and, once the user agrees, installs) what \
building needs.
- Before writing app code, read the stack's docs with `read_knowledge` (start with \
`overview`). The library versions are pinned and differ from what you may know.
- Give every element a `data-ak-id`. Never edit the `ak/` folder (it is generated).
- The user can point at an element in the preview. When they say \"this\", \"here\" \
or similar, call `get_selection` to see what they picked.
- After changing files, call `build_status`; check the screen with `dom_summary`, \
`interact` and `screenshot_preview` (they wait for the build first). The preview is \
often only 700–900 px wide (`dom_summary` gives the size): make layouts work there, \
not only at `lg:` and wider.
- `checkpoint` after each working part; `restore` goes back (the user confirms). \
`run_app` opens the app in a real window, `package_app` makes the file to share.
- Text the user sees in the app is in the user's language. Give each call a short \
`label` in the user's language: AgentKnife shows it as the step.
- While AgentKnife's own AI is working, changes are refused: wait and try again.";
