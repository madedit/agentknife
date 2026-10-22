# Calling the backend: ak_bridge::invoke

`ak_bridge::invoke` calls a backend command (topic `tauri-commands`). It works in
AgentKnife's preview and in the installed app. Never call Tauri's JavaScript API.

```rust
use serde::{Deserialize, Serialize};

/// Arguments: one field per command argument, camelCase on the wire.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameArgs {
    item_id: u64,      // the command's `item_id` argument
    new_name: String,  // the command's `new_name` argument
}

/// The result type must match what the command returns.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: u64,
    name: String,
}

async fn rename(id: u64, name: String) -> Result<Item, ak_bridge::InvokeError> {
    ak_bridge::invoke::<Item>("rename_item", &RenameArgs { item_id: id, new_name: name }).await
}
```
- **Always `#[serde(rename_all = "camelCase")]` on the arguments struct**, even
  when every name is one word today: the backend expects `fileName` for its
  `file_name` argument, and a mismatch makes the command fail.
- **Always handle the `Err`**: show it (`toaster.error(e.to_string())`). Never
  `let _ =` or `if let Ok` alone: the button then silently does nothing. In the
  preview a failed command is also listed under the page's errors in
  `dom_summary`.
- No arguments: pass `&()`.
- No result (`Result<(), String>` in the backend): `invoke::<()>(…)`.
- `invoke` is async: call it inside `leptos::task::spawn_local(async move { … })`
  from event handlers (topic `leptos-reactivity`).

## Errors
`InvokeError` displays well (`e.to_string()`):
- `Command(value)` — the command returned `Err(…)`: show it to the user, e.g. with
  `toaster.error(e.to_string())` (get `toaster` with `use_toaster()` in the
  component body).
- `Transport` — the backend is restarting or stopped (in the preview, after a backend
  change); trying again usually works.
- `Decode` — the result did not match the Rust type: fix the types.
- `NoBackend` — opened outside the app and the preview.

## Startup
`src/main.rs` calls `ak_bridge::start()` before mounting. Keep it: in the preview it
connects to the backend and lets the user point at elements.

## Helping AgentKnife check your work
```rust
// Lets AgentKnife read the app's state while checking the preview.
ak_bridge::set_state_reader(move || serde_json::json!({ "todos": todos.get_untracked().len() }));
```
(Needs `serde_json` in the screens' `[dependencies]`.) For apps that run by
themselves (games, timers), include a value that changes every step (a tick
count, the falling piece's row): two readings then show whether it is running.
For apps that draw on a `<canvas>`, `ak_bridge::set_hit_test(|p| …)` tells
AgentKnife what is at a point (`p.x`, `p.y` in canvas pixels). Both do nothing in
the installed app.

## Not available
Backend → screen events (`app.emit` / `listen`) do not reach the preview. Instead,
have the screen call a command again (e.g. reload the list after saving).
