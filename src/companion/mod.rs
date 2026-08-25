//! OpenAI-compatible `/v1` companion client, packing, and chat loop.

use crate::text;

mod cancel;
mod chat;
mod client;
mod pack;
mod session;
mod sse;
mod tools;
mod types;

#[cfg(test)]
mod loop_tests;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod pins;

pub use cancel::Cancellation;
pub use chat::run_chat;
pub use client::{chat_completions_url, CompanionClient, CONNECT_TIMEOUT, REQUEST_TIMEOUT};
pub use pack::{estimate_tokens, pack_messages, NoopRecall, RecallInjector};
pub use session::Session;
pub use tools::{NoopExecutor, ToolExecutor, ToolSpec};
pub use types::{Finish, Message, Role, ToolCall};

#[derive(Debug)]
pub enum CompanionError {
    Unreachable,
    Timeout,
    HttpStatus,
    Cancelled,
    InvalidResponse,
    TurnTooLarge,
    Runtime,
    Io,
    ToolLoop,
}

impl std::fmt::Display for CompanionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self {
            Self::Unreachable => "companion.unreachable",
            Self::Timeout => "companion.timeout",
            Self::HttpStatus => "companion.http_status",
            Self::Cancelled => "companion.cancelled",
            Self::InvalidResponse => "companion.invalid_response",
            Self::TurnTooLarge => "companion.turn_too_large",
            Self::Runtime => "companion.runtime_failed",
            Self::Io => "companion.io_failed",
            Self::ToolLoop => "companion.tool_loop",
        };
        f.write_str(text::get(key))
    }
}

impl std::error::Error for CompanionError {}
