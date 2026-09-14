//! Final-output admission, bounded state and arithmetic checks remain in the host.
use super::{Binding, ProviderIdentity, ProviderObservation, contract::*, process::Session, usage};
use crate::{
    adapters::{
        responses::{PreparedResponses, output_verifier},
        sse::SseEvent,
    },
    codecs::verification::Progress,
    ir::{
        IrError,
        capability::{BridgeRule, Support, TranslationPlan},
        request::RequestIR,
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct AuthorizedProviderHistory {
    spans: Vec<ReplaySpan>,
}
impl AuthorizedProviderHistory {
    pub(crate) fn from_verified(
        history: &crate::ir::continuity::VerifiedProviderHistory,
        binding: &Binding,
    ) -> Result<Self, IrError> {
        let identity = binding.identity().state_binding();
        let mut spans = Vec::new();
        let mut last_end = 0;
        let mut format: Option<(String, u32)> = None;
        for (start, (end, native)) in &history.segments {
            if start < &last_end || end < start {
                return Err(IrError::ContinuityMismatch);
            }
            let state = native.provider()?;
            state.validate_for(&identity, format.as_ref().map(|(f, v)| (f.as_str(), *v)))?;
            if format.is_none() {
                format = Some((state.format.clone(), state.version));
            }
            spans.push(ReplaySpan {
                start: u32::try_from(*start).map_err(|_| IrError::SizeLimit)?,
                end: u32::try_from(*end).map_err(|_| IrError::SizeLimit)?,
                state: OpaqueState {
                    format: state.format.clone(),
                    version: state.version,
                    data_base64: STANDARD.encode(state.bytes()),
                },
            });
            last_end = *end;
        }
        Ok(Self { spans })
    }
    fn continuation(&self, pending_tools: bool) -> Continuation {
        Continuation::Managed {
            pending_tools,
            history: self
                .spans
                .iter()
                .map(|span| ReplaySpan {
                    start: span.start,
                    end: span.end,
                    state: OpaqueState {
                        format: span.state.format.clone(),
                        version: span.state.version,
                        data_base64: span.state.data_base64.clone(),
                    },
                })
                .collect(),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct ProviderLimits {
    pub request_bytes: usize,
    pub output_bytes: usize,
}
// Decoded opaque bytes pass only through the host continuation boundary.
pub(crate) struct ValidatedProviderState {
    pub format: String,
    pub version: u32,
    pub bytes: Vec<u8>,
}
pub(crate) struct ProviderOutput {
    pub response: Value,
    pub outcome: Outcome,
    pub state: Option<ValidatedProviderState>,
    pub observation: ProviderObservation,
    pub terminal_events: Vec<Value>,
}
pub(crate) struct ProviderProgress {
    pub events: Vec<Value>,
    pub observation: ProviderObservation,
    pub semantic_complete: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Streaming,
    Finished,
    Failed,
}
pub(crate) struct PreparedProvider {
    session: Session,
    verifier: PreparedResponses,
    identity: ProviderIdentity,
    limits: ProviderLimits,
    payload: Value,
    phase: Phase,
    streaming: bool,
    managed: bool,
    expected_format: Option<(String, u32)>,
    model: String,
    observation: ProviderObservation,
}
impl PreparedProvider {
    pub(crate) async fn prepare(
        binding: &Binding,
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &AuthorizedProviderHistory,
        managed_pending: Option<bool>,
        limits: ProviderLimits,
    ) -> Result<Self, IrError> {
        if (managed_pending.is_none() && !history.spans.is_empty())
            || (managed_pending.is_some() && !binding.capabilities.supports("managed_continuation"))
        {
            return Err(IrError::UnsupportedFeature);
        }
        if binding.protocol != gateway_plugin_contract::PROVIDER_PROTOCOL
            || !gateway_plugin_contract::valid_provider_protocol(&binding.provider_protocol)
            || !binding.capabilities.validate_for(&binding.protocol)
            || (request.generation.stream == Some(true)
                && !binding.capabilities.supports("streaming"))
            || (plan.editing.is_some() && !binding.capabilities.supports("editing"))
        {
            return Err(IrError::UnsupportedFeature);
        }
        if limits.request_bytes == 0
            || limits.output_bytes == 0
            || limits.request_bytes > gateway_plugin_contract::MAX_FRAME
            || limits.output_bytes > gateway_plugin_contract::MAX_FRAME
        {
            return Err(IrError::SizeLimit);
        }
        let verifier = output_verifier(request, plan)?;
        let value = Prepare {
            request: crate::ir::responses::encode(request, None)?,
            route: Route {
                provider_protocol: binding.provider_protocol.clone(),
                model: plan.route.model.clone(),
                profile_id: plan.route.capabilities.id.clone(),
                profile_version: plan.route.capabilities.version.clone(),
                support: plan
                    .route
                    .capabilities
                    .support
                    .iter()
                    .map(|(feature, support)| {
                        let name = serde_json::to_value(feature)
                            .map_err(|_| IrError::InvalidEventOrder)?;
                        Ok((
                            name.as_str().ok_or(IrError::InvalidEventOrder)?.into(),
                            support_name(*support).into(),
                        ))
                    })
                    .collect::<Result<_, IrError>>()?,
                editing: match &plan.editing {
                    Some(policy) => EditingSelection::Enabled {
                        policy: serde_json::to_value(policy)
                            .map_err(|_| IrError::InvalidEventOrder)?,
                    },
                    None => EditingSelection::None,
                },
            },
            continuation: managed_pending.map_or(Continuation::Stateless, |pending| {
                history.continuation(pending)
            }),
            max_request_bytes: limits.request_bytes as u32,
            max_output_bytes: limits.output_bytes as u32,
        };
        bound(&value.request, limits.request_bytes)?;
        let mut session = Session::start(binding).await?;
        let ResultValue::Prepared { payload } = session
            .call(Operation::Prepare {
                value: Box::new(value),
            })
            .await?
        else {
            return Err(IrError::InvalidEventOrder);
        };
        if !payload.is_object() {
            return Err(IrError::InvalidField("provider_payload"));
        }
        bound(&payload, limits.request_bytes)?;
        Ok(Self {
            session,
            verifier,
            identity: binding.identity(),
            limits,
            payload,
            phase: Phase::Prepared,
            streaming: request.generation.stream == Some(true),
            managed: managed_pending.is_some(),
            expected_format: history
                .spans
                .last()
                .map(|span| (span.state.format.clone(), span.state.version)),
            model: request.model.clone(),
            observation: ProviderObservation {
                identity: binding.identity(),
                usage: gateway_usage_contract::CanonicalUsage::default(),
            },
        })
    }
    pub(crate) fn observation(&self) -> &ProviderObservation {
        &self.observation
    }
    fn capture_attempt(&mut self) {
        if self.observation.usage.violations.is_empty()
            && let Some(usage) = self.session.attempted_usage()
        {
            self.observation = ProviderObservation {
                identity: self.identity.clone(),
                usage: usage.clone(),
            };
        }
    }
    fn fail(&mut self) {
        self.phase = Phase::Failed;
        self.session.poison();
    }
    fn verifier_model(&self) -> &str {
        &self.model
    }
    pub(crate) fn payload(&self) -> &Value {
        &self.payload
    }
    pub(crate) fn take_payload(&mut self) -> Value {
        std::mem::take(&mut self.payload)
    }
    pub(crate) async fn json(
        &mut self,
        body: &[u8],
        response_id: &str,
    ) -> Result<ProviderOutput, IrError> {
        if self.phase != Phase::Prepared || self.streaming {
            self.fail();
            return Err(IrError::InvalidEventOrder);
        }
        self.phase = Phase::Failed;
        let result = async {
            if body.len() > self.limits.output_bytes {
                return Err(IrError::SizeLimit);
            }
            // Validate JSON framing and duplicate keys, without assuming vendor semantic keys.
            crate::adapters::json::decode(body)?;
            let body = std::str::from_utf8(body)
                .map_err(|_| IrError::InvalidField("upstream_json"))?
                .into();
            let ResultValue::Completed { value } = self
                .session
                .call(Operation::Json {
                    body,
                    response_id: response_id.into(),
                })
                .await?
            else {
                return Err(IrError::InvalidEventOrder);
            };
            self.verify(
                *value,
                &gateway_usage_contract::CanonicalUsage::default(),
                response_id,
            )
        }
        .await;
        if result.is_ok() {
            self.phase = Phase::Finished;
        } else {
            self.capture_attempt();
            self.session.poison();
        }
        result
    }
    fn verify(
        &mut self,
        value: Completed,
        previous: &gateway_usage_contract::CanonicalUsage,
        response_id: &str,
    ) -> Result<ProviderOutput, IrError> {
        let mut usage = usage::observe(&value.usage);
        usage::observe_cumulative(previous, &mut usage);
        self.observation = ProviderObservation {
            identity: self.identity.clone(),
            usage,
        };
        usage::ensure_valid(&self.observation.usage)?;
        let usage = self.observation.usage.clone();
        if response_id.is_empty() || value.response["id"] != response_id {
            return Err(IrError::InvalidEventOrder);
        }
        let state = match (value.state, self.managed) {
            (StateResult::None, false) => None,
            (StateResult::Opaque { value: state }, true) => {
                let decoded = crate::continuation::decode_provider_state(
                    self.identity.state_binding(),
                    state.format,
                    state.version,
                    &state.data_base64,
                )
                .map_err(|_| IrError::ContinuityMismatch)?;
                decoded.validate_for(
                    &self.identity.state_binding(),
                    self.expected_format.as_ref().map(|(f, v)| (f.as_str(), *v)),
                )?;
                Some(ValidatedProviderState {
                    format: decoded.format.clone(),
                    version: decoded.version,
                    bytes: decoded.bytes().to_vec(),
                })
            }
            _ => return Err(IrError::ContinuityMismatch),
        };
        if self.managed && value.outcome == Outcome::Incomplete {
            return Err(IrError::ContinuityMismatch);
        }
        bound(&value.response, self.limits.output_bytes)?;
        usage::verify_response(&value.response, &usage)?;
        let response = self.verifier.decode_bytes(
            &serde_json::to_vec(&value.response).map_err(|_| IrError::InvalidEventOrder)?,
        )?;
        bound(&response, self.limits.output_bytes)?;
        usage::verify_response(&response, &usage)?;
        let tools = response["output"]
            .as_array()
            .ok_or(IrError::InvalidEventOrder)?
            .iter()
            .any(|item| {
                matches!(
                    item["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                )
            });
        match value.outcome {
            Outcome::AwaitingTools if response["status"] == "completed" && tools => {}
            Outcome::Completed if response["status"] == "completed" && !tools => {}
            Outcome::Incomplete if response["status"] == "incomplete" && !tools => {}
            _ => return Err(IrError::InvalidEventOrder),
        }
        Ok(ProviderOutput {
            response,
            outcome: value.outcome,
            state,
            observation: ProviderObservation {
                identity: self.identity.clone(),
                usage,
            },
            terminal_events: vec![],
        })
    }
    pub(crate) async fn stream(
        &mut self,
        response_id: String,
    ) -> Result<ProviderStream<'_>, IrError> {
        if self.phase != Phase::Prepared || !self.streaming {
            self.fail();
            return Err(IrError::InvalidEventOrder);
        }
        self.phase = Phase::Failed;
        let result = match self
            .session
            .call(Operation::Stream {
                response_id: response_id.clone(),
            })
            .await
        {
            Ok(result) => result,
            Err(error) => {
                self.capture_attempt();
                self.fail();
                return Err(error);
            }
        };
        if !matches!(result,ResultValue::Progress {ref events,complete:false,usage:UsageSnapshot::Unobserved} if events.is_empty())
        {
            self.capture_attempt();
            self.fail();
            return Err(IrError::InvalidEventOrder);
        }
        self.phase = Phase::Streaming;
        Ok(ProviderStream {
            prepared: self,
            progress: Progress::default(),
            complete: false,
            observation: gateway_usage_contract::CanonicalUsage::default(),
            input_bytes: 0,
            response_id,
            created_at: None,
        })
    }
}
pub(crate) struct ProviderStream<'a> {
    prepared: &'a mut PreparedProvider,
    progress: Progress,
    complete: bool,
    observation: gateway_usage_contract::CanonicalUsage,
    input_bytes: usize,
    response_id: String,
    created_at: Option<u64>,
}
impl ProviderStream<'_> {
    pub(crate) fn observation(&self) -> &ProviderObservation {
        self.prepared.observation()
    }
    pub(crate) async fn event(&mut self, event: SseEvent) -> Result<ProviderProgress, IrError> {
        if self.complete || self.prepared.phase != Phase::Streaming {
            self.prepared.fail();
            return Err(IrError::InvalidEventOrder);
        }
        let result = async {
            self.input_bytes = self
                .input_bytes
                .checked_add(event.event.len())
                .and_then(|n| n.checked_add(event.data.len()))
                .ok_or(IrError::SizeLimit)?;
            if self.input_bytes > self.prepared.limits.output_bytes {
                return Err(IrError::SizeLimit);
            }
            let ResultValue::Progress {
                events,
                complete,
                usage,
            } = self
                .prepared
                .session
                .call(Operation::Event {
                    event: event.event,
                    data: event.data,
                })
                .await?
            else {
                return Err(IrError::InvalidEventOrder);
            };
            let mut observation = usage::observe(&usage);
            usage::observe_cumulative(&self.observation, &mut observation);
            self.prepared.observation = ProviderObservation {
                identity: self.prepared.identity.clone(),
                usage: observation.clone(),
            };
            usage::ensure_valid(&observation)?;
            let mut semantic_events = events.clone();
            for event in &mut semantic_events {
                if event["type"] == "response.created" {
                    let response = &mut event["response"];
                    if self.created_at.is_some()
                        || response["id"] != self.response_id
                        || response["model"] != self.prepared.verifier_model()
                    {
                        return Err(IrError::InvalidEventOrder);
                    }
                    self.created_at = Some(
                        response["created_at"]
                            .as_u64()
                            .ok_or(IrError::InvalidEventOrder)?,
                    );
                    let response = response.as_object_mut().ok_or(IrError::InvalidEventOrder)?;
                    response.remove("created_at");
                    response.remove("model");
                }
            }
            self.prepared.verifier.verify_progress(&semantic_events)?;
            self.progress
                .observe_provider(&events, self.prepared.limits.output_bytes)?;
            self.observation = observation;
            self.complete = complete;
            Ok(ProviderProgress {
                events,
                semantic_complete: complete,
                observation: ProviderObservation {
                    identity: self.prepared.identity.clone(),
                    usage: self.observation.clone(),
                },
            })
        }
        .await;
        if result.is_err() {
            self.prepared.capture_attempt();
            self.prepared.phase = Phase::Failed;
            self.prepared.session.poison();
        }
        result
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.complete && self.prepared.phase == Phase::Streaming
    }
    pub(crate) async fn finish(&mut self) -> Result<ProviderOutput, IrError> {
        if !self.is_complete() {
            self.prepared.fail();
            return Err(IrError::InvalidEventOrder);
        }
        self.prepared.phase = Phase::Failed;
        let result = async {
            let ResultValue::Completed { value } =
                self.prepared.session.call(Operation::Finish).await?
            else {
                return Err(IrError::InvalidEventOrder);
            };
            let mut output = self
                .prepared
                .verify(*value, &self.observation, &self.response_id)?;
            if self
                .created_at
                .is_some_and(|time| output.response["created_at"] != time)
            {
                return Err(IrError::InvalidEventOrder);
            }
            output.terminal_events = self
                .progress
                .terminal_events(&output.response, self.prepared.limits.output_bytes)?;
            Ok(output)
        }
        .await;
        if result.is_ok() {
            self.prepared.phase = Phase::Finished;
        } else {
            self.prepared.capture_attempt();
            self.prepared.session.poison();
        }
        result
    }
}
fn bound(value: &Value, maximum: usize) -> Result<(), IrError> {
    if serde_json::to_vec(value)
        .map_err(|_| IrError::InvalidEventOrder)?
        .len()
        > maximum
    {
        Err(IrError::SizeLimit)
    } else {
        Ok(())
    }
}
fn support_name(s: Support) -> &'static str {
    match s {
        Support::Native => "native",
        Support::Unsupported => "unsupported",
        Support::Bridged(b) => match b {
            BridgeRule::CodeModeTextParts => "code_mode_text_parts",
            BridgeRule::CustomToolJson => "custom_tool_json",
            BridgeRule::ToolNamespace => "tool_namespace",
            BridgeRule::CodexPatchGrammar => "codex_patch_grammar",
            BridgeRule::RegisteredGrammarValidation => "registered_grammar_validation",
            BridgeRule::MessagesInstructionEnvelope => "messages_instruction_envelope",
            BridgeRule::GeminiInstructionEnvelope => "gemini_instruction_envelope",
            BridgeRule::ChatInstructionEnvelope => "chat_instruction_envelope",
            BridgeRule::ProviderParallelPermission => "provider_parallel_permission",
        },
    }
}

#[cfg(all(test, unix))]
impl PreparedProvider {
    pub(super) fn test_output_limit(&mut self, maximum: usize) {
        self.limits.output_bytes = maximum;
    }
    pub(super) fn test_prepared(
        binding: &Binding,
        request: &RequestIR,
        plan: &TranslationPlan,
    ) -> (Self, std::os::unix::net::UnixStream) {
        let (session, peer) = Session::test_pair();
        (
            Self {
                session,
                verifier: output_verifier(request, plan).unwrap(),
                identity: binding.identity(),
                limits: ProviderLimits {
                    request_bytes: 65536,
                    output_bytes: 65536,
                },
                payload: serde_json::json!({"query":"synthetic"}),
                phase: Phase::Prepared,
                streaming: request.generation.stream == Some(true),
                managed: false,
                expected_format: None,
                model: request.model.clone(),
                observation: ProviderObservation {
                    identity: binding.identity(),
                    usage: gateway_usage_contract::CanonicalUsage::default(),
                },
            },
            peer,
        )
    }
}
