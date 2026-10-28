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
use std::path::Path;

/// Picture files up to this size are shown as they are (see `picture_url`).
const AS_IS_MAX: u64 = 8 * 1024 * 1024;

fn data_url(mime: &str, bytes: &[u8]) -> String {
    let text = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{text}")
}

/// A small copy of the picture at `path` (at most `size` pixels wide and high),
/// as text the screens can put in `<img src=…>`: for grids of many pictures.
/// PNG when it has transparency, else JPEG (much smaller). An animated GIF
/// becomes its first frame.
fn thumbnail_url(path: &Path, size: u32) -> Result<String, String> {
    let picture = image::open(path)
        .map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    // Keeps the proportions.
    let small = picture.thumbnail(size, size);
    let mut bytes = Vec::new();
    if small.color().has_alpha() {
        small
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(data_url("image/png", &bytes))
    } else {
        // JPEG has no transparency: convert to RGB first.
        image::DynamicImage::ImageRgb8(small.to_rgb8())
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?;
        Ok(data_url("image/jpeg", &bytes))
    }
}

/// One picture shown large: the file as it is (transparency, animation and
/// full quality kept; no decoding), or a 2048-pixel copy when it is very big.
fn picture_url(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("Could not open {}: {e}", path.display()))?
        .len();
    if size > AS_IS_MAX {
        return thumbnail_url(path, 2048);
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let svg = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let mime = if svg {
        "image/svg+xml"
    } else {
        image::guess_format(&bytes)
            .map_err(|e| format!("Not a picture: {e}"))?
            .to_mime_type()
    };
    Ok(data_url(mime, &bytes))
}

/// Width and height, read from the file's first bytes (nothing is decoded).
fn picture_size(path: &Path) -> Result<(u32, u32), String> {
    image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("Could not open {}: {e}", path.display()))?
        .into_dimensions()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn thumbnail(path: String) -> Result<String, String> {
    // Decoding pictures is slow: keep it off the window's thread.
    tauri::async_runtime::spawn_blocking(move || thumbnail_url(Path::new(&path), 240))
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

// ---- images-dialogs: rfd ----
#[tauri::command]
async fn choose_folder_rfd() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Choose a folder")
            .pick_folder()
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

// ==== generated from new topics (compose_checks.py) ====

// ---- import-backup ----
/// The rows of a CSV file, each a list of cells.
#[tauri::command]
async fn import_csv(path: String) -> Result<Vec<Vec<String>>, String> {
    let path = path.trim().trim_matches('"'); // "Copy as path" adds quotes
    let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    Ok(parse_csv(&decode_text(&bytes)))
}

/// UTF-8 (with or without a BOM), else the old encoding of Excel's "CSV".
fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        // Traditional Chinese Windows. Simplified: GBK; Japanese: SHIFT_JIS.
        Err(_) => encoding_rs::BIG5.decode(bytes).0.into_owned(),
    }
}

/// Splits CSV text into rows. Quoted cells may hold commas, line breaks and
/// doubled quotes (""); empty lines are skipped.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => cell.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut cell)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut cell));
                if row.iter().any(|c| !c.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                }
                row.clear();
            }
            (false, c) => cell.push(c),
        }
    }
    row.push(cell);
    if row.iter().any(|c| !c.is_empty()) {
        rows.push(row);
    }
    rows
}

/// Copies the saved notes to `folder`; returns the backup's full path.
#[tauri::command]
fn backup_notes(app: tauri::AppHandle, folder: String) -> Result<String, String> {
    let from = notes_path(&app)?;
    if !from.exists() {
        return Err("Nothing has been saved yet.".into());
    }
    let name = format!("notes-backup-{}.json", chrono::Local::now().format("%Y-%m-%d-%H%M"));
    let to = std::path::Path::new(folder.trim().trim_matches('"')).join(name);
    std::fs::copy(&from, &to).map_err(|e| format!("Cannot write the backup: {e}"))?;
    Ok(to.display().to_string())
}

/// Replaces the notes with a backup and returns them for the screen.
#[tauri::command]
fn restore_notes(app: tauri::AppHandle, path: String) -> Result<Vec<Note>, String> {
    let text = std::fs::read_to_string(path.trim().trim_matches('"'))
        .map_err(|e| format!("Cannot read the backup: {e}"))?;
    // Check it before touching the current data.
    let notes: Vec<Note> =
        serde_json::from_str(&text).map_err(|_| "This file is not a backup of this app.".to_string())?;
    let current = notes_path(&app)?;
    if current.exists() {
        std::fs::copy(&current, current.with_extension("json.bak")).map_err(|e| e.to_string())?;
    }
    let tmp = current.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &current).map_err(|e| e.to_string())?;
    Ok(notes)
}

// ---- desktop ----
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

fn desktop_builder(b: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    b.plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .on_window_event(|window, event| if !ak_bridge::is_preview() { hide_on_close(window, event) })
        .setup(|app| {
            log_crashes(app);
            if !ak_bridge::is_preview() {
                setup_tray(app)?;
            }
            Ok(())
        })
}

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

// ---- sound: the user's songs ----
mod songs {
    /// Checks a song the user picked; returns its name for the list.
    #[tauri::command]
    fn add_song(path: String) -> Result<String, String> {
        let path = std::path::Path::new(&path);
        song_type(path)?;
        if !path.is_file() {
            return Err("There is no song at this path.".into());
        }
        let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Song");
        Ok(name.to_owned())
    }

    /// One song as a data URL for `<audio src>`, read from where the user keeps it.
    #[tauri::command]
    fn song_url(path: String) -> Result<String, String> {
        use base64::Engine;
        let path = std::path::Path::new(&path);
        let mime = song_type(path)?;
        let bytes = std::fs::read(path)
            .map_err(|_| "The song is no longer there (moved or deleted). Add it again.".to_owned())?;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(format!("data:{mime};base64,{data}"))
    }

    /// Only sound files: the screens cannot ask for any other file.
    fn song_type(path: &std::path::Path) -> Result<&'static str, String> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext.to_ascii_lowercase().as_str() {
            "mp3" => Ok("audio/mpeg"),
            "wav" => Ok("audio/wav"),
            _ => Err("Choose an MP3 or WAV file.".into()),
        }
    }
}
