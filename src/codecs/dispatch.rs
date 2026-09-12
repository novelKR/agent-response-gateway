//! Selection is explicit and fixed for the complete request. No fallback or retry.
use super::{
    Binding,
    execution::{CodecOutput, CodecStream, PreparedCodec},
};
use crate::{
    adapters::{
        ActiveStream, PreparedAdapter,
        managed::{Accounting, ManagedAdapter, ManagedOutput, ManagedStream},
        sse::SseEvent,
    },
    ir::{
        IrError, capability::TranslationPlan, continuity::VerifiedProviderHistory,
        request::RequestIR,
    },
};
use serde_json::Value;

pub(crate) enum Dispatch {
    Builtin(Box<PreparedAdapter>),
    External(Box<PreparedCodec>),
}
impl Dispatch {
    pub(crate) async fn prepare(
        binding: Option<&Binding>,
        request: &RequestIR,
        plan: &TranslationPlan,
        maximum: usize,
        profile: gateway_usage_contract::Profile,
    ) -> Result<Self, IrError> {
        if let Some(binding) = binding {
            PreparedCodec::prepare(
                binding,
                request,
                plan,
                &VerifiedProviderHistory::default(),
                None,
                maximum,
                profile,
            )
            .await
            .map(|p| Self::External(Box::new(p)))
        } else {
            PreparedAdapter::encode(request, plan).map(|p| Self::Builtin(Box::new(p)))
        }
    }
    pub(crate) fn take_payload(&mut self) -> Value {
        match self {
            Self::Builtin(p) => p.take_payload(),
            Self::External(p) => std::mem::take(&mut p.payload),
        }
    }
    pub(crate) async fn decode_bytes(&mut self, body: &[u8]) -> Result<Value, IrError> {
        match self {
            Self::Builtin(p) => p.decode_bytes(body),
            Self::External(p) => match p.json(body, "").await? {
                CodecOutput::Stateless(v) => Ok(v),
                _ => Err(IrError::InvalidEventOrder),
            },
        }
    }
    pub(crate) async fn stream(&mut self, maximum: usize) -> Result<Stream<'_>, IrError> {
        match self {
            Self::Builtin(p) => p.stream(maximum).map(|s| Stream::Builtin(Box::new(s))),
            Self::External(p) => p
                .stream(String::new())
                .await
                .map(|s| Stream::External(Box::new(s))),
        }
    }
}
pub(crate) enum Stream<'a> {
    Builtin(Box<ActiveStream<'a>>),
    External(Box<CodecStream<'a>>),
}
impl Stream<'_> {
    pub(crate) async fn event(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        match self {
            Self::Builtin(s) => s.event(event),
            Self::External(s) => s.event(event).await,
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Builtin(s) => s.is_complete(),
            Self::External(s) => s.is_complete(),
        }
    }
    pub(crate) fn gates_tool_completion(&self) -> bool {
        match self {
            Self::Builtin(s) => s.gates_tool_completion(),
            Self::External(_) => true,
        }
    }
    pub(crate) fn finish(&self) -> Result<(), IrError> {
        match self {
            Self::Builtin(s) => s.finish(),
            Self::External(s) if s.is_complete() => Ok(()),
            _ => Err(IrError::InvalidEventOrder),
        }
    }
}

pub(crate) enum ManagedDispatch {
    Builtin(Box<ManagedAdapter>),
    External(Box<PreparedCodec>, gateway_usage_contract::Profile),
}
impl ManagedDispatch {
    pub(crate) async fn prepare(
        binding: Option<&Binding>,
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &VerifiedProviderHistory,
        pending: bool,
        maximum: usize,
        profile: gateway_usage_contract::Profile,
    ) -> Result<Self, IrError> {
        if let Some(binding) = binding {
            Ok(Self::External(
                Box::new(
                    PreparedCodec::prepare(
                        binding,
                        request,
                        plan,
                        history,
                        Some(pending),
                        maximum,
                        profile,
                    )
                    .await?,
                ),
                profile,
            ))
        } else {
            let p = ManagedAdapter::encode(request, plan, history)?;
            if pending {
                p.validate_pending_controls(history)?;
            }
            Ok(Self::Builtin(Box::new(p)))
        }
    }
    pub(crate) fn usage_profile(&self) -> gateway_usage_contract::Profile {
        match self {
            Self::Builtin(p) => p.usage_profile(),
            Self::External(_, profile) => *profile,
        }
    }
    pub(crate) fn payload(&self) -> &Value {
        match self {
            Self::Builtin(p) => p.payload(),
            Self::External(p, _) => &p.payload,
        }
    }
    pub(crate) async fn decode_bytes(
        &mut self,
        body: &[u8],
        id: &str,
    ) -> Result<ManagedOutput, IrError> {
        match self {
            Self::Builtin(p) => p.decode_bytes(body, id),
            Self::External(p, _) => match p.json(body, id).await? {
                CodecOutput::Managed(v) => Ok(*v),
                _ => Err(IrError::InvalidEventOrder),
            },
        }
    }
    pub(crate) async fn stream(
        &mut self,
        maximum: usize,
        id: String,
    ) -> Result<Managed<'_>, IrError> {
        match self {
            Self::Builtin(p) => Ok(Managed::Builtin(Box::new(p.stream(maximum, id)))),
            Self::External(p, _) => p
                .stream(id)
                .await
                .map(|s| Managed::External(Box::new(s), vec![])),
        }
    }
}
pub(crate) enum Managed<'a> {
    Builtin(Box<ManagedStream<'a>>),
    External(Box<CodecStream<'a>>, Vec<Value>),
}
impl Managed<'_> {
    pub(crate) async fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        match self {
            Self::Builtin(s) => s.event(event),
            Self::External(s, events) => {
                *events = s.event(event).await?;
                Ok(())
            }
        }
    }
    pub(crate) fn accounting(&self) -> Accounting {
        match self {
            Self::Builtin(s) => s.accounting(),
            Self::External(s, _) => s.accounting().expect("verified event accounting").clone(),
        }
    }
    pub(crate) fn take_progress(&mut self) -> Vec<Value> {
        match self {
            Self::Builtin(s) => s.take_progress(),
            Self::External(_, events) => std::mem::take(events),
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Builtin(s) => s.is_complete(),
            Self::External(s, _) => s.is_complete(),
        }
    }
    pub(crate) async fn finish(self) -> Result<ManagedOutput, IrError> {
        match self {
            Self::Builtin(s) => s.finish(),
            Self::External(s, _) => s.finish().await,
        }
    }
}
