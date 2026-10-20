//! AgentKnife's own UI (Leptos, WASM). It runs in the Tauri window and talks to
//! ak-app through Tauri IPC only; `trunk serve` in this folder runs it in a
//! browser.

#![deny(unsafe_code)]
// Leptos views nest deeply in types (the chat pane).
#![recursion_limit = "256"]

pub mod app;
pub mod backend;
pub mod chat;
pub mod connect;
mod demo;
pub mod files;
pub mod home;
pub mod i18n;
pub mod layout;
// CodeMirror glue: the one module allowed unsafe (wasm-bindgen imports).
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod interop;
pub mod preview;
pub mod settings;
pub mod shell;
mod splitter;

pub use app::App;
