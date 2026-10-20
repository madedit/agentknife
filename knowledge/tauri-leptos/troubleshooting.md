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

## Tauri backend
- **`command X not found`** at runtime — add it to `tauri::generate_handler![…]`.
- **`missing required key itemId`** — the screen sent the wrong argument name: use
  camelCase on the screen side (topic `ak-bridge`).
- **`state not managed for field`** — add `.manage(MyState::default())` to the builder.
- **`no method named path`** — add `use tauri::Manager;`.

## Trunk / Tailwind
- **A Tailwind class has no effect** — the full class name must appear in the code
  (topic `tailwind`); or a typo in the class.
- **`wasm-bindgen` version mismatch** — do not change `wasm-bindgen` in Cargo.toml.
- **A crate fails for `wasm32-unknown-unknown`** — it needs the operating system
  (files, threads, network): move that work to a backend command.

## When stuck
Undo to the last working checkpoint rather than piling up fixes, then make a smaller
change.
