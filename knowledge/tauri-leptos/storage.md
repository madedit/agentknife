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
#[serde(rename_all = "camelCase")]
struct SaveArgs { notes: Vec<Note> }

let toaster = use_toaster();   // here, not inside the async block
let notes = RwSignal::new(Vec::<Note>::new());
leptos::task::spawn_local(async move {
    match ak_bridge::invoke::<Vec<Note>>("load_notes", &()).await {
        Ok(list) => notes.set(list),
        Err(e) => toaster.error(e.to_string()),
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

## Exporting a file (CSV for Excel)
The backend writes it; the screen sends the rows and shows where the file went.
```rust
/// Writes `rows` as a CSV file in the user's Downloads folder; returns its path.
#[tauri::command]
fn export_csv(
    app: tauri::AppHandle,
    file_name: String,
    rows: Vec<Vec<String>>,
) -> Result<String, String> {
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.csv", safe_file_name(&file_name)));
    // The BOM makes Excel read Chinese and other non-English text correctly.
    let mut text = String::from('\u{feff}');
    for row in &rows {
        let cells: Vec<String> = row.iter().map(|c| csv_cell(c)).collect();
        text.push_str(&cells.join(","));
        text.push_str("\r\n");
    }
    // Same as saving: a temporary file first, so a crash never leaves half a file.
    let tmp = path.with_extension("csv.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Quotes a cell that holds a comma, a quote or a line break.
fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// A name Windows accepts: no \ / : * ? " < > |, no trailing dot or space.
fn safe_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let s = s.trim().trim_end_matches(['.', ' ']);
    if s.is_empty() { "export".to_owned() } else { s.to_owned() }
}
```
Screens (`toaster` and `notes` as above):
```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportArgs {
    file_name: String,
    rows: Vec<Vec<String>>,
}

let export = move |_| {
    let mut rows = vec![vec!["Date".to_owned(), "Amount".to_owned()]];
    rows.extend(notes.get_untracked().iter().map(|n| vec![n.id.to_string(), n.text.clone()]));
    let args = ExportArgs { file_name: "notes 2026-10".into(), rows };
    leptos::task::spawn_local(async move {
        match ak_bridge::invoke::<String>("export_csv", &args).await {
            Ok(path) => toaster.success(format!("Saved to {path}")),
            Err(e) => toaster.error(e.to_string()),
        }
    });
};
```
- Excel needs the BOM (`\u{feff}`) at the start, or Chinese headers turn into
  garbage.
- Names from the user (a month, a title) go through `safe_file_name`: Windows
  refuses `\ / : * ? " < > |` in file names.
- Return the full path and show it: the user knows where to look, and you can check
  the file.

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
