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
- **`type mismatch in closure arguments`** on `Button`'s `on_click` — write
  `move |_| …`; `move |()| …` is for `Form`'s `on_submit`.

## Tauri backend
- **`command X not found`** at runtime — add it to `tauri::generate_handler![…]`.
- **`missing required key itemId`** — the screen sent the wrong argument name: use
  camelCase on the screen side (topic `ak-bridge`).
- **`state not managed for field`** — add `.manage(MyState::default())` to the builder.
- **`no method named path`** — add `use tauri::Manager;`.

## Builds, but the page fails
Call `dom_summary`: its last part lists the page's errors.
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
- **A Tailwind class has no effect** — the full class name must appear in the code
  (topic `tailwind`); or a typo in the class.
- **`wasm-bindgen` version mismatch** — do not change `wasm-bindgen` in Cargo.toml.
- **A crate fails for `wasm32-unknown-unknown`** — it needs the operating system
  (files, threads, network): move that work to a backend command.

## When stuck
Undo to the last working checkpoint rather than piling up fixes, then make a smaller
change.
