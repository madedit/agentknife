//! Other AI tools operating AgentKnife (MCP, P1-MCP-001): who is connected,
//! the setup command for each tool, the connect wizard's step and its
//! on/off switch.

use ak_types::ipc::McpStatus;
use leptos::prelude::*;

use crate::backend::Backend;
use crate::chat::tools::copy;
use crate::i18n::{t, t_with};

/// The tools the wizard offers: id and the name people know it by (`None`:
/// "another MCP tool", in the UI language).
pub const TOOLS: &[(&str, Option<&str>)] = &[
    ("claude-code", Some("Claude Code")),
    ("codex", Some("Codex")),
    ("antigravity", Some("Antigravity")),
    ("other", None),
];

/// What the window knows about other AI tools (a context).
#[derive(Debug, Clone, Copy)]
pub struct External {
    pub status: RwSignal<McpStatus>,
}

impl External {
    pub fn new() -> Self {
        External {
            status: RwSignal::new(McpStatus::default()),
        }
    }

    /// The context, or a fresh one (render tests).
    pub fn get() -> Self {
        use_context::<External>().unwrap_or_else(|| {
            let e = External::new();
            provide_context(e);
            e
        })
    }

    /// The tools connected now, joined for a sentence ("Claude Code, Codex").
    pub fn names(self) -> Option<String> {
        self.status
            .with(|s| (!s.clients.is_empty()).then(|| s.clients.join(", ")))
    }

    /// Fetches the status and keeps it up to date.
    pub fn follow(self, backend: &Backend) {
        backend.on(ak_types::ipc::MCP_EVENT, move |s: McpStatus| {
            self.status.set(s);
        });
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            if let Ok(s) = backend.mcp_status().await {
                self.status.set(s);
            }
        });
    }

    /// Turns "let other AI tools operate AgentKnife" on or off.
    pub fn set(self, backend: Backend, on: bool) {
        leptos::task::spawn_local(async move {
            if let Ok(s) = backend.set_mcp(on).await {
                self.status.set(s);
            }
        });
    }
}

impl Default for External {
    fn default() -> Self {
        External::new()
    }
}

/// A tool's name for the user.
pub fn tool_name(id: &str) -> String {
    match TOOLS.iter().find(|(i, _)| *i == id) {
        Some((_, Some(name))) => (*name).to_owned(),
        _ => t("external-other")(),
    }
}

/// What the user pastes to set up `tool` with `program` (agentknife-mcp's
/// full path): a terminal command run in the projects folder, or for other
/// tools the usual JSON config. Claude Code keeps it for that folder only
/// (its default), so sessions elsewhere do not start it; Codex and
/// Antigravity (`agy`) have one setting for every folder.
pub fn setup_text(tool: &str, program: &str) -> String {
    let quoted = format!("\"{program}\"");
    match tool {
        "claude-code" => format!("claude mcp add agentknife -- {quoted}"),
        "codex" => format!("codex mcp add agentknife -- {quoted}"),
        "antigravity" => format!("agy mcp add agentknife {quoted}"),
        _ => {
            let config = serde_json::json!({
                "mcpServers": { "agentknife": { "command": program } }
            });
            serde_json::to_string_pretty(&config).unwrap_or_default()
        }
    }
}

/// How to set up one tool: the text to paste, with a copy button.
#[component]
pub fn SetupCommand(tool: &'static str) -> impl IntoView {
    let external = External::get();
    let program = move || external.status.with(|s| s.program.clone());
    let folder = move || external.status.with(|s| s.folder.clone());
    let text = move || setup_text(tool, &program());
    // Values in `Show`'s children, which run again: kept, not moved.
    let name = StoredValue::new(tool_name(tool));
    let terminal = tool != "other";
    let first = if terminal {
        "external-step1"
    } else {
        "external-step1-config"
    };
    let backend = StoredValue::new(expect_context::<Backend>());
    view! {
        <div data-setup=tool class="text-sm">
            <Show
                when=move || !program().is_empty()
                fallback=|| view! {
                    <p role="alert" class="text-red-700 dark:text-red-300">{t("external-no-program")}</p>
                }
            >
                <p>{t(first)}</p>
                <div class="mt-1 flex items-start gap-2">
                    <pre data-command="" class="min-w-0 flex-1 overflow-auto rounded-lg bg-gray-100 p-2 text-xs whitespace-pre-wrap dark:bg-gray-800">{text}</pre>
                    <button
                        type="button"
                        data-action="copy"
                        title=t("external-copy-tip")
                        class="shrink-0 rounded-lg border border-gray-300 px-2 py-1 text-xs hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800"
                        on:click=move |_| copy(&text())
                    >
                        {t("external-copy")}
                    </button>
                </div>
                {terminal.then(|| view! {
                    <button
                        type="button"
                        data-action="open-terminal"
                        title=move || t_with("external-open-terminal-tip", [("folder", folder())])()
                        class="mt-2 rounded-lg border border-gray-300 px-3 py-1 text-xs hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800"
                        on:click=move |_| {
                            let backend = backend.get_value();
                            leptos::task::spawn_local(async move {
                                let _ = backend.open_terminal().await;
                            });
                        }
                    >
                        {t("external-open-terminal")}
                    </button>
                })}
                {matches!(tool, "codex" | "antigravity").then(|| view! {
                    <p class="mt-2 text-xs text-gray-600 dark:text-gray-400">
                        {t_with("external-everywhere", [("name", name.get_value())])}
                    </p>
                })}
                <p class="mt-2">
                    {move || t_with("external-step2", [("name", name.get_value()), ("folder", folder())])()}
                </p>
            </Show>
        </div>
    }
}

/// The connect wizard's step for another AI tool: turns the setting on,
/// shows the setup, and waits until the tool connects.
#[component]
pub fn ExternalStep(
    tool: &'static str,
    on_back: Callback<()>,
    on_done: Option<Callback<()>>,
) -> impl IntoView {
    let external = External::get();
    let backend = expect_context::<Backend>();
    if !external.status.with_untracked(|s| s.enabled) {
        external.set(backend, true);
    }
    let name = tool_name(tool);
    let about = t_with("external-about", [("name", name.clone())]);
    let waiting = t_with("external-waiting", [("name", name.clone())]);
    let connected = move || external.names();
    view! {
        <section data-step="external" class="mt-6 rounded-xl border border-gray-200 bg-white p-5 dark:border-gray-700 dark:bg-gray-950">
            <div data-bar="step" class=crate::connect::STEP_BAR>
                <button title=t("connect-back-tip") type="button" class=SECONDARY on:click=move |_| on_back.run(())>
                    {t("connect-back")}
                </button>
                {on_done.map(|done| view! {
                    <button
                        title=t("external-done-tip")
                        type="button"
                        data-action="external-done"
                        class=PRIMARY
                        disabled=move || connected().is_none()
                        on:click=move |_| done.run(())
                    >
                        {t("external-done")}
                    </button>
                })}
            </div>
            <h2 class="text-lg font-medium">{name}</h2>
            <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">{about}</p>
            <div class="mt-4">
                <SetupCommand tool />
            </div>
            <p data-external-state="" class="mt-4 text-sm" aria-live="polite">
                {move || match connected() {
                    Some(names) => t_with("external-connected", [("names", names)])(),
                    None => waiting(),
                }}
            </p>
        </section>
    }
}

/// The switch in the wizard's group: "let other AI tools operate AgentKnife"
/// and who is connected. Turning it off disconnects them all.
#[component]
pub fn ExternalSwitch() -> impl IntoView {
    let external = External::get();
    let backend = expect_context::<Backend>();
    let enabled = move || external.status.with(|s| s.enabled);
    view! {
        <div data-external-switch="" class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
            <label class="flex items-center gap-2" title=t("connect-external-toggle-tip")>
                <input
                    type="checkbox"
                    data-setting="mcp"
                    prop:checked=enabled
                    on:change=move |e| external.set(backend.clone(), event_target_checked(&e))
                />
                {t("connect-external-toggle")}
            </label>
            <Show when=enabled>
                <span data-external-clients="" class="text-gray-600 dark:text-gray-400">
                    {move || match external.names() {
                        Some(names) => t_with("external-connected", [("names", names)])(),
                        None => t("connect-external-none")(),
                    }}
                </span>
            </Show>
        </div>
    }
}

const PRIMARY: &str = "rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50";
const SECONDARY: &str = "rounded-lg border border-gray-300 px-4 py-2 text-sm hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tool_gets_its_own_setup() {
        let p = r"C:\Program Files\AgentKnife\agentknife-mcp.exe";
        assert_eq!(
            setup_text("claude-code", p),
            r#"claude mcp add agentknife -- "C:\Program Files\AgentKnife\agentknife-mcp.exe""#
        );
        assert!(setup_text("codex", p).starts_with("codex mcp add agentknife -- \""));
        assert!(setup_text("antigravity", p).starts_with("agy mcp add agentknife \""));
        // JSON escapes the backslashes.
        let json: serde_json::Value =
            serde_json::from_str(&setup_text("other", p)).unwrap_or_default();
        assert_eq!(json["mcpServers"]["agentknife"]["command"], p);
    }
}
