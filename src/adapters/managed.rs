//! Provider-specific conversion ends here. The executor consumes typed outcomes.
use super::{
    chat::{PreparedChat, managed_stream::NativeChatStream},
    interactions::{DecodedInteraction, InteractionsStream, PreparedInteractions},
    messages::{PreparedMessages, managed_stream::NativeMessagesStream},
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
    pub usage: Value,
}
impl ManagedOutput {
    /// Retain numeric metadata separately. Codex's optional usage object requires
    /// complete core counters, and its detail objects require non-null integers.
    pub fn project_usage(&mut self) {
        let mut usage = self.usage.clone();
        if ["input_tokens", "output_tokens", "total_tokens"]
            .iter()
            .any(|key| !usage[key].is_u64())
        {
            self.response["usage"] = Value::Null;
            return;
        }
        for (field, counter) in [
            ("input_tokens_details", "cached_tokens"),
            ("output_tokens_details", "reasoning_tokens"),
        ] {
            if !usage[field][counter].is_u64() {
                usage.as_object_mut().expect("usage object").remove(field);
            }
        }
        self.response["usage"] = usage;
    }
}
fn interaction_output(value: DecodedInteraction) -> Result<ManagedOutput, IrError> {
    let outcome = match value.provider_status.as_str() {
        "completed" => Outcome::Completed,
        "requires_action" => Outcome::AwaitingTools,
        _ => return Err(IrError::InvalidEventOrder),
    };
    Ok(ManagedOutput {
        usage: value.response["usage"].clone(),
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
    Messages(PreparedMessages),
    Chat(PreparedChat),
}
impl ManagedAdapter {
    pub fn encode(
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &VerifiedProviderHistory,
    ) -> Result<Self, IrError> {
        match plan.route.api {
            crate::ir::ApiProtocol::GeminiInteractions => {
                PreparedInteractions::encode(request, plan, history).map(Self::Gemini)
            }
            crate::ir::ApiProtocol::Messages => {
                super::messages::encode_with_history(request, plan, history, true)
                    .map(Self::Messages)
            }
            crate::ir::ApiProtocol::ChatCompletions => {
                super::chat::encode_with_history(request, plan, history, true).map(Self::Chat)
            }
            _ => Err(IrError::WrongProtocol),
        }
    }
    pub fn validate_pending_controls(
        &self,
        history: &VerifiedProviderHistory,
    ) -> Result<(), IrError> {
        if let Self::Chat(p) = self {
            let Some((_, native)) = history.segments.values().last() else {
                return Err(IrError::ContinuityMismatch);
            };
            let NativeReplay::Chat { controls, .. } = native else {
                return Err(IrError::ContinuityMismatch);
            };
            if p.reasoning_controls.as_ref() != Some(controls) {
                return Err(IrError::ContinuityMismatch);
            }
        }
        if let Self::Messages(p) = self {
            let Some((_, native)) = history.segments.values().last() else {
                return Err(IrError::ContinuityMismatch);
            };
            let NativeReplay::Messages { controls, .. } = native else {
                return Err(IrError::ContinuityMismatch);
            };
            if p.reasoning_controls.as_ref() != Some(controls) {
                return Err(IrError::ContinuityMismatch);
            }
        }
        Ok(())
    }
    pub fn payload(&self) -> &Value {
        match self {
            Self::Gemini(p) => &p.payload,
            Self::Messages(p) => &p.payload,
            Self::Chat(p) => &p.payload,
        }
    }
    pub fn decode_bytes(&self, bytes: &[u8], id: &str) -> Result<ManagedOutput, IrError> {
        match self {
            Self::Gemini(p) => interaction_output(p.decode_bytes(bytes, id)?),
            Self::Messages(p) => p.decode_managed(super::json::decode(bytes)?),
            Self::Chat(p) => p.decode_managed(super::json::decode(bytes)?),
        }
    }
    pub fn stream(&self, limit: usize, id: String) -> ManagedStream<'_> {
        match self {
            Self::Gemini(p) => ManagedStream::Gemini(p.stream(limit, id)),
            Self::Messages(p) => ManagedStream::Messages(NativeMessagesStream::new(p, limit)),
            Self::Chat(p) => ManagedStream::Chat(NativeChatStream::new(p, limit)),
        }
    }
}
pub(crate) enum ManagedStream<'a> {
    Gemini(InteractionsStream<'a>),
    Messages(NativeMessagesStream<'a>),
    Chat(NativeChatStream<'a>),
}
impl ManagedStream<'_> {
    pub fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        match self {
            Self::Gemini(s) => s.event(event),
            Self::Messages(s) => s.event(event),
            Self::Chat(s) => s.event(event),
        }
    }
    pub fn take_progress(&mut self) -> Vec<Value> {
        match self {
            Self::Gemini(s) => s.take_progress(),
            Self::Messages(s) => s.take_progress(),
            Self::Chat(s) => s.take_progress(),
        }
    }
    pub fn is_complete(&self) -> bool {
        match self {
            Self::Gemini(s) => s.is_complete(),
            Self::Messages(s) => s.is_complete(),
            Self::Chat(s) => s.is_complete(),
        }
    }
    pub fn finish(self) -> Result<ManagedOutput, IrError> {
        match self {
            Self::Gemini(s) => interaction_output(s.finish()?),
            Self::Messages(s) => s.finish(),
            Self::Chat(s) => s.finish(),
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod chat_tests;
