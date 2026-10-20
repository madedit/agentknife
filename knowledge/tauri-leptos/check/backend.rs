//! Snippets from the knowledge pack (backend side), compiled by `cargo xtask check-knowledge`.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Manager, State};

// ---- tauri-commands ----
#[tauri::command]
fn add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b).ok_or_else(|| "The numbers are too big.".to_string())
}

#[derive(Default)]
struct Counter(Mutex<u32>);

#[tauri::command]
fn bump(counter: State<'_, Counter>) -> u32 {
    let mut n = counter.0.lock().unwrap_or_else(|e| e.into_inner());
    *n += 1;
    *n
}

#[tauri::command]
async fn slow_task(seconds: u64) -> Result<String, String> {
    std::thread::sleep(std::time::Duration::from_secs(seconds.min(5)));
    Ok("done".into())
}

#[tauri::command]
fn data_file(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("data.json").display().to_string())
}

// ---- storage ----
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn save_notes(app: tauri::AppHandle, notes: Vec<Note>) -> Result<(), String> {
    let path = notes_path(&app)?;
    let text = serde_json::to_string_pretty(&notes).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// The registrations the docs show, to make sure they type-check.
pub fn register(b: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    b.manage(Counter::default())
        .invoke_handler(tauri::generate_handler![add, bump, slow_task, data_file, load_notes, save_notes])
}
