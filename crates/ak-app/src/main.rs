//! AgentKnife: the Tauri window around ak-shell, and the commands behind it
//! (`ak_types::ipc`). The only AgentKnife crate that depends on tauri; the
//! only one that uses `anyhow`.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod connect;
mod home;
mod state;
mod tools;
mod turn;
mod workspace;

use ak_types::i18n::Locale;
use tauri::{Manager, RunEvent};

/// Command results: errors are JSON (text, `ConnectError`, `BundleFailure`).
pub type Res<T> = Result<T, serde_json::Value>;

pub fn fail(e: impl std::fmt::Display) -> serde_json::Value {
    serde_json::Value::String(e.to_string())
}

/// The UI language (the system's until settings can change it, P1-APP-002).
pub fn ui_locale() -> Locale {
    Locale::resolve(None, &Locale::os_preferences())
}

/// Every command the window can call, named as in `ak_types::ipc::cmd`.
#[cfg(test)]
const COMMANDS: &[&str] = &[
    "home_state",
    "create_project",
    "open_project",
    "open_folder",
    "list_models",
    "check_model",
    "save_connection",
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
];

fn main() -> anyhow::Result<()> {
    let state = state::AppState::load()?;
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            home::home_state,
            home::create_project,
            home::open_project,
            home::open_folder,
            connect::list_models,
            connect::check_model,
            connect::save_connection,
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
        ])
        .build(tauri::generate_context!())?;
    app.run(|handle, event| {
        // Stop the preview's dev servers and the app window with AgentKnife.
        if let RunEvent::Exit = event {
            let state = handle.state::<state::AppState>();
            tauri::async_runtime::block_on(async {
                let open = state.inner.lock().await.open.take();
                home::close(open).await;
            });
        }
    });
    Ok(())
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
        let source = include_str!("main.rs");
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
