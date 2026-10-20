//! Embedded in generated apps (dev builds): WebSocket invoke, inspector and control channel.
//!
//! Generated apps call `start()` and `invoke` in their frontend and register `init` in their
//! Tauri backend. In release builds (`dev` feature off) `invoke` is plain Tauri IPC
//! and the plugin does nothing: the bridge server and client are not compiled in.

#![forbid(unsafe_code)]

pub mod protocol;

#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(all(not(target_arch = "wasm32"), feature = "dev"))]
pub use host::server;
#[cfg(not(target_arch = "wasm32"))]
pub use host::{init, is_preview};

#[cfg(target_arch = "wasm32")]
mod client;
#[cfg(target_arch = "wasm32")]
pub use client::{HitTestInput, InvokeError, invoke, invoke_value, set_hit_test, start};
#[cfg(all(target_arch = "wasm32", feature = "dev"))]
mod inspector;

/// Whether the preview bridge is compiled in (the `dev` feature).
pub const ENABLED: bool = cfg!(feature = "dev");

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "dev"))]
    #[test]
    fn release_builds_have_no_bridge() {
        assert!(!super::is_preview());
    }
}
