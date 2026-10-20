//! Toolchain detection and isolated install. Deterministic; never calls an LLM.

#![forbid(unsafe_code)]

pub mod assets;
mod detect;
pub mod fetch;
pub mod install;
pub mod probe;
mod toolchain;

pub use detect::{Platform, check, parse_version};
pub use install::Installer;
pub use toolchain::{Pinned, Toolchain};
