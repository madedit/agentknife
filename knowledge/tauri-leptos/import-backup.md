# Importing files, backups and moving to another computer

## Importing a CSV file (from Excel)
The backend reads the file and returns rows of text; the screens turn the rows
into the app's records and show what was imported. Excel saves CSV two ways:
"CSV UTF-8" (with a BOM) and plain "CSV", which is in the computer's own encoding
(Big5 on Traditional Chinese Windows). Read both. Add
`encoding_rs = "0.8.42"` (already downloaded) to `src-tauri/Cargo.toml`.
```rust
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
```
In the screens, find the columns by their header names (the user's file may have
them in another order or have more), and keep the rows that fail:
```rust
/// "1,234.5", "$80", " 12 " → a number; None if it is not one.
fn parse_amount(cell: &str) -> Option<f64> {
    let s: String = cell.chars().filter(|c| !matches!(c, ',' | '$' | '¥' | '元' | ' ')).collect();
    s.parse().ok()
}

/// "2026/10/5", "2026-10-05" → "2026-10-05".
fn parse_date(cell: &str) -> Option<String> {
    let parts: Vec<u32> = cell.trim().split(['/', '-', '.']).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts[..] {
        [y, m, d] if y > 1900 && (1..=12).contains(&m) && (1..=31).contains(&d) => {
            Some(format!("{y:04}-{m:02}-{d:02}"))
        }
        _ => None,
    }
}

/// The column whose header is one of `names`.
fn column(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|h| names.contains(&h.trim()))
}
```
- Header names in the user's language and in English: `column(&rows[0], &["日期",
  "Date"])`. A missing column: say which one and stop.
- Before adding anything, show a summary: "120 rows can be imported; rows 5 and 9
  are skipped (the amount is not a number)", with "Import" and "Cancel". Importing
  twice should not double the data: skip rows that are already there (same date,
  text and amount), or ask.
- Choosing the file: the dialog plugin with `.add_filter("CSV", &["csv"])` and
  `blocking_pick_file()` (topic `images-dialogs`), plus a text field for a path
  that calls the same import (the preview has no dialogs). Test in the preview with
  a small CSV file you write with `write_file` (also one in Big5: ask the user for
  one saved by their Excel, or test UTF-8 only and say so).
- **Excel files (`.xlsx`)**: ask the user to save the sheet from Excel as "CSV
  UTF-8" (File → Save As) and import that. Reading `.xlsx` directly needs a crate
  this pack does not cover.

## Backups and restoring
The data lives in the app data folder (topic `storage`). A backup copies it to a
place the user chooses (a USB stick, a OneDrive folder); restoring checks the file
first, keeps the current data as `.bak`, then replaces it.
```rust
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
```
- The folder: the dialog's `blocking_pick_folder()` (topic `images-dialogs`) and a
  text field for the preview. After restoring, put the returned data in the
  screen's signal; ask before restoring ("This replaces the current notes").
- Data in several files (pictures, one file per document): copy the whole data
  folder into a dated folder instead, and restore by copying it back.
- Moving to another computer: back up on the old one, install the app on the new
  one, restore there. Tell the user this in the app (a short line next to the
  buttons), and show where the data is: `app_data_dir()` returned as text.
- Check a backup in the preview: back up into the project's folder (its path is in
  your instructions), `list_files` it, change a note, restore, `read_state`.
