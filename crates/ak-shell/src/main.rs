//! Mounts AgentKnife's UI (see the library).

#![deny(unsafe_code)]
// Leptos views nest deeply in types (the chat pane).
#![recursion_limit = "256"]

fn main() {
    leptos::mount::mount_to_body(ak_shell::App);
}
