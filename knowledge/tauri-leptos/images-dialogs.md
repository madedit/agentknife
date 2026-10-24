# Pictures and the system dialogs (backend)

Both run in the backend (topic `tauri-commands`); the screens call them with
`ak_bridge::invoke` (topic `ak-bridge`).

## Pictures: the `image` crate
In `src-tauri/Cargo.toml` (only the formats you need: the build is much shorter):
```toml
image = { version = "0.25.10", default-features = false, features = ["jpeg", "png", "gif", "webp"] }
base64 = "0.22.1"
```
```rust
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
```
- `thumbnail(width, height)` takes both sizes and keeps the proportions
  (`thumbnail_exact` stretches). `resize` is slower and sharper.
- Writing to memory is `write_to(&mut Cursor::new(&mut bytes), ImageFormat::…)`;
  to a file `save(path)` (the format comes from the extension). There is no
  `save_to`.
- `Unsupported` errors: the format's feature is missing above, or a JPEG was
  written from a picture with transparency (convert with `to_rgb8()`).
- Many pictures: make thumbnails one command call each (or a few at a time) so
  the screen can show them as they arrive, and keep them in memory or in the
  app's data folder instead of making them again.

## Choosing a folder or a file: the dialog plugin
In `src-tauri/Cargo.toml`: `tauri-plugin-dialog = "2.8.3"`, and in `main`'s
builder `.plugin(tauri_plugin_dialog::init())`.
```rust
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
```
- The builder methods are `set_title`, `add_filter("Pictures", &["jpg", "png"])`,
  `set_directory(path)`; then `blocking_pick_file`, `blocking_pick_files`,
  `blocking_pick_folder` or `blocking_save_file`.
- They return a `FilePath` (a path or a URL), not a `PathBuf`: `into_path()` gives
  the `PathBuf` (`?`), or `to_string()` the text. It has no `.display()`.
- Without the plugin, `rfd = "0.16.1"` (what the plugin uses) opens the same
  dialogs and returns a plain `PathBuf`:
  ```rust
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
  ```
- Dialogs only open in the real window, not in the preview (topic `overview`):
  next to the button, offer a text field where a path can be typed. The field
  must do the whole job, so it can be tried in the preview: Import = the
  "Choose…" button or a typed path, both calling the same `import_from(path)`
  (which reads, checks, shows the result or the error); Export the same with
  `export_to(path)`.
