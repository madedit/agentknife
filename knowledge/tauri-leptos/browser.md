# Browser features in the screens (web-sys)

The screens run in a web view: the clipboard, focus, the page's elements and
events come from the browser through `web-sys` (and `js-sys`, `wasm-bindgen-futures`
for promises). Leptos covers most needs; use these for the rest.

## Dependencies
In the screens' `Cargo.toml` (the versions the project already has):
```toml
js-sys = "0.3.106"
wasm-bindgen-futures = "0.4.79"
web-sys = { version = "0.3.106", features = ["Window", "Navigator", "Clipboard", "Document", "Element", "HtmlInputElement", "EventTarget"] }
```
Every web-sys type and method needs its feature, named after the type that has
it. "no method named …" or "cannot find type … in web_sys" means a missing
feature. Add all you need at once:

| Need | Features |
|---|---|
| the window, the page | `Window`, `Document` |
| an element, its attributes, focus | `Element`, `HtmlElement` |
| one kind of element | `HtmlInputElement`, `HtmlTextAreaElement`, `HtmlSelectElement`, `HtmlDivElement`, `HtmlCanvasElement` |
| events | `Event`, `EventTarget`, `KeyboardEvent`, `MouseEvent` |
| clipboard | `Navigator`, `Clipboard` |
| saved settings in the page | `Storage` |
| nodes and their text | `Node`, `Text`, `CharacterData` |
| the selection | `Selection`, `Range` |
| editing commands (`execCommand`) | `HtmlDocument` |
| fetching from the internet | topic `network` |

Methods live on the exact type, and a general type must be turned into it with
`dyn_into`/`dyn_ref` first: `Node` has `set_text_content` but no `append_data`
(that is on `Text`); `Document` has no `exec_command` (that is on
`HtmlDocument`). To check where a method lives, search the web-sys source
(`src/features/gen_<Type>.rs`) in the libraries folder (see the system prompt).

## Common pieces
```rust
use wasm_bindgen::JsCast;

/// Copies `text` to the clipboard.
async fn copy_text(text: &str) -> Result<(), String> {
    let clipboard = web_sys::window().ok_or("no window")?.navigator().clipboard();
    wasm_bindgen_futures::JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|e| format!("Could not copy: {e:?}"))
}

/// The data-ak-id of the element that has the keyboard focus, if any.
fn focused_id() -> Option<String> {
    let el = web_sys::window()?.document()?.active_element()?;
    el.get_attribute("data-ak-id")
}

/// The `<input>` an event came from (its value, checked state, files).
fn input_of(ev: &web_sys::Event) -> Option<web_sys::HtmlInputElement> {
    ev.target()?.dyn_into::<web_sys::HtmlInputElement>().ok()
}
```
- Text editors (`contenteditable`, bold/colour buttons, find and replace): topic
  `rich-text`.
- Promises (`write_text`, `read_text`) become futures with `JsFuture::from(…)`;
  await them inside `spawn_local`.
- A general `Element`/`EventTarget` becomes a specific one with
  `dyn_into::<web_sys::HtmlInputElement>()` (`use wasm_bindgen::JsCast;`).
- Not `document.execCommand("copy")`: it is old and needs `HtmlDocument`; use the
  clipboard as above.
- Dates: topic `date-time`. `js_sys::Date::to_date_string()` returns a `JsString`
  (`String::from(…)`), not a `Result`.
- Files dragged in from the computer and the system's dialogs belong to the real
  window, not the screens (topic `overview`).
