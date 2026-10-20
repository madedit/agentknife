//! Mounts AgentKnife's UI (see the library).

#![deny(unsafe_code)]

fn main() {
    leptos::mount::mount_to_body(ak_shell::App);
}
