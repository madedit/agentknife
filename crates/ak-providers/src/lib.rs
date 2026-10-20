//! LLM providers: Anthropic, OpenAI and OpenAI-compatible endpoints.
//!
//! Every provider implements [`Provider`] and speaks the neutral
//! [`ak_types::chat`] schema. Providers talk HTTP only through [`HttpTransport`],
//! so tests replay recorded [`fixture`]s and never touch the network.

#![forbid(unsafe_code)]

mod assemble;
mod error;
pub mod fixture;
mod provider;
pub mod sse;
mod transport;

pub use assemble::{Assembled, MessageAssembler, collect};
pub use error::ProviderError;
pub use provider::{
    Capabilities, ChatRequest, EventStream, ModelInfo, Provider, StopReason, StreamEvent, Usage,
};
pub use transport::{ByteStream, HttpRequest, HttpResponse, HttpTransport, Method};
