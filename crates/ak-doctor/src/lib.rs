//! Toolchain detection and isolated install. Deterministic; never calls an LLM.

#![forbid(unsafe_code)]

mod detect;
pub mod probe;
mod toolchain;

pub use detect::{Platform, check, parse_version};
pub use toolchain::{Pinned, Toolchain};
