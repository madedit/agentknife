# Tauri backend commands (Tauri 2)

The backend (`src-tauri/src/main.rs`) does what the screens cannot: files, the
computer's folders, long work. The screens call it with `ak_bridge::invoke`.

## A command
```rust
/// Errors (the `Err` text) are shown to the user: write them plainly, in the
/// user's language.
#[tauri::command]
fn add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b).ok_or_else(|| "The numbers are too big.".to_string())
}
```
- Arguments and the result must be serde types (`String`, numbers, `bool`, `Vec`,
  `Option`, and your own `#[derive(Serialize, Deserialize)]` structs).
- Return `Result<T, String>` so mistakes reach the user instead of crashing.

## One data model for both sides
The screens and the backend can use the same file, so their JSON always fits.
Keep it in `src/model.rs` (screens: `mod model;`) and in `src-tauri/src/main.rs`:
```rust
#[path = "../../src/model.rs"]
#[allow(dead_code)] // methods only the screens use
mod model;
```
The path is from `src-tauri/src/`; move the file and change it too. Both
`Cargo.toml` need `serde` with `derive`; nothing in the file may use web-only
crates (`web-sys`, `leptos`).

## Registering
Every command must be listed in `generate_handler!` in `main()`:
```rust
.invoke_handler(tauri::generate_handler![greet, add, list_items])
```
Keep the existing `.plugin(ak_bridge::init())` and the `.setup(…)` that shows the
window.

## Argument names
The screens send argument names in **camelCase**: a command argument `item_id`
arrives as `itemId`. Use a struct with `#[serde(rename_all = "camelCase")]` on the
screen side (topic `ak-bridge`), or keep argument names one word long.

## State that lives while the app runs
```rust
use std::sync::Mutex;
use tauri::State;

#[derive(Default)]
struct Counter(Mutex<u32>);

#[tauri::command]
fn bump(counter: State<'_, Counter>) -> u32 {
    let mut n = counter.0.lock().unwrap_or_else(|e| e.into_inner());
    *n += 1;
    *n
}

// in main(): tauri::Builder::default().manage(Counter::default())…
```
`State` arguments are filled in by Tauri, not sent by the screens.

## Async commands
```rust
#[tauri::command]
async fn slow_task(seconds: u64) -> Result<String, String> {
    std::thread::sleep(std::time::Duration::from_secs(seconds.min(5)));
    Ok("done".into())
}
```
Use `async` for anything that takes time (files, network) so the window stays
responsive. Async commands that take `State` must return `Result` and write the
lifetime: `state: State<'_, Library>`. Work that blocks (a dialog waiting for the
user, a big folder scan) goes in
`tauri::async_runtime::spawn_blocking(move || …).await`.

## Files in the app's own folder
```rust
use tauri::Manager;

#[tauri::command]
fn data_file(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("data.json").display().to_string())
}
```
`app_data_dir()` is per app (by its identifier) and per user — the right place for
saved data (topic `storage`).

## Do not
- Do not use `println!` to talk to the screens; return values instead.
- Do not panic (`unwrap()` on user data): return `Err("…")`.
- Do not add Tauri plugins unless the user's request needs them. A plugin used
  only from Rust (`app.dialog()` in a command) needs no
  `capabilities/default.json` entry: capabilities limit what the page's
  JavaScript may call, and the screens here call only your own commands.
