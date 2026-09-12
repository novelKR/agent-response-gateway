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

/// Numeric accounting and allowlisted identities are independent of replay data.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounting {
    pub usage: gateway_usage_contract::CanonicalUsage,
    pub model: Option<String>,
    pub response_id: Option<String>,
    pub upstream: gateway_usage_contract::Outcome,
}
impl Accounting {
    pub fn new(
        profile: gateway_usage_contract::Profile,
        usage: Option<&Value>,
        metadata: &Value,
        upstream: gateway_usage_contract::Outcome,
    ) -> Self {
        let empty = serde_json::json!({});
        Self {
            usage: gateway_usage_contract::normalize(
                profile,
                gateway_usage_contract::extract(
                    profile,
                    usage.filter(|v| !v.is_null()).unwrap_or(&empty),
                ),
            ),
            model: metadata
                .get("model")
                .and_then(Value::as_str)
                .filter(|v| gateway_usage_contract::safe_label(v))
                .map(str::to_owned),
            response_id: metadata
                .get("id")
                .and_then(Value::as_str)
                .filter(|v| gateway_usage_contract::safe_label(v))
                .map(str::to_owned),
            upstream,
        }
    }
}
pub(crate) struct ManagedOutput {
    pub response: Value,
    pub native: NativeReplay,
    pub outcome: Outcome,
    pub usage: Value,
    pub accounting: Accounting,
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
fn interaction_output(
    value: DecodedInteraction,
    accounting: Accounting,
) -> Result<ManagedOutput, IrError> {
    let outcome = match value.provider_status.as_str() {
        "completed" => Outcome::Completed,
        "requires_action" => Outcome::AwaitingTools,
        _ => return Err(IrError::InvalidEventOrder),
    };
    Ok(ManagedOutput {
        usage: accounting.usage.responses(),
        accounting,
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
    pub fn usage_profile(&self) -> gateway_usage_contract::Profile {
        match self {
            Self::Gemini(_) => gateway_usage_contract::Profile::GeminiInteractionsV1,
            Self::Messages(_) => gateway_usage_contract::Profile::MessagesV1,
            Self::Chat(p) => p.accounting_profile(),
        }
    }
    pub fn payload(&self) -> &Value {
        match self {
            Self::Gemini(p) => &p.payload,
            Self::Messages(p) => &p.payload,
            Self::Chat(p) => &p.payload,
        }
    }
    pub fn decode_bytes(&self, bytes: &[u8], id: &str) -> Result<ManagedOutput, IrError> {
        let output = match self {
            Self::Gemini(p) => {
                let raw: Value = super::json::decode(bytes)?;
                let accounting = Accounting::new(
                    gateway_usage_contract::Profile::GeminiInteractionsV1,
                    raw.get("usage"),
                    &raw,
                    gateway_usage_contract::Outcome::Completed,
                );
                interaction_output(p.decode(raw, id)?, accounting)
            }
            Self::Messages(p) => p.decode_managed(super::json::decode(bytes)?),
            Self::Chat(p) => p.decode_managed(super::json::decode(bytes)?),
        }?;
        checked_usage(output)
    }
    pub fn stream(&self, limit: usize, id: String) -> ManagedStream<'_> {
        let native = match self {
            Self::Gemini(p) => NativeManagedStream::Gemini(p.stream(limit, id)),
            Self::Messages(p) => NativeManagedStream::Messages(NativeMessagesStream::new(p, limit)),
            Self::Chat(p) => NativeManagedStream::Chat(NativeChatStream::new(p, limit)),
        };
        ManagedStream {
            native,
            counters: gateway_usage_contract::Accumulator::new(self.usage_profile()),
            invalid: false,
        }
    }
}
fn checked_usage(mut output: ManagedOutput) -> Result<ManagedOutput, IrError> {
    if !output.accounting.usage.violations.is_empty() {
        return Err(IrError::InvalidField("usage"));
    }
    output.usage = output.accounting.usage.responses();
    output.response["usage"] = output.usage.clone();
    Ok(output)
}
pub(crate) struct ManagedStream<'a> {
    native: NativeManagedStream<'a>,
    counters: gateway_usage_contract::Accumulator,
    invalid: bool,
}
enum NativeManagedStream<'a> {
    Gemini(InteractionsStream<'a>),
    Messages(NativeMessagesStream<'a>),
    Chat(NativeChatStream<'a>),
}
impl ManagedStream<'_> {
    pub fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        if self.invalid {
            return Err(IrError::InvalidEventOrder);
        }
        let result = match &mut self.native {
            NativeManagedStream::Gemini(s) => s.event(event),
            NativeManagedStream::Messages(s) => s.event(event),
            NativeManagedStream::Chat(s) => s.event(event),
        };
        if result.is_err() {
            self.invalid = true;
            return result;
        }
        self.counters
            .observe_counters(self.accounting().usage.reported);
        // Partial snapshots can update related counters in separate events.
        // Reject malformed/decreasing reported values now; validate cross-counter
        // relationships against the assembled terminal snapshot in finish().
        if self.counters.incomplete
            || self
                .counters
                .usage
                .reported
                .values()
                .any(|c| c.source == gateway_usage_contract::Source::Invalid)
        {
            self.invalid = true;
            return Err(IrError::InvalidField("usage"));
        }
        Ok(())
    }
    pub fn accounting(&self) -> Accounting {
        match &self.native {
            NativeManagedStream::Gemini(s) => s.accounting(),
            NativeManagedStream::Messages(s) => s.accounting(),
            NativeManagedStream::Chat(s) => s.accounting(),
        }
    }
    pub fn take_progress(&mut self) -> Vec<Value> {
        if self.invalid {
            return vec![];
        }
        match &mut self.native {
            NativeManagedStream::Gemini(s) => s.take_progress(),
            NativeManagedStream::Messages(s) => s.take_progress(),
            NativeManagedStream::Chat(s) => s.take_progress(),
        }
    }
    pub fn is_complete(&self) -> bool {
        !self.invalid
            && match &self.native {
                NativeManagedStream::Gemini(s) => s.is_complete(),
                NativeManagedStream::Messages(s) => s.is_complete(),
                NativeManagedStream::Chat(s) => s.is_complete(),
            }
    }
    pub fn finish(self) -> Result<ManagedOutput, IrError> {
        if self.invalid {
            return Err(IrError::InvalidEventOrder);
        }
        let mut output = match self.native {
            NativeManagedStream::Gemini(s) => {
                let accounting = s.accounting();
                interaction_output(s.finish()?, accounting)
            }
            NativeManagedStream::Messages(s) => s.finish(),
            NativeManagedStream::Chat(s) => s.finish(),
        }?;
        output.accounting.usage = self.counters.usage;
        checked_usage(output)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod chat_tests;
