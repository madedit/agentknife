//! The pixels of AgentKnife's own web view, for screenshots of the preview
//! (the preview page is an iframe of that view): the platform's capture call,
//! and cutting the preview's part out of the result.
//!
//! The only AgentKnife crate besides ak-shell's interop that may use `unsafe`
//! (owner, 2026-10-27): the web view APIs are COM (WebView2) and Objective-C
//! (WKWebView) calls. It stays in the platform modules, each block with a
//! `// SAFETY:` comment; everything else here is safe code.

#![deny(unsafe_code)]

mod crop;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod webview2;

pub use crop::{PixelRect, crop, size};

/// Whether this platform's web view can be captured.
pub const AVAILABLE: bool = cfg!(windows);
