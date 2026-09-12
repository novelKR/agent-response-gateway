//! Provider-specific conversion ends here. The executor consumes typed outcomes.
use super::{
    interactions::{DecodedInteraction, InteractionsStream, PreparedInteractions},
    sse::SseEvent,
};
use crate::{
    continuation::{NativeReplay, Outcome},
    ir::{
        IrError, capability::TranslationPlan, continuity::VerifiedProviderHistory,
        request::RequestIR,
    },
};
use serde_json::Value;

pub(crate) struct ManagedOutput {
    pub response: Value,
    pub native: NativeReplay,
    pub outcome: Outcome,
}
fn interaction_output(value: DecodedInteraction) -> Result<ManagedOutput, IrError> {
    let outcome = match value.provider_status.as_str() {
        "completed" => Outcome::Completed,
        "requires_action" => Outcome::AwaitingTools,
        _ => return Err(IrError::InvalidEventOrder),
    };
    Ok(ManagedOutput {
        response: value.response,
        native: NativeReplay::Gemini {
            version: 1,
            steps: value.steps,
        },
        outcome,
    })
}
pub(crate) enum ManagedAdapter {
    Gemini(PreparedInteractions),
}
impl ManagedAdapter {
    pub fn encode(
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &VerifiedProviderHistory,
    ) -> Result<Self, IrError> {
        PreparedInteractions::encode(request, plan, history).map(Self::Gemini)
    }
    pub fn payload(&self) -> &Value {
        match self {
            Self::Gemini(p) => &p.payload,
        }
    }
    pub fn decode_bytes(&self, bytes: &[u8], id: &str) -> Result<ManagedOutput, IrError> {
        match self {
            Self::Gemini(p) => interaction_output(p.decode_bytes(bytes, id)?),
        }
    }
    pub fn stream(&self, limit: usize, id: String) -> ManagedStream<'_> {
        match self {
            Self::Gemini(p) => ManagedStream::Gemini(p.stream(limit, id)),
        }
    }
}
pub(crate) enum ManagedStream<'a> {
    Gemini(InteractionsStream<'a>),
}
impl ManagedStream<'_> {
    pub fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        match self {
            Self::Gemini(s) => s.event(event),
        }
    }
    pub fn take_progress(&mut self) -> Vec<Value> {
        match self {
            Self::Gemini(s) => s.take_progress(),
        }
    }
    pub fn is_complete(&self) -> bool {
        match self {
            Self::Gemini(s) => s.is_complete(),
        }
    }
    pub fn finish(self) -> Result<ManagedOutput, IrError> {
        match self {
            Self::Gemini(s) => interaction_output(s.finish()?),
        }
    }
}
