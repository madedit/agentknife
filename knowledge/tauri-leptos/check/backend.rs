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

// ---- storage: export ----
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

/// The registrations the docs show, to make sure they type-check.
pub fn register(b: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    b.manage(Counter::default())
        .invoke_handler(tauri::generate_handler![add, bump, slow_task, data_file, load_notes, save_notes, export_csv])
}

// ---- date-time ----
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ---- images-dialogs ----
use base64::Engine;
use std::io::Cursor;

/// A JPEG thumbnail of the picture at `path` (at most `size` pixels wide and high),
/// as text the screens can put in `<img src=…>`.
fn thumbnail_url(path: &std::path::Path, size: u32) -> Result<String, String> {
    let picture = image::open(path)
        .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    // Keeps the proportions. JPEG has no transparency: convert to RGB first.
    let small = image::DynamicImage::ImageRgb8(picture.thumbnail(size, size).to_rgb8());
    let mut bytes = Vec::new();
    small
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    let text = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/jpeg;base64,{text}"))
}

#[tauri::command]
async fn thumbnail(path: String) -> Result<String, String> {
    // Decoding pictures is slow: keep it off the window's thread.
    tauri::async_runtime::spawn_blocking(move || thumbnail_url(std::path::Path::new(&path), 240))
        .await
        .map_err(|e| e.to_string())?
}

use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn choose_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    // The dialog waits for the user: keep it off the window's thread.
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a photo folder")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(folder) = picked else {
        return Ok(None); // the user cancelled
    };
    let path = folder.into_path().map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
