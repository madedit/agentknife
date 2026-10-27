# Fixing common build errors

Read the first error; later ones often follow from it. After a fix, check the preview
builds again.

## Rust and Leptos
- **`cannot find function/struct X`** — missing `use` (`use leptos::prelude::*;`,
  `use ak_ui::*;`, `use serde::{Deserialize, Serialize};`) or a typo.
- **`use of moved value` / `value moved into closure`** — a `String` or `Vec` is
  captured by two `move` closures. Clone it before each closure, or store it in a
  signal (signals are `Copy`).
- **`the trait bound … IntoView is not satisfied` / `if and else have incompatible
  types`** — branches return different views: end each branch with `.into_any()`.
- **A wall of `HtmlElement<…>: IntoView`, `ShowPropsBuilder` / `ForPropsBuilder`,
  `Render`, `RenderHtml`, `ReactiveFunction` bounds** for a whole `view!` — the real
  error is inside the children. Look for: branches of an `if`/`match` returning
  different views (`.into_any()`), `each=` without a closure, a handler with the
  wrong event type (`type mismatch in closure arguments`, usually listed too).
  Still unclear: move the children into a small component of their own; the error
  then points at the line.
- **`invalid tag name or attribute key` + `tokens was ignored during parsing`** on
  `class:bg-[#…]` — `class:` takes plain class names only: return the whole class
  string from a closure (topic `leptos-components`).
- **`type annotations needed: multiple impls satisfying _: From<&str>`** on a prop —
  drop the `.into()`: `name="marquee"` (the prop has `#[prop(into)]`).
- **`the trait bound …: Eq` / `Hash is not satisfied`** on `<For key=…>` — key by a
  number or a `String`, not by a struct.
- **`closure invoked recursively or after being dropped`** in the page's errors —
  a part was removed while one of its handlers ran (a menu in `Show` that its own
  item closes): keep it on the page and hide it with `class:hidden`.
- **`unterminated raw string` (E0748)** — the closing `"#` of an `r#"…"#` is
  missing or misplaced (long SVG or HTML text): check it ends with `"#`.
- **`expected a closure … found …` on an attribute** — reactive attributes need a
  closure: `disabled=move || busy.get()`.
- **The screen does not update** — a value was read once: wrap it, `{move || x.get()}`.
- **`… cannot be sent between threads safely`** — reactive closures must be
  `Send + Sync`: no `Rc`/`RefCell` in them; use signals.
- **`mismatched types: expected String, found &str`** — `"text".to_string()`, or give
  the prop `#[prop(into)]`.
- **`this function takes N arguments`** on a component — check prop names; optional
  props need `#[prop(optional)]`.
- **`invalid tag name or attribute key`** at a `<` or `>` inside `view!`, or
  **`expected bool, found f64`** on `<Show when=…>` — an attribute value compares
  something: put it in braces, `when={move || total.get() > 0.0}` (topic
  `leptos-components`).
- **`the trait bound … Ord is not satisfied`** on `.sort()` of `f64` values — numbers with a
  fraction cannot `sort()`: `v.sort_by(|a, b| a.1.total_cmp(&b.1))`.
- **`use of moved value`** for a `String` shown in `view!` after another use —
  `.clone()` it for the earlier use, or show `{text.clone()}`.
- **`mismatched types: expected &AppHandle, found AppHandle`** — pass a reference: `notes_path(&app)`.
- **`expected a closure that implements the Fn trait, but this closure only
  implements FnOnce`**, often pointing at the start or end of a whole `view!` —
  a closure moves a `String` out each time it runs, or one `String` is used by
  several `move ||` closures (`class:x={move || …}`, `when`): make it a
  `StoredValue` or clone it per closure. A handler inside `<Show>` (or `For`
  rows) that uses a `String` from outside, like `on:click=move |_|
  open(path.clone())`: `<Show>` runs its children again each time, so turn the
  value into a `StoredValue` first (topic `leptos-components`, "Lists"). One
  handler for two events: a `Callback`, called with `.run(…)`.
- **Paths look like `\\?\C:\Users\…`** on Windows — `std::fs::canonicalize` adds
  that prefix, which users cannot read and which breaks splitting the path into
  parts. For paths the user sees, use `std::path::absolute(&p)` (no prefix), or
  remove it: `let s = p.to_string_lossy(); s.strip_prefix(r"\\?\").unwrap_or(&s)`.
- **`expected Callback<()>, found closure`** on your own component — give the prop
  `#[prop(into)]` (topic `leptos-components`).
- **`cannot assign to *n, which is behind a & reference`** in `with_value`, or
  **`expected (), found u64`** from `update_value` — change a `StoredValue` with
  `update_value(|n| *n += 1)`, then read it with `get_value()`.
- **`no method named data_transfer found for DragEvent`** — add the web-sys
  features `DragEvent` and `DataTransfer` (topic `drag-drop`).
- **Dragging does nothing in the installed app on Windows** — `"dragDropEnabled":
  false` in `tauri.conf.json` (topic `drag-drop`).
- **`expected Option<()>, found ()`** pointing into `effect_function.rs` — `.run(…)`
  called on a plain closure: call it as `f(x)` (only a `Callback` has `.run`).
- **`IntoReactiveValue<Callback<MouseEvent>, _> is not satisfied`** — an ak-ui
  event prop got a closure that returns a value: end it with `;`,
  `move |_| { save(); }` (topic `ak-ui`).
- **`type annotations needed`** on a method of a `NodeRef`'s element — name the
  element: `NodeRef::<leptos::html::Div>::new()`.
- **`no method named append_data found for Node`** (or another web-sys method
  "not found") — it lives on a more exact type: `dyn_ref::<web_sys::Text>()`,
  `dyn_into::<web_sys::HtmlDocument>()` (topic `browser`), or its feature is
  missing.
- **`spawn_local` not found** (in this scope, or in `leptos`) — it is
  `leptos::task::spawn_local(async move { … })`; **`{async closure}` is not a
  future** — pass an `async move { … }` block, not `async move || …`.
- **`cannot find value usize`** — a type where a value belongs:
  `RwSignal::new((0usize, 0usize))`, not `(0usize, usize)`.
- **`no method named take_value`**, or **`the ? operator can only be used in a
  closure that returns Result or Option`** inside `update_value` — use
  `try_update_value(|v| …)`, which returns what the closure returns (topic
  `leptos-reactivity`).
- **E0204 `this field does not implement Copy`** on `#[derive(Copy)]` — a `Cell`,
  `String` or `Vec` field: make it a signal or drop `Copy`.
- **`StoredValue<…>: Send` / `Sync` is not satisfied** for a canvas context or a
  JS `Closure` — `StoredValue::new_local(…)`.
- **A window listener stops working, with no error** — it was added with
  `Closure::wrap` and the `Closure` was dropped: use `window_event_listener` and
  `on_cleanup(move || handle.remove())` (topic `leptos-components`).
- **`expected &PointerEvent, found &MouseEvent`** — `on:dblclick` and `on:click`
  give a `MouseEvent`: a helper used by both takes `&web_sys::MouseEvent` (a
  `PointerEvent` derefs to it: `&*e`).
- **`expected i32, found f64`** on `set_scroll_left` / `set_scroll_top` — they
  take whole pixels: `x as i32`.
- **`no method named clipboard_data` / `key` / … found** — a web-sys feature is
  missing; the message does not say which: it is named after the type that has
  the method (`ClipboardEvent`, `DataTransfer`, …; topic `browser`).
- **Many errors at once** — `build_status` shows the first ones in full and lists
  the rest by file: fix them file by file in one pass, then call `build_status`
  again, rather than one error per build.
- **`cannot find module or crate ev`** — `use leptos::ev;`, or leave the event's
  type out: `move |_| …`.
- **`expected function, found macro format`**, or a string with `` ` `` and
  `${x}` — that is JavaScript: Rust writes `format!("{x} points")`.
- **A black 3D canvas without errors** — it has no height: `dom_summary` shows
  `size=…x0`. The height comes from `h-screen` on the outer element through
  `flex flex-col` and `min-h-0 flex-1` on every element in between (`ToastHost`
  adds no element; your own wrapper `div`s do) (topic `three-d`).
- **`type mismatch in closure arguments`** on `Button`'s `on_click` — write
  `move |_| …`; `move |()| …` is for `Form`'s `on_submit`.

- **The page panics about a missing context (`Toaster`)** — `use_toaster()` was
  called outside a component's body or outside `ToastHost`: pass the `Toaster` in
  (topic `ak-ui`).
- **The page scrolls although the wheel handler calls `prevent_default`** — add
  the listener yourself with `passive: false` (topic `canvas`).
- **A dragged box stops or sticks to the mouse** — `set_pointer_capture` on the
  stage at `pointerdown` (topic `canvas`).
- **The screen freezes or updates without end** after adding an `Effect` — two
  effects write each other's values: write only on a real change (topic
  `leptos-reactivity`).
- **Delete in a text field deletes the selected item** — shortcut keys must skip
  fields (`is_typing`, topic `canvas`).
- **The page's title changes or an SVG part does not show** after adding
  `<title>` inside `<svg>` — `view!` makes `title` the HTML title: write the text
  on the chart instead (topic `reports`).
- **`the trait bound …: PartialEq is not satisfied`** on `Memo::new` — derive
  `PartialEq` on the rows' type (topic `long-lists`).
- **Imported Chinese text is garbage** — the CSV is Big5 (Excel's plain "CSV"):
  decode it (topic `import-backup`).
- **`no method named notification` / `autolaunch`** — add the plugin's `use`
  (`NotificationExt`, `ManagerExt`) and its `.plugin(…)` (topic `desktop`).
- **`could not find tray in tauri`** — add the `tray-icon` feature to tauri
  (topic `desktop`).

## Tauri backend
- **`command X not found`** at runtime — add it to `tauri::generate_handler![…]`.
- **`missing required key itemId`** — the screen sent the wrong argument name: use
  camelCase on the screen side (topic `ak-bridge`).
- **`state not managed for field`** — add `.manage(MyState::default())` to the builder.
- **`no method named path`** — add `use tauri::Manager;`.
- **`no method named save_to` / `thumbnail` takes 2 arguments** (`image`), **`no
  method named with_title` / `display` on `FilePath`** (dialogs) — topic
  `images-dialogs`.

## Builds, but the page fails
Call `dom_summary`: its last part lists the page's errors. While the app is being
rebuilt the preview may briefly fail (the backend restarts): check again after the
build instead of changing code.
- **`byte index N is out of bounds` / `is not a char boundary`** (a panic) — a
  position found in one string (often a lowercased copy) was used on another, or
  cut through a Chinese character: search the original text (topic `rich-text`).
- **`Rust panic: panicked at src/….rs:LINE:COL: …`** — the screens' Rust code
  crashed at that line (often `unwrap()`, `expect()` or an index out of range).
  Handle the `None`/`Err` or check the length instead. The page may stop
  reacting until it reloads.
- **"The backend crashed while running"** (in `build_status` and `dom_summary`) —
  a command panicked at the given `src-tauri/src` line; the screen got a failed
  command. Return `Err("…")` instead of panicking (topic `tauri-commands`).
- **`Command `x` failed: …`** — a backend call failed (wrong argument names,
  the command's own `Err`): see topic `ak-bridge`.
- **`….some_name is not a function`** — Rust calls a JS function by the wrong
  name: rename the JS method to the same snake_case name, or add
  `js_name = someName` to its `#[wasm_bindgen(…)]` (topic `three-d`).
- **`closure invoked recursively or after being dropped`** — usually follows
  another error: fix that one first. Otherwise a `Closure` passed to JS was
  dropped; keep it alive (topic `three-d`).

## Trunk / Tailwind
- **`internal compiler error: … unstable fingerprints`** — the compiler's cache
  tripped, usually next to a real error. Fix the real error and build again; it
  goes away. Do not `cargo clean` (a full rebuild takes many minutes).
- **A Tailwind class has no effect** — the full class name must appear in the code
  (topic `tailwind`); or a typo in the class.
- **`wasm-bindgen` version mismatch** — do not change `wasm-bindgen` in Cargo.toml.
- **A crate fails for `wasm32-unknown-unknown`** — it needs the operating system
  (files, threads, network): move that work to a backend command.

## When stuck
Undo to the last working checkpoint rather than piling up fixes, then make a smaller
change.
