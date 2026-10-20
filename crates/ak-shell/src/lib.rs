//! AgentKnife's own UI (Leptos, WASM). It runs in the Tauri window and talks to
//! ak-app through Tauri IPC only; `trunk serve` in this folder runs it in a
//! browser.

#![deny(unsafe_code)]

pub mod i18n;
pub mod layout;
mod panes;
pub mod shell;
mod splitter;

use leptos::prelude::*;

/// The whole UI, in the browser's language.
#[component]
pub fn App() -> impl IntoView {
    i18n::provide(i18n::browser_locale());
    view! { <shell::Shell /> }
}
