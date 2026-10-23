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
  a closure moves a `String` out each time it runs. Inside `Show`/`For` rows:
  topic `leptos-components` ("Lists"). One handler for two events: a `Callback`,
  called with `.run(…)`.
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
- **`type mismatch in closure arguments`** on `Button`'s `on_click` — write
  `move |_| …`; `move |()| …` is for `Form`'s `on_submit`.

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
