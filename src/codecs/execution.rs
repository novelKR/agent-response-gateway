//! Core retains final tool verification, numeric provenance and protected replay ownership.
use super::{Binding, contract::*, process::Session};
use crate::{
    adapters::{
        managed::{Accounting, ManagedOutput},
        responses::{PreparedResponses, output_verifier},
        sse::SseEvent,
    },
    ir::{
        IrError, capability::TranslationPlan, continuity::VerifiedProviderHistory,
        request::RequestIR,
    },
};
use serde_json::Value;

pub(crate) struct PreparedCodec {
    session: Session,
    pub payload: Value,
    verifier: PreparedResponses,
    managed: bool,
    profile: gateway_usage_contract::Profile,
    maximum: usize,
    api: crate::ir::ApiProtocol,
    model: String,
}
impl PreparedCodec {
    pub(crate) async fn prepare(
        binding: &Binding,
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &VerifiedProviderHistory,
        managed_pending: Option<bool>,
        maximum: usize,
        profile: gateway_usage_contract::Profile,
    ) -> Result<Self, IrError> {
        let managed = managed_pending.is_some();
        let pending_tools = managed_pending.unwrap_or(false);
        let verifier = output_verifier(request, plan)?;
        if plan.editing.is_some() && binding.protocol != EDITING_PROTOCOL {
            return Err(IrError::UnsupportedVersion);
        }
        let value = Prepare {
            editing: plan.editing.clone(),
            request: crate::ir::responses::encode(request, None)?,
            route: Route::from_snapshot(&plan.route),
            managed,
            pending_tools,
            history: history
                .segments
                .iter()
                .map(|(start, (end, native))| ReplaySpan {
                    start: *start,
                    end: *end,
                    native: native.clone(),
                })
                .collect(),
            max_output_bytes: maximum,
        };
        let mut session = Session::start(binding).await?;
        let ResultValue::Prepared { payload } = session
            .call(Operation::Prepare {
                value: Box::new(value),
            })
            .await?
        else {
            return Err(IrError::InvalidEventOrder);
        };
        if !payload.is_object()
            || payload["model"] != request.model
            || serde_json::to_vec(&payload)
                .map_err(|_| IrError::InvalidEventOrder)?
                .len()
                > maximum
        {
            return Err(IrError::InvalidField("codec_payload"));
        }
        // Transport/auth controls never come from the codec. Only this JSON object is sent.
        Ok(Self {
            session,
            payload,
            verifier,
            managed,
            profile,
            maximum,
            api: plan.route.api,
            model: request.model.clone(),
        })
    }
    pub(crate) async fn json(
        &mut self,
        body: &[u8],
        response_id: &str,
    ) -> Result<CodecOutput, IrError> {
        let body = std::str::from_utf8(body)
            .map_err(|_| IrError::InvalidField("upstream_json"))?
            .to_owned();
        let value = self
            .session
            .call(Operation::Json {
                body: body.clone(),
                response_id: response_id.into(),
            })
            .await?;
        let raw = crate::adapters::json::decode(body.as_bytes())?;
        if raw["model"] != self.model {
            return Err(IrError::InvalidField("codec_provider_model"));
        }
        if self.managed {
            let terminal = match self.api {
                crate::ir::ApiProtocol::Messages => {
                    matches!(raw["stop_reason"].as_str(), Some("end_turn" | "tool_use"))
                }
                crate::ir::ApiProtocol::ChatCompletions => matches!(
                    raw["choices"][0]["finish_reason"].as_str(),
                    Some("stop" | "tool_calls")
                ),
                crate::ir::ApiProtocol::GeminiInteractions => matches!(
                    raw["status"].as_str(),
                    Some("completed" | "requires_action")
                ),
                _ => false,
            };
            if !terminal {
                return Err(IrError::InvalidEventOrder);
            }
        }
        let mut decoded = self.output(value)?;
        let observed = Accounting::new(
            self.profile,
            raw.get("usage"),
            &raw,
            gateway_usage_contract::Outcome::Completed,
        );
        if let CodecOutput::Stateless(response) = &decoded {
            verify_usage(response, &display_usage(self.profile, &observed.usage)?)?;
        }

        if let CodecOutput::Managed(output) = &mut decoded {
            let accounting = Accounting::new(
                self.profile,
                raw.get("usage"),
                &raw,
                gateway_usage_contract::Outcome::Completed,
            );
            if !accounting.usage.violations.is_empty()
                || serde_json::to_value(&accounting.usage).ok()
                    != serde_json::to_value(&output.accounting.usage).ok()
            {
                return Err(IrError::InvalidField("codec_accounting"));
            }
            output.accounting = accounting;
            output.usage = output.accounting.usage.responses();
            output.project_usage();
        }
        Ok(decoded)
    }
    fn output(&self, value: ResultValue) -> Result<CodecOutput, IrError> {
        match value {
            ResultValue::Json { response } if !self.managed => {
                Ok(CodecOutput::Stateless(self.verify(response)?))
            }
            ResultValue::Managed { value } if self.managed => {
                validate_native(&value.native, self.api)?;
                let response = self.verify(value.response)?;
                let tools = response["output"]
                    .as_array()
                    .ok_or(IrError::InvalidEventOrder)?
                    .iter()
                    .any(|i| {
                        matches!(
                            i["type"].as_str(),
                            Some("function_call" | "custom_tool_call")
                        )
                    });
                if response["status"] != "completed"
                    || (value.outcome == crate::ir::continuity::Outcome::AwaitingTools) != tools
                {
                    return Err(IrError::InvalidEventOrder);
                }
                let usage = value.accounting.usage.responses();
                Ok(CodecOutput::Managed(Box::new(ManagedOutput {
                    response,
                    native: value.native,
                    outcome: value.outcome,
                    usage,
                    accounting: value.accounting,
                })))
            }
            _ => Err(IrError::InvalidEventOrder),
        }
    }
    fn verify(&self, response: Value) -> Result<Value, IrError> {
        let bytes = serde_json::to_vec(&response).map_err(|_| IrError::InvalidEventOrder)?;
        if bytes.len() > self.maximum {
            return Err(IrError::SizeLimit);
        }
        self.verifier.decode_bytes(&bytes)
    }
    pub(crate) async fn stream(&mut self, response_id: String) -> Result<CodecStream<'_>, IrError> {
        match self.session.call(Operation::Stream { response_id }).await? {
            ResultValue::Progress {
                events,
                complete: false,
                accounting: None,
            } if events.is_empty() => {}
            _ => return Err(IrError::InvalidEventOrder),
        }
        let verifier = if self.managed {
            None
        } else {
            Some(self.verifier.stream(self.maximum)?)
        };
        Ok(CodecStream {
            session: &mut self.session,
            output: &self.verifier,
            verifier,
            managed: self.managed,
            maximum: self.maximum,
            profile: self.profile,
            api: self.api,
            model: self.model.clone(),
            complete: false,
            accounting: None,
            raw_usage: gateway_usage_contract::Accumulator::new(self.profile),
            progress: super::verification::Progress::default(),
            terminal_observed: false,
            raw_model: None,
            raw_id: None,
            held: vec![],
            gating: false,
            sequence: 0,
        })
    }
}
pub(crate) enum CodecOutput {
    Stateless(Value),
    Managed(Box<ManagedOutput>),
}
pub(crate) struct CodecStream<'a> {
    session: &'a mut Session,
    output: &'a PreparedResponses,
    verifier: Option<crate::adapters::responses::ResponsesStream<'a>>,
    managed: bool,
    maximum: usize,
    profile: gateway_usage_contract::Profile,
    api: crate::ir::ApiProtocol,
    model: String,
    complete: bool,
    accounting: Option<Accounting>,
    raw_usage: gateway_usage_contract::Accumulator,
    progress: super::verification::Progress,
    terminal_observed: bool,
    raw_model: Option<String>,
    raw_id: Option<String>,
    held: Vec<Value>,
    gating: bool,
    sequence: u64,
}
impl CodecStream<'_> {
    pub(crate) async fn event(&mut self, event: SseEvent) -> Result<Vec<Value>, IrError> {
        if self.complete {
            return Err(IrError::InvalidEventOrder);
        }
        if matches!(
            self.api,
            crate::ir::ApiProtocol::ChatCompletions | crate::ir::ApiProtocol::GeminiInteractions
        ) && event.data == "[DONE]"
        {
            self.terminal_observed = true;
        }
        let raw = crate::adapters::json::decode(event.data.as_bytes()).ok();
        if let Some(raw) = raw {
            self.observe(&raw)?;
        }
        let result = self
            .session
            .call(Operation::Event {
                event: event.event,
                data: event.data,
            })
            .await?;
        let ResultValue::Progress {
            events,
            complete,
            accounting,
        } = result
        else {
            return Err(IrError::InvalidEventOrder);
        };
        if complete && (!self.terminal_observed || !self.raw_usage.usage.violations.is_empty()) {
            return Err(IrError::InvalidEventOrder);
        }
        self.complete = complete;
        if self.managed {
            let mut accounting = accounting.ok_or(IrError::InvalidField("codec_accounting"))?;
            self.check_accounting(&accounting)?;
            accounting.usage = self.raw_usage.usage.clone();
            accounting.model = self.raw_model.clone();
            accounting.response_id = self.raw_id.clone();
            accounting.upstream = if complete {
                gateway_usage_contract::Outcome::Completed
            } else {
                gateway_usage_contract::Outcome::InProgress
            };
            self.accounting = Some(accounting);
            self.output.verify_progress(&events)?;
            self.progress.observe(&events, self.maximum)?;
            Ok(events)
        } else {
            if accounting.is_some() {
                return Err(IrError::InvalidField("codec_accounting"));
            }
            let verifier = self.verifier.as_mut().ok_or(IrError::InvalidEventOrder)?;
            let mut checked = Vec::new();
            for event in events {
                checked.extend(
                    verifier.event(SseEvent {
                        event: event["type"]
                            .as_str()
                            .ok_or(IrError::InvalidEventOrder)?
                            .into(),
                        data: event.to_string(),
                    }).inspect_err(|error|tracing::warn!(error=%error, sequence=event["sequence_number"].as_u64(),"codec_output_verification_failed"))?,
                );
            }
            if complete {
                verifier.finish()?;
                let terminal = checked
                    .iter()
                    .find_map(|event| event.get("response"))
                    .ok_or(IrError::InvalidEventOrder)?;
                verify_usage(
                    terminal,
                    &display_usage(self.profile, &self.raw_usage.usage)?,
                )?;
                if !matches!(
                    self.session.call(Operation::Finish).await?,
                    ResultValue::Finished
                ) {
                    return Err(IrError::InvalidEventOrder);
                }
            } else if verifier.is_complete() {
                return Err(IrError::InvalidEventOrder);
            }
            let mut published = Vec::new();
            for event in checked {
                if event["type"] == "response.output_item.added"
                    && matches!(
                        event["item"]["type"].as_str(),
                        Some("function_call" | "custom_tool_call")
                    )
                {
                    self.gating = true;
                }
                if self.gating {
                    self.held.push(event);
                } else {
                    published.push(event);
                }
            }
            if serde_json::to_vec(&self.held)
                .map_err(|_| IrError::InvalidEventOrder)?
                .len()
                > self.maximum
            {
                return Err(IrError::SizeLimit);
            }
            if complete {
                // Preserve client item ordering while executable arguments await final validation.
                self.held
                    .sort_by_key(|event| event["output_index"].as_u64().unwrap_or(u64::MAX));
                published.append(&mut self.held);
            }
            for event in &mut published {
                event["sequence_number"] = serde_json::json!(self.sequence);
                self.sequence += 1;
            }
            Ok(published)
        }
    }
    fn observe(&mut self, value: &Value) -> Result<(), IrError> {
        if (self.api == crate::ir::ApiProtocol::Messages && value["type"] == "message_stop")
            || (self.api == crate::ir::ApiProtocol::Responses
                && matches!(
                    value["type"].as_str(),
                    Some("response.completed" | "response.incomplete" | "response.failed")
                ))
        {
            self.terminal_observed = true;
        }
        let metadata = value
            .get("message")
            .or_else(|| value.get("interaction"))
            .or_else(|| value.get("response"))
            .unwrap_or(value);
        if metadata
            .get("model")
            .is_some_and(|v| v != &Value::String(self.model.clone()))
        {
            return Err(IrError::InvalidField("codec_provider_model"));
        }
        if self
            .raw_id
            .as_ref()
            .is_some_and(|id| metadata.get("id").is_some_and(|v| v != id))
        {
            return Err(IrError::InvalidEventOrder);
        }
        for (key, destination) in [("model", &mut self.raw_model), ("id", &mut self.raw_id)] {
            if let Some(text) = metadata[key]
                .as_str()
                .filter(|v| gateway_usage_contract::safe_label(v))
            {
                *destination = Some(text.into());
            }
        }
        let profile = self.profile;
        let usage = match profile {
            gateway_usage_contract::Profile::ResponsesV1 => {
                value.get("response").and_then(|v| v.get("usage"))
            }
            gateway_usage_contract::Profile::MessagesV1 => value
                .get("usage")
                .or_else(|| value.get("message").and_then(|v| v.get("usage"))),
            gateway_usage_contract::Profile::GeminiInteractionsV1 => value
                .get("usage")
                .or_else(|| value.get("interaction").and_then(|v| v.get("usage"))),
            _ => value.get("usage"),
        };
        if let Some(usage) = usage.filter(|v| !v.is_null()) {
            self.raw_usage.observe(usage);
        }
        Ok(())
    }
    fn check_accounting(&self, accounting: &Accounting) -> Result<(), IrError> {
        if self.raw_usage.incomplete || accounting.usage.reported != self.raw_usage.usage.reported {
            return Err(IrError::InvalidField("codec_accounting"));
        }
        Ok(())
    }
    pub(crate) fn accounting(&self) -> Option<&Accounting> {
        self.accounting.as_ref()
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.complete
    }
    pub(crate) async fn finish(self) -> Result<ManagedOutput, IrError> {
        if !self.complete || !self.managed {
            return Err(IrError::InvalidEventOrder);
        }
        let result = self.session.call(Operation::Finish).await?;
        let ResultValue::Managed { value } = result else {
            return Err(IrError::InvalidEventOrder);
        };
        validate_native(&value.native, self.api)?;
        let bytes = serde_json::to_vec(&value.response).map_err(|_| IrError::InvalidEventOrder)?;
        if bytes.len() > self.maximum {
            return Err(IrError::SizeLimit);
        }
        let response = self.output.decode_bytes(&bytes)?;
        let tools = response["output"]
            .as_array()
            .ok_or(IrError::InvalidEventOrder)?
            .iter()
            .any(|i| {
                matches!(
                    i["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                )
            });
        if response["status"] != "completed"
            || (value.outcome == crate::ir::continuity::Outcome::AwaitingTools) != tools
        {
            return Err(IrError::InvalidEventOrder);
        }
        let mut output = ManagedOutput {
            response,
            native: value.native,
            outcome: value.outcome,
            usage: value.accounting.usage.responses(),
            accounting: value.accounting,
        };
        self.check_accounting(&output.accounting)?;
        if !self.raw_usage.usage.violations.is_empty() {
            return Err(IrError::InvalidField("usage"));
        }
        output.accounting.usage = self.raw_usage.usage;
        output.accounting.model = self.raw_model;
        output.accounting.response_id = self.raw_id;
        output.accounting.upstream = gateway_usage_contract::Outcome::Completed;
        output.usage = output.accounting.usage.responses();
        output.project_usage();
        self.progress.finish(&output.response)?;
        Ok(output)
    }
}

fn validate_native(
    native: &crate::ir::continuity::NativeReplay,
    api: crate::ir::ApiProtocol,
) -> Result<(), IrError> {
    use crate::ir::{ApiProtocol, continuity::NativeReplay};
    native.validate()?;
    if !matches!(
        (native, api),
        (NativeReplay::Gemini { .. }, ApiProtocol::GeminiInteractions)
            | (NativeReplay::Messages { .. }, ApiProtocol::Messages)
            | (NativeReplay::Chat { .. }, ApiProtocol::ChatCompletions)
    ) {
        return Err(IrError::ContinuityMismatch);
    }
    Ok(())
}

fn verify_usage(
    response: &Value,
    expected: &gateway_usage_contract::CanonicalUsage,
) -> Result<(), IrError> {
    let projected = Accounting::new(
        gateway_usage_contract::Profile::ResponsesV1,
        response.get("usage"),
        response,
        gateway_usage_contract::Outcome::Completed,
    );
    for field in [
        "input_tokens",
        "output_tokens",
        "total_tokens",
        "cache_read_input_tokens",
        "cache_write_input_tokens",
        "reasoning_output_tokens",
    ] {
        if projected.usage.value(field).is_some_and(|value| {
            expected
                .value(field)
                .map_or(value != 0, |expected| value != expected)
        }) {
            return Err(IrError::InvalidField("codec_usage"));
        }
    }
    Ok(())
}

fn display_usage(
    profile: gateway_usage_contract::Profile,
    observed: &gateway_usage_contract::CanonicalUsage,
) -> Result<gateway_usage_contract::CanonicalUsage, IrError> {
    if profile != gateway_usage_contract::Profile::MessagesV1 {
        return Ok(observed.clone());
    }
    // Reconstruct only allowlisted numeric paths already extracted by the core.
    let mut raw = serde_json::json!({});
    for (path, counter) in &observed.reported {
        if counter.source == gateway_usage_contract::Source::Invalid {
            return Err(IrError::InvalidField("usage"));
        }
        let mut parts = path.split('.');
        let first = parts.next().ok_or(IrError::InvalidField("usage"))?;
        if let Some(second) = parts.next() {
            if parts.next().is_some() {
                return Err(IrError::InvalidField("usage"));
            }
            if raw.get(first).is_none() {
                raw[first] = serde_json::json!({});
            }
            raw[first][second] = serde_json::json!(counter.value);
        } else {
            raw[first] = serde_json::json!(counter.value);
        }
    }
    let display = crate::adapters::messages::projected_usage(&raw)?;
    Ok(Accounting::new(
        gateway_usage_contract::Profile::ResponsesV1,
        Some(&display),
        &Value::Null,
        gateway_usage_contract::Outcome::Completed,
    )
    .usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_gate_preserves_unknown_display_zero_but_rejects_fabricated_usage() {
        let empty = gateway_usage_contract::CanonicalUsage::default();
        verify_usage(
            &serde_json::json!({"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}),
            &empty,
        )
        .unwrap();
        assert!(verify_usage(&serde_json::json!({"usage":{"input_tokens":99,"output_tokens":0,"total_tokens":99}}),&empty).is_err());
        let observed = Accounting::new(
            gateway_usage_contract::Profile::ResponsesV1,
            Some(&serde_json::json!({"input_tokens":2,"output_tokens":1,"total_tokens":3})),
            &Value::Null,
            gateway_usage_contract::Outcome::Completed,
        );
        assert!(
            verify_usage(
                &serde_json::json!({"usage":{"input_tokens":3,"output_tokens":1,"total_tokens":4}}),
                &observed.usage
            )
            .is_err()
        );
    }

    #[test]
    fn messages_display_is_checked_against_the_existing_provider_projection() {
        let observed = Accounting::new(
            gateway_usage_contract::Profile::MessagesV1,
            Some(&serde_json::json!({"input_tokens":10,"output_tokens":3})),
            &Value::Null,
            gateway_usage_contract::Outcome::Completed,
        )
        .usage;
        assert_eq!(observed.value("input_tokens"), None);
        let display =
            display_usage(gateway_usage_contract::Profile::MessagesV1, &observed).unwrap();
        verify_usage(
            &serde_json::json!({"usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}}),
            &display,
        )
        .unwrap();
        assert!(verify_usage(&serde_json::json!({"usage":{"input_tokens":11,"output_tokens":3,"total_tokens":14}}),&display).is_err());
    }
}
