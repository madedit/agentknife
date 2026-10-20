//! AgentKnife's own UI (Leptos, WASM). It runs in the Tauri window and talks to
//! ak-app through Tauri IPC only; `trunk serve` in this folder runs it in a
//! browser.

#![deny(unsafe_code)]

pub mod app;
pub mod backend;
pub mod connect;
pub mod home;
pub mod i18n;
pub mod layout;
mod panes;
pub mod shell;
mod splitter;

pub use app::App;
