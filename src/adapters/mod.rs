//! Explicit wire adapters. Library availability does not enable HTTP dispatch.
mod json;
pub mod messages;
pub mod sse;
mod toolset;

pub mod chat;

use crate::ir::{ApiProtocol, IrError, capability::TranslationPlan, request::RequestIR};
use serde_json::Value;
use sse::SseEvent;

/// Dispatch state owns one admitted request's identity bindings for its whole HTTP lifetime.
pub(crate) enum PreparedAdapter {
    Messages(messages::PreparedMessages),
    Chat(chat::PreparedChat),
}
impl PreparedAdapter {
    pub(crate) fn encode(request: &RequestIR, plan: &TranslationPlan) -> Result<Self, IrError> {
        match plan.route.api {
            ApiProtocol::Messages => messages::encode_admitted(request, plan).map(Self::Messages),
            ApiProtocol::ChatCompletions => chat::encode_admitted(request, plan).map(Self::Chat),
            _ => Err(IrError::WrongProtocol),
        }
    }
    pub(crate) fn take_payload(&mut self) -> Value {
        match self {
            Self::Messages(p) => std::mem::take(&mut p.payload),
            Self::Chat(p) => std::mem::take(&mut p.payload),
        }
    }
    pub(crate) fn decode_bytes(&self, bytes: &[u8]) -> Result<Value, IrError> {
        match self {
            Self::Messages(p) => p.decode_bytes(bytes),
            Self::Chat(p) => p.decode_bytes(bytes),
        }
    }
    pub(crate) fn stream(&self, limit: usize) -> Result<ActiveStream<'_>, IrError> {
        match self {
            Self::Messages(p) => p.stream(limit).map(ActiveStream::Messages),
            Self::Chat(p) => p.stream(limit).map(ActiveStream::Chat),
        }
    }
}
pub(crate) enum ActiveStream<'a> {
    Messages(messages::MessagesStream<'a>),
    Chat(chat::ChatStream<'a>),
}
impl ActiveStream<'_> {
    pub(crate) fn event(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        match self {
            Self::Messages(s) => s.event(event),
            Self::Chat(s) => s.event(event),
        }
    }
    pub(crate) fn finish(&self) -> Result<(), IrError> {
        match self {
            Self::Messages(s) => s.finish(),
            Self::Chat(s) => s.finish(),
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Messages(s) => s.is_complete(),
            Self::Chat(s) => s.is_complete(),
        }
    }
}

pub mod interactions;
