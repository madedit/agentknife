# Saving data

Users expect what they entered to be there next time. Choose one way per app.

## Recommended: a JSON file written by the backend
Survives updates, works the same in the preview and the installed app, easy to back
up. Backend (`src-tauri/src/main.rs`):
```rust
use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Note {
    id: u64,
    text: String,
}

fn notes_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("notes.json"))
}

#[tauri::command]
fn load_notes(app: tauri::AppHandle) -> Result<Vec<Note>, String> {
    let path = notes_path(&app)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("The saved notes are damaged: {e}")),
        // First run: nothing saved yet.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn save_notes(app: tauri::AppHandle, notes: Vec<Note>) -> Result<(), String> {
    let path = notes_path(&app)?;
    let text = serde_json::to_string_pretty(&notes).map_err(|e| e.to_string())?;
    // Write to a temporary file first, so a crash never leaves half a file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}
```
Add `serde_json = "1.0.154"` to `src-tauri/Cargo.toml` `[dependencies]` and register
both commands in `generate_handler!`.

Screens: load once at start, save after each change (see `example-todo` for the whole
thing). In the component body:
```rust
#[derive(Serialize)]
struct SaveArgs { notes: Vec<Note> }

let toaster = use_toaster();   // here, not inside the async block
let notes = RwSignal::new(Vec::<Note>::new());
leptos::task::spawn_local(async move {
    if let Ok(list) = ak_bridge::invoke::<Vec<Note>>("load_notes", &()).await {
        notes.set(list);
    }
});
// after a change:
let save = move || {
    let args = SaveArgs { notes: notes.get_untracked() };
    leptos::task::spawn_local(async move {
        if let Err(e) = ak_bridge::invoke::<()>("save_notes", &args).await {
            toaster.error(e.to_string());
        }
    });
};
```
The screen-side `Note` struct must have the same fields and the same
`#[serde(rename_all = "camelCase")]`.

## Simple alternative: localStorage
Only for small settings (a theme, the last tab). Data stays in the web view and is
lost if the app's identifier changes. Requires `web-sys` with the `Storage` and
`Window` features:
```rust
fn save_setting(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}
```
