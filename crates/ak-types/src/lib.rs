//! Shared DTOs (IPC payloads, events), i18n API and the `StackProfile` trait. Bottom layer; native + wasm32.

#![forbid(unsafe_code)]

pub mod agent;
pub mod chat;
pub mod doctor;
pub mod i18n;
#[cfg(not(target_arch = "wasm32"))]
pub mod ids;
pub mod ipc;
pub mod preview;
pub mod settings;
pub mod stack;
