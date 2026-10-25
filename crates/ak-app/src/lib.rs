//! AgentKnife: the Tauri window around ak-shell, and the commands behind it
//! (`ak_types::ipc`). The only AgentKnife crate that depends on tauri; the
//! only one that uses `anyhow`.
//!
//! A library so tests/e2e.rs can run the same commands on Tauri's mock
//! runtime; src/main.rs is the window.

#![forbid(unsafe_code)]

pub mod connect;
pub mod crash;
pub mod home;
pub mod mcp;
pub mod settings;
pub mod state;
pub mod tools;
pub mod turn;
pub mod workspace;

use ak_types::i18n::Locale;
use tauri::{Builder, Manager, RunEvent, Runtime};

/// Command results: errors are JSON (text, `ConnectError`, `BundleFailure`).
pub type Res<T> = Result<T, serde_json::Value>;

pub fn fail(e: impl std::fmt::Display) -> serde_json::Value {
    serde_json::Value::String(e.to_string())
}

/// The language chosen in the settings (`None`: the system's).
static LANGUAGE: std::sync::RwLock<Option<Locale>> = std::sync::RwLock::new(None);

pub fn set_language(language: Option<Locale>) {
    if let Ok(mut l) = LANGUAGE.write() {
        *l = language;
    }
}

/// The UI language: the settings' choice, else the system's.
pub fn ui_locale() -> Locale {
    let chosen = LANGUAGE.read().ok().and_then(|l| *l);
    Locale::resolve(chosen, &Locale::os_preferences())
}

/// Every command the window can call, named as in `ak_types::ipc::cmd`.
#[cfg(test)]
const COMMANDS: &[&str] = &[
    "home_state",
    "create_project",
    "open_project",
    "delete_project",
    "open_folder",
    "list_models",
    "check_model",
    "save_connection",
    "saved_connections",
    "send_message",
    "answer_confirmation",
    "answer_plan",
    "answer_question",
    "cancel_turn",
    "continue_turn",
    "stuck_choice",
    "resolve_interruption",
    "preview_status",
    "run_app",
    "get_log",
    "clear_log",
    "list_files",
    "read_file",
    "write_file",
    "timeline",
    "restore",
    "mark_version",
    "doctor_check",
    "doctor_install",
    "doctor_action",
    "bundle",
    "reveal",
    "open_url",
    "get_settings",
    "save_settings",
    "crash_reports",
    "pick_folder",
    "mcp_status",
    "set_mcp",
    "set_selection",
    "open_terminal",
];

/// The app's state and commands on `builder` (plugins are the caller's).
pub fn with_commands<R: Runtime>(builder: Builder<R>, state: state::AppState) -> Builder<R> {
    builder
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            home::home_state,
            home::create_project,
            home::open_project,
            home::delete_project,
            home::open_folder,
            connect::list_models,
            connect::check_model,
            connect::save_connection,
            connect::saved_connections,
            turn::send_message,
            turn::answer_confirmation,
            turn::answer_plan,
            turn::answer_question,
            turn::cancel_turn,
            turn::continue_turn,
            turn::stuck_choice,
            turn::resolve_interruption,
            workspace::preview_status,
            workspace::run_app,
            workspace::get_log,
            workspace::clear_log,
            workspace::list_files,
            workspace::read_file,
            workspace::write_file,
            workspace::timeline,
            workspace::restore,
            workspace::mark_version,
            tools::doctor_check,
            tools::doctor_install,
            tools::doctor_action,
            tools::bundle,
            tools::reveal,
            tools::open_url,
            settings::get_settings,
            settings::save_settings,
            settings::crash_reports,
            settings::pick_folder,
            mcp::mcp_status,
            mcp::set_mcp,
            workspace::set_selection,
            mcp::open_terminal,
        ])
        .setup(|app| {
            // Listens for other AI tools when the setting is on.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { mcp::apply(&handle).await });
            Ok(())
        })
}

/// For `App::run`: stops the preview's dev servers and the app window with
/// AgentKnife.
pub fn on_event<R: Runtime>(handle: &tauri::AppHandle<R>, event: RunEvent) {
    if let RunEvent::Exit = event {
        mcp::shut_down(&handle.state::<state::AppState>());
        close_project(handle);
    }
}

/// Closes the open project (its turn, preview and app window).
pub fn close_project<R: Runtime>(handle: &tauri::AppHandle<R>) {
    let state = handle.state::<state::AppState>();
    tauri::async_runtime::block_on(async {
        let open = state.inner.lock().await.open.take();
        home::close(open).await;
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_ipc_command_is_handled() {
        let ours: BTreeSet<&str> = COMMANDS.iter().copied().collect();
        let contract: BTreeSet<&str> = ak_types::ipc::cmd::ALL.iter().copied().collect();
        assert_eq!(ours, contract);
        // The handler list above names the same functions.
        let source = include_str!("lib.rs");
        let handlers = source
            .split("generate_handler![")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .unwrap_or_default();
        for c in COMMANDS {
            assert!(
                handlers.contains(&format!("::{c},")),
                "{c} is not registered"
            );
        }
    }
}
