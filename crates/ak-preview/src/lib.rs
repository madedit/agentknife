//! The live preview: runs a project's dev server and app backend from its stack's
//! `PreviewPlan` and reports their state ([`dev::Preview`]); plus the stack
//! profiles ([`profiles`]).

#![forbid(unsafe_code)]

pub mod control;
pub mod dev;
pub mod ids;
pub mod log;
pub mod process;
pub mod profiles;
pub mod session;

pub use dev::Preview;
pub use process::Tools;
pub use session::{Capturer, Image, PreviewSession};
