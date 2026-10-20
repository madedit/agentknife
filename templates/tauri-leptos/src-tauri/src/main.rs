//! The app's backend: Tauri commands the screens call with `ak_bridge::invoke`.

// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

/// Returns a greeting for `name`. Errors are shown to the user, so write them
/// in the app's language.
#[tauri::command]
fn greet(name: String) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Please type a name.".into());
    }
    Ok(format!("Hello, {name}!"))
}

fn main() {
    tauri::Builder::default()
        // In AgentKnife's preview this serves the commands to the preview and
        // keeps the window hidden; in the real app it does nothing.
        .plugin(ak_bridge::init())
        .invoke_handler(tauri::generate_handler![greet])
        .setup(|app| {
            // The window starts hidden (tauri.conf.json) so the preview never
            // flashes it; the real app shows it here.
            if !ak_bridge::is_preview()
                && let Some(window) = app.get_webview_window("main")
            {
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the app failed to start");
}
