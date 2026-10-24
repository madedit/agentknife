# Windows features: notifications, tray icon, starting with Windows, logs

These run in the backend and mostly show only in the real window, not in the
preview (topic `overview`). Build them so the preview still works, then ask the
user to try them in the real window and tell you what they saw.

## Notifications
Add `tauri-plugin-notification = "2.6.0"` to `src-tauri/Cargo.toml`, and
`.plugin(tauri_plugin_notification::init())` to the builder in `main()`.
```rust
use tauri_plugin_notification::NotificationExt;

/// Shows a Windows notification (the small box at the bottom right).
#[tauri::command]
fn notify(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}
```
- Called from the backend, the plugin needs nothing in `capabilities/`.
- A reminder at a set time needs the app running: check the time once a minute
  (a timer in the screens, topic `leptos-reactivity`) and call `notify`. If the
  user closes the window, nothing reminds them: keep the app in the tray (below).
- Windows can turn notifications off for an app (Settings → System →
  Notifications). If the user sees none, ask them to look there.

## A tray icon (the app keeps running in the background)
Add the feature to tauri in `src-tauri/Cargo.toml`:
`tauri = { version = "=2.12.4", features = ["tray-icon"] }` (keep the version).
```rust
use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// The tray icon with "Open" and "Quit"; a left click opens the window.
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("My app")
        .menu(&menu)
        .show_menu_on_left_click(false) // left click opens, right click shows the menu
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Closing the window hides it; "Quit" in the tray ends the app.
fn hide_on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
    }
}
```
In `main()`, only outside the preview (there the backend restarts on every change,
and "Quit" would stop the preview):
```rust
// .on_window_event(|window, event| if !ak_bridge::is_preview() { hide_on_close(window, event) })
// in .setup(|app| { … }): if !ak_bridge::is_preview() { setup_tray(app)?; }
```
- Labels ("Open", "Quit", the tooltip) in the user's language.
- Hiding on close surprises users: tell them in the app the first time ("The app
  keeps running in the tray; quit it there"), or make it a setting.

## Starting with Windows
Add `tauri-plugin-autostart = "2.7.0"` and
`.plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))`.
```rust
use tauri_plugin_autostart::ManagerExt;

/// Turns "start with Windows" on or off; returns the new state.
#[tauri::command]
fn set_autostart(app: tauri::AppHandle, on: bool) -> Result<bool, String> {
    if ak_bridge::is_preview() {
        // It would register the preview's build, which moves with every change.
        return Err("This can be set in the installed app only.".into());
    }
    let launcher = app.autolaunch();
    let done = if on { launcher.enable() } else { launcher.disable() };
    done.map_err(|e| e.to_string())?;
    launcher.is_enabled().map_err(|e| e.to_string())
}
```
- Only when the user turns it on (a Toggle in the settings, off at first).
- It remembers where the program file is: if the user moves the app's folder, they
  turn it off and on again.

## A second window
Prefer a `Dialog` or another page in the same window (topic `ak-ui`, `Nav`): a
second window cannot be shown or checked in the preview. Make one only when the
user asks for it, and tell them to try it in the real window.

## Keeping a log
The installed app has no console: when something fails at the user's, a log file
tells what happened. Write one line per important event and every crash to the
app's log folder.
```rust
use std::io::Write;

/// Appends a line with the time to `app.log` in the app's log folder.
fn log_line(app: &tauri::AppHandle, text: &str) {
    if let Ok(dir) = app.path().app_log_dir() {
        let _ = std::fs::create_dir_all(&dir);
        append(&dir.join("app.log"), text);
    }
}

fn append(file: &std::path::Path, text: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{} {text}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// Call first thing in `.setup(…)`: crashes are written to the log too.
fn log_crashes(app: &tauri::App) {
    let Ok(dir) = app.path().app_log_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("app.log");
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        append(&file, &format!("crash: {info}"));
        before(info); // keeps the preview's crash report
    }));
}
```
- Log what happened, never the user's data or passwords ("saved 12 notes", not
  their text). Errors returned to the screens: log them in the command too.
- Show the log's place to the user (a line in the settings with
  `app_log_dir()` as text) so they can send it to you.

## Signing and updates
Packaging is AgentKnife's step ("build the installer"), not code in the app. Do not
add a signing setup or the updater plugin: signing needs a certificate the user
buys, and updates need a server that hosts each new version. If the user asks,
explain that, and that Windows shows a SmartScreen warning ("Windows protected
your PC" → More info → Run anyway) for apps that are not signed.
