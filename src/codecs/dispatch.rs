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
    provider_plugins::{
        self, AuthorizedProviderHistory, PreparedProvider, ProviderLimits, ProviderObservation,
        ProviderStream,
    },
};
use serde_json::{Value, json};

/// Provider observations stay attached until the host chooses the recording contract.
pub(crate) struct Decoded {
    pub response: Value,
    pub provider_observation: Option<ProviderObservation>,
}
pub(crate) struct Batch {
    pub events: Vec<Value>,
    pub provider_observation: Option<ProviderObservation>,
}
pub(crate) enum Dispatch {
    Builtin(Box<PreparedAdapter>),
    External(Box<PreparedCodec>),
    Provider(Box<PreparedProvider>),
}
impl Dispatch {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn prepare(
        binding: Option<&Binding>,
        provider: Option<&provider_plugins::Binding>,
        request: &RequestIR,
        plan: &TranslationPlan,
        request_maximum: usize,
        maximum: usize,
        profile: Option<gateway_usage_contract::Profile>,
    ) -> Result<Self, IrError> {
        if let Some(provider) = provider {
            if binding.is_some() || profile.is_some() {
                return Err(IrError::UnsupportedFeature);
            }
            return PreparedProvider::prepare(
                provider,
                request,
                plan,
                &AuthorizedProviderHistory::default(),
                None,
                ProviderLimits {
                    request_bytes: request_maximum,
                    output_bytes: maximum,
                },
            )
            .await
            .map(|p| Self::Provider(Box::new(p)));
        }
        let profile = profile.ok_or(IrError::UnsupportedFeature)?;
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
    /// Failure observations remain available for versioned recorder delivery.
    #[allow(dead_code)] // Consumed when provider recording is enabled.
    pub(crate) fn provider_observation(&self) -> Option<&ProviderObservation> {
        match self {
            Self::Provider(provider) => Some(provider.observation()),
            _ => None,
        }
    }
    pub(crate) fn take_payload(&mut self) -> Value {
        match self {
            Self::Builtin(p) => p.take_payload(),
            Self::External(p) => std::mem::take(&mut p.payload),
            Self::Provider(p) => p.take_payload(),
        }
    }
    pub(crate) async fn decode_bytes(
        &mut self,
        body: &[u8],
        response_id: &str,
    ) -> Result<Decoded, IrError> {
        let response = match self {
            Self::Builtin(p) => p.decode_bytes(body)?,
            Self::External(p) => match p.json(body, "").await? {
                CodecOutput::Stateless(v) => v,
                _ => return Err(IrError::InvalidEventOrder),
            },
            Self::Provider(p) => {
                let output = p.json(body, response_id).await?;
                return Ok(Decoded {
                    response: output.response,
                    provider_observation: Some(output.observation),
                });
            }
        };
        Ok(Decoded {
            response,
            provider_observation: None,
        })
    }
    pub(crate) async fn stream(
        &mut self,
        maximum: usize,
        response_id: String,
    ) -> Result<Stream<'_>, IrError> {
        match self {
            Self::Builtin(p) => p.stream(maximum).map(|s| Stream::Builtin(Box::new(s))),
            Self::External(p) => p
                .stream(String::new())
                .await
                .map(|s| Stream::External(Box::new(s))),
            Self::Provider(p) => p.stream(response_id).await.map(|s| Stream::Provider {
                stream: Some(Box::new(s)),
                complete: false,
                sequence: 0,
                maximum,
                emitted_bytes: 0,
                observation: None,
            }),
        }
    }
}
pub(crate) enum Stream<'a> {
    Builtin(Box<ActiveStream<'a>>),
    External(Box<CodecStream<'a>>),
    Provider {
        stream: Option<Box<ProviderStream<'a>>>,
        complete: bool,
        sequence: u64,
        maximum: usize,
        emitted_bytes: usize,
        observation: Option<Box<ProviderObservation>>,
    },
}
impl Stream<'_> {
    pub(crate) async fn event(&mut self, event: SseEvent) -> Result<Batch, IrError> {
        let events = match self {
            Self::Builtin(s) => s.event(event)?,
            Self::External(s) => s.event(event).await?,
            Self::Provider {
                stream,
                complete,
                sequence,
                maximum,
                emitted_bytes,
                observation: last_observation,
            } => {
                if *complete {
                    return Err(IrError::InvalidEventOrder);
                }
                let progress = stream
                    .as_mut()
                    .ok_or(IrError::InvalidEventOrder)?
                    .event(event)
                    .await?;
                *last_observation = Some(Box::new(progress.observation.clone()));
                let mut events = progress.events;
                let mut observation = progress.observation;
                if progress.semantic_complete {
                    let output = stream
                        .as_mut()
                        .ok_or(IrError::InvalidEventOrder)?
                        .finish()
                        .await?;
                    *last_observation = Some(Box::new(output.observation.clone()));
                    stream.take();
                    observation = output.observation;
                    events.extend(output.terminal_events);
                    *complete = true;
                }
                for event in &mut events {
                    event["sequence_number"] = json!(*sequence);
                    *sequence = sequence.checked_add(1).ok_or(IrError::SizeLimit)?;
                    let kind = event["type"].as_str().ok_or(IrError::InvalidEventOrder)?;
                    *emitted_bytes = emitted_bytes
                        .checked_add(event.to_string().len())
                        .and_then(|n| n.checked_add(kind.len() + 16))
                        .ok_or(IrError::SizeLimit)?;
                    if *emitted_bytes > *maximum {
                        return Err(IrError::SizeLimit);
                    }
                }
                return Ok(Batch {
                    events,
                    provider_observation: Some(observation),
                });
            }
        };
        Ok(Batch {
            events,
            provider_observation: None,
        })
    }
    /// A failed exchange does not erase the plugin's numeric interpretation.
    #[allow(dead_code)] // Consumed when provider recording is enabled.
    pub(crate) fn provider_observation(&self) -> Option<&ProviderObservation> {
        match self {
            Self::Provider {
                stream,
                observation,
                ..
            } => stream
                .as_ref()
                .map(|stream| stream.observation())
                .or(observation.as_deref()),
            _ => None,
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Builtin(s) => s.is_complete(),
            Self::External(s) => s.is_complete(),
            Self::Provider { complete, .. } => *complete,
        }
    }
    pub(crate) fn gates_tool_completion(&self) -> bool {
        match self {
            Self::Builtin(s) => s.gates_tool_completion(),
            Self::External(_) | Self::Provider { .. } => true,
        }
    }
    pub(crate) fn finish(&self) -> Result<(), IrError> {
        match self {
            Self::Builtin(s) => s.finish(),
            Self::External(s) if s.is_complete() => Ok(()),
            Self::Provider { complete: true, .. } => Ok(()),
            _ => Err(IrError::InvalidEventOrder),
        }
    }
}

/// Common host result; legacy adapter/wire output types keep their original shape.
pub(crate) struct ManagedDecoded {
    pub response: Value,
    pub native: crate::ir::continuity::NativeState,
    pub outcome: crate::continuation::Outcome,
    pub observation: ManagedObservation,
    pub terminal_events: Option<Vec<Value>>,
}
pub(crate) enum ManagedObservation {
    Builtin(Accounting),
    Provider(ProviderObservation),
}
impl From<ManagedOutput> for ManagedDecoded {
    fn from(mut value: ManagedOutput) -> Self {
        value.project_usage();
        Self {
            response: value.response,
            native: value.native.into(),
            outcome: value.outcome,
            observation: ManagedObservation::Builtin(value.accounting),
            terminal_events: None,
        }
    }
}
impl TryFrom<provider_plugins::ProviderOutput> for ManagedDecoded {
    type Error = IrError;
    fn try_from(value: provider_plugins::ProviderOutput) -> Result<Self, IrError> {
        let state = value.state.ok_or(IrError::ContinuityMismatch)?;
        let native = crate::ir::continuity::ProviderNativeState::new(
            value.observation.identity.state_binding(),
            state.format,
            state.version,
            state.bytes,
        )?;
        let outcome = match value.outcome {
            gateway_plugin_contract::provider::Outcome::Completed => {
                crate::continuation::Outcome::Completed
            }
            gateway_plugin_contract::provider::Outcome::AwaitingTools => {
                crate::continuation::Outcome::AwaitingTools
            }
            gateway_plugin_contract::provider::Outcome::Incomplete => {
                return Err(IrError::ContinuityMismatch);
            }
        };
        Ok(Self {
            response: value.response,
            native: crate::ir::continuity::NativeState::Provider(native),
            outcome,
            observation: ManagedObservation::Provider(value.observation),
            terminal_events: Some(value.terminal_events),
        })
    }
}
pub(crate) enum ManagedDispatch {
    Builtin(Box<ManagedAdapter>),
    External(Box<PreparedCodec>, gateway_usage_contract::Profile),
    Provider(Box<PreparedProvider>),
}
impl ManagedDispatch {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn prepare(
        binding: Option<&Binding>,
        provider: Option<&provider_plugins::Binding>,
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &VerifiedProviderHistory,
        pending: bool,
        request_maximum: usize,
        maximum: usize,
        profile: Option<gateway_usage_contract::Profile>,
    ) -> Result<Self, IrError> {
        if let Some(provider) = provider {
            if binding.is_some() || profile.is_some() {
                return Err(IrError::UnsupportedFeature);
            }
            let authorized = AuthorizedProviderHistory::from_verified(history, provider)?;
            return PreparedProvider::prepare(
                provider,
                request,
                plan,
                &authorized,
                Some(pending),
                ProviderLimits {
                    request_bytes: request_maximum,
                    output_bytes: maximum,
                },
            )
            .await
            .map(|p| Self::Provider(Box::new(p)));
        }
        let profile = profile.ok_or(IrError::UnsupportedFeature)?;
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
    pub(crate) fn usage_profile(&self) -> Option<gateway_usage_contract::Profile> {
        match self {
            Self::Builtin(p) => Some(p.usage_profile()),
            Self::External(_, profile) => Some(*profile),
            Self::Provider(_) => None,
        }
    }
    pub(crate) fn payload(&self) -> &Value {
        match self {
            Self::Builtin(p) => p.payload(),
            Self::External(p, _) => &p.payload,
            Self::Provider(p) => p.payload(),
        }
    }
    pub(crate) async fn decode_bytes(
        &mut self,
        body: &[u8],
        id: &str,
    ) -> Result<ManagedDecoded, IrError> {
        match self {
            Self::Builtin(p) => p.decode_bytes(body, id).map(Into::into),
            Self::External(p, _) => match p.json(body, id).await? {
                CodecOutput::Managed(v) => Ok((*v).into()),
                _ => Err(IrError::InvalidEventOrder),
            },
            Self::Provider(p) => p.json(body, id).await?.try_into(),
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
            Self::Provider(p) => p
                .stream(id)
                .await
                .map(|s| Managed::Provider(Box::new(s), vec![])),
        }
    }
}
pub(crate) enum Managed<'a> {
    Builtin(Box<ManagedStream<'a>>),
    External(Box<CodecStream<'a>>, Vec<Value>),
    Provider(Box<ProviderStream<'a>>, Vec<Value>),
}
impl Managed<'_> {
    pub(crate) async fn event(&mut self, event: SseEvent) -> Result<(), IrError> {
        match self {
            Self::Builtin(s) => s.event(event),
            Self::External(s, events) => {
                *events = s.event(event).await?;
                Ok(())
            }
            Self::Provider(s, events) => {
                *events = s.event(event).await?.events;
                Ok(())
            }
        }
    }
    pub(crate) fn accounting(&self) -> ManagedObservation {
        match self {
            Self::Builtin(s) => ManagedObservation::Builtin(s.accounting()),
            Self::External(s, _) => ManagedObservation::Builtin(
                s.accounting().expect("verified event accounting").clone(),
            ),
            Self::Provider(s, _) => ManagedObservation::Provider(s.observation().clone()),
        }
    }
    pub(crate) fn take_progress(&mut self) -> Vec<Value> {
        match self {
            Self::Builtin(s) => s.take_progress(),
            Self::External(_, events) | Self::Provider(_, events) => std::mem::take(events),
        }
    }
    pub(crate) fn is_complete(&self) -> bool {
        match self {
            Self::Builtin(s) => s.is_complete(),
            Self::External(s, _) => s.is_complete(),
            Self::Provider(s, _) => s.is_complete(),
        }
    }
    pub(crate) async fn finish(self) -> Result<ManagedDecoded, IrError> {
        match self {
            Self::Builtin(s) => s.finish().map(Into::into),
            Self::External(s, _) => s.finish().await.map(Into::into),
            Self::Provider(mut s, _) => s.finish().await?.try_into(),
        }
    }
}
