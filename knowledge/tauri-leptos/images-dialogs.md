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
```
- Many small pictures (a grid): `thumbnail_url`. One picture shown large (a
  viewer): `picture_url`, which keeps PNG/WebP transparency, GIF animation and SVG.
  Only the size: `picture_size`; `image::open` would decode every pixel first.
- `thumbnail(width, height)` takes both sizes and keeps the proportions
  (`thumbnail_exact` stretches). `resize` is slower and sharper.
- Writing to memory is `write_to(&mut Cursor::new(&mut bytes), ImageFormat::…)`;
  to a file `save(path)` (the format comes from the extension). There is no
  `save_to`.
- `Unsupported` errors: the format's feature is missing above, or a JPEG was
  written from a picture with transparency (convert with `to_rgb8()`). SVG is not
  an `image` format: send its file as is, as above.
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
