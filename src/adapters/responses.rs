//! Opt-in checked Responses with selective, request-scoped tool restoration.
//! Passthrough does not use this codec. Unknown semantic fields fail closed.
mod stream;
pub use stream::ResponsesStream;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{
    json::{decode, event_text_chunks, known_fields, string},
    toolset::{PreparedTools, tool_output},
};
use crate::ir::{
    ApiProtocol, CallId, IrError, ItemId, ResponseId, ToolIdentity,
    bridge::CustomToolBridge,
    capability::{Feature, Support, TranslationPlan},
    event::{
        ContentIndex, EventIR, EventLimits, EventValidator, OutputIndex, OutputKind, PartKind,
        Terminal,
    },
    request::{Content, Extensions, Input, Item, RequestIR, ToolCall, ToolCallStatus, ToolInput},
    responses,
};

pub struct PreparedResponses {
    pub payload: Value,
    original: Value,
    echoes: Value,
    tools: PreparedTools,
    profile: crate::ir::capability::CapabilityProfile,
    model: String,
    preserve_item_order: bool,
}

/// Core-side validation of already restored client output. This does not encode a provider request.
pub(crate) fn output_verifier(
    request: &RequestIR,
    plan: &TranslationPlan,
) -> Result<PreparedResponses, IrError> {
    let mut profile = plan.route.capabilities.clone();
    profile.protocol = ApiProtocol::Responses;
    for feature in [
        Feature::FunctionTools,
        Feature::CustomTools,
        Feature::NamespacedTools,
        Feature::CustomGrammar,
    ] {
        profile.support.insert(feature, Support::Native);
    }
    let registry = CustomToolBridge::for_plan(request, &profile, plan.editing.as_ref(), false)?;
    let original = responses::encode(request, None)?;
    Ok(PreparedResponses {
        payload: Value::Null,
        echoes: original.clone(),
        original,
        tools: PreparedTools::new(registry, request),
        profile,
        model: request.model.clone(),
        preserve_item_order: true,
    })
}

pub(crate) fn encode_admitted(
    request: &RequestIR,
    plan: &TranslationPlan,
) -> Result<PreparedResponses, IrError> {
    if request.source != ApiProtocol::Responses || plan.route.api != ApiProtocol::Responses {
        return Err(IrError::WrongProtocol);
    }
    validate_request(
        request,
        plan.bridges
            .contains(&crate::ir::capability::BridgeRule::CodeModeTextParts),
    )?;
    let registry = CustomToolBridge::for_plan(
        request,
        &plan.route.capabilities,
        plan.editing.as_ref(),
        true,
    )?;
    let original = responses::encode(request, None)?;
    let mut lowered = request.clone();
    if request.tools.is_some() {
        lowered.tools = Some(registry.definitions().to_vec());
    }
    if let Some(choice) = &request.generation.tool_choice {
        lowered.generation.tool_choice = Some(registry.lower_choice(choice)?);
    }
    let mut calls = BTreeMap::new();
    if let Some(Input::Items(items)) = &mut lowered.input {
        for item in items {
            match item {
                Item::ToolCall(call) => {
                    calls.insert(call.call_id.clone(), call.clone());
                    *call = registry.lower_call(call)?;
                }
                Item::ToolResult(result) => {
                    *result = registry.lower_result(
                        result,
                        calls
                            .get(&result.call_id)
                            .ok_or(IrError::InvalidToolMapping)?,
                    )?;
                }
                _ => {}
            }
        }
    }
    let payload = responses::encode(&lowered, None)?;
    let echoes = ["tools", "tool_choice", "parallel_tool_calls"]
        .into_iter()
        .filter_map(|k| payload.get(k).map(|v| (k.to_owned(), v.clone())))
        .collect::<serde_json::Map<_, _>>();
    Ok(PreparedResponses {
        preserve_item_order: false,
        payload,
        original,
        echoes: Value::Object(echoes),
        tools: PreparedTools::new(registry, request),
        model: plan.route.model.clone(),
        profile: plan.route.capabilities.clone(),
    })
}

fn validate_request(request: &RequestIR, code_mode_results: bool) -> Result<(), IrError> {
    // Only nonsemantic transport/cache hints and standard optional nulls are retained.
    for (key, value) in &request.extensions.fields {
        let valid = match key.as_str() {
            "client_metadata" | "metadata" => value
                .as_object()
                .is_some_and(|m| m.values().all(Value::is_string)),
            "prompt_cache_key" => value.is_string(),
            // Opaque continuation is not admitted by this stateless codec.
            "include" => value
                .as_array()
                .is_some_and(|a| a.iter().all(|v| v == "reasoning.encrypted_content")),
            "background" => value == false,
            "input"
            | "instructions"
            | "tools"
            | "stream"
            | "max_output_tokens"
            | "temperature"
            | "top_p"
            | "parallel_tool_calls"
            | "tool_choice"
            | "text"
            | "reasoning" => value.is_null(),
            _ => false,
        };
        if !valid {
            return Err(IrError::UnsupportedExtension);
        }
    }
    if let Some(output) = &request.generation.output {
        use crate::ir::request::OutputFormat;
        let extensions = match &output.format {
            Some(OutputFormat::Text(e) | OutputFormat::JsonObject(e))
            | Some(OutputFormat::JsonSchema { extensions: e, .. }) => Some(e),
            Some(OutputFormat::Extension { .. }) => return Err(IrError::UnsupportedExtension),
            None => None,
        };
        if !output.extensions.fields.is_empty() || extensions.is_some_and(|e| !e.fields.is_empty())
        {
            return Err(IrError::UnsupportedExtension);
        }
    }
    if request
        .generation
        .reasoning
        .as_ref()
        .is_some_and(|r| !r.extensions.fields.is_empty())
    {
        return Err(IrError::UnsupportedExtension);
    }
    if let Some(Input::Items(items)) = &request.input {
        for item in items {
            match item {
                Item::Message(m) => {
                    for (key, value) in &m.extensions.fields {
                        if key != "phase" || !valid_phase(value) {
                            return Err(IrError::UnsupportedExtension);
                        }
                    }
                    if matches!(
                        m.status,
                        Some(ToolCallStatus::InProgress | ToolCallStatus::Incomplete)
                    ) {
                        return Err(IrError::InvalidEventOrder);
                    }
                    if let Content::Parts(parts) = &m.content {
                        for p in parts {
                            if !p.extensions.fields.is_empty()
                                || matches!(p.kind, crate::ir::request::PartKind::Extension(_))
                                || p.annotations.as_ref().is_some_and(|a| !a.is_empty())
                            {
                                return Err(IrError::UnsupportedExtension);
                            }
                        }
                    }
                }
                Item::ToolCall(c) if c.extensions.fields.is_empty() => {}
                Item::ToolResult(r)
                    if r.extensions.fields.is_empty()
                        && (r.output.is_string()
                            || (code_mode_results
                                && crate::editing::code_mode_result(&r.output).is_ok())) => {}
                Item::Reasoning(r) if r.extensions.fields.is_empty() && r.opaque.is_none() => {
                    for p in r.summary.iter().flatten() {
                        if !p.extensions.fields.is_empty() {
                            return Err(IrError::UnsupportedExtension);
                        }
                    }
                }
                _ => return Err(IrError::UnsupportedExtension),
            }
        }
    }
    Ok(())
}

impl PreparedResponses {
    pub(crate) fn verify_progress(&self, events: &[Value]) -> Result<(), IrError> {
        for event in events {
            if event["type"] == "response.created" {
                let response = &event["response"];
                known_fields(response, &["id", "object", "status", "output"])?;
                ResponseId::new(string(response, "id")?)?;
                if response["object"] != "response"
                    || event["response"]["status"] != "in_progress"
                    || event["response"]["output"] != json!([])
                {
                    return Err(IrError::InvalidEventOrder);
                }
            }
            if (event["item"]["type"] == "reasoning"
                || event["type"]
                    .as_str()
                    .is_some_and(|v| v.starts_with("response.reasoning_")))
                && self.profile.support(Feature::ReasoningItems) != Support::Native
            {
                return Err(IrError::UnsupportedFeature);
            }
        }
        Ok(())
    }
    pub fn encode(
        request: &RequestIR,
        target: &crate::ir::continuity::ContinuityBinding,
    ) -> Result<Self, IrError> {
        let plan = crate::ir::capability::plan_translation(request, target)?;
        encode_admitted(request, &plan)
    }
    pub fn decode_bytes(&self, bytes: &[u8]) -> Result<Value, IrError> {
        let value = decode(bytes)?;
        self.decode_value(value)
    }

    pub fn stream(&self, limit: usize) -> Result<ResponsesStream<'_>, IrError> {
        ResponsesStream::new(self, limit)
    }

    fn decode_value(&self, mut value: Value) -> Result<Value, IrError> {
        self.header(&value)?;
        let terminal = terminal(string(&value, "status")?)?;
        let items = value["output"]
            .as_array()
            .ok_or(IrError::InvalidField("output"))?;
        let mut validator = EventValidator::new(EventLimits::default())?;
        validator.apply(EventIR::Started {
            id: ResponseId::new(string(&value, "id")?)?,
        })?;
        let mut output = Vec::new();
        let mut count = 0;
        for (index, item) in items.iter().enumerate() {
            let restored = self.restore_item(item)?;
            if terminal == Terminal::Completed && restored["status"] == "incomplete" {
                return Err(IrError::InvalidEventOrder);
            }
            if is_tool(&restored) {
                count += 1;
                if terminal != Terminal::Completed {
                    return Err(IrError::InvalidEventOrder);
                }
            }
            validate_complete_item(&mut validator, &restored, index)?;
            output.push(restored);
        }
        self.tools
            .validate_count(count, terminal == Terminal::Completed)?;
        let mut usage =
            gateway_usage_contract::Accumulator::new(gateway_usage_contract::Profile::ResponsesV1);
        if let Some(counters) = value.get("usage").filter(|v| !v.is_null()) {
            usage.observe(counters);
        }
        if !usage.usage.violations.is_empty() {
            return Err(IrError::InvalidField("usage"));
        }
        validator.apply(EventIR::Finished {
            status: terminal,
            reason: None,
        })?;
        if let Some(echo) = value.get("output_text").filter(|v| !v.is_null()) {
            let text = output
                .iter()
                .filter(|v| v["type"] == "message")
                .flat_map(|v| v["content"].as_array().into_iter().flatten())
                .map(|v| string(v, "text"))
                .collect::<Result<Vec<_>, _>>()?
                .join("");
            if echo != &Value::String(text) {
                return Err(IrError::InvalidEventOrder);
            }
        }
        value["output"] = json!(output);
        self.restore_echoes(&mut value)?;
        if terminal == Terminal::Failed {
            value["error"] =
                json!({"code":"upstream_error","message":"The upstream response failed"});
        }
        Ok(value)
    }

    fn header(&self, value: &Value) -> Result<(), IrError> {
        known_fields(
            value,
            &[
                "id",
                "object",
                "created_at",
                "completed_at",
                "status",
                "model",
                "output",
                "usage",
                "error",
                "incomplete_details",
                "instructions",
                "tools",
                "tool_choice",
                "parallel_tool_calls",
                "max_output_tokens",
                "temperature",
                "top_p",
                "text",
                "reasoning",
                "metadata",
                "store",
                "background",
                "previous_response_id",
                "conversation",
                "max_tool_calls",
                "prompt_cache_key",
                "service_tier",
                "truncation",
                "user",
                "safety_identifier",
                "output_text",
                "top_logprobs",
            ],
        )?;
        ResponseId::new(string(value, "id")?)?;
        let status = string(value, "status")?;
        if !matches!(
            status,
            "in_progress" | "completed" | "incomplete" | "failed"
        ) || (status != "failed" && value.get("error").is_some_and(|v| !v.is_null()))
        {
            return Err(IrError::InvalidEventOrder);
        }
        if let Some(details) = value.get("incomplete_details").filter(|v| !v.is_null()) {
            known_fields(details, &["reason"])?;
            if status != "incomplete"
                || !matches!(
                    string(details, "reason")?,
                    "max_output_tokens" | "content_filter"
                )
            {
                return Err(IrError::UnsupportedFeature);
            }
        }
        if string(value, "object")? != "response"
            || string(value, "model")? != self.model
            || !value["created_at"].is_u64()
            || value
                .get("store")
                .is_some_and(|v| !v.is_null() && v != false)
            || value
                .get("background")
                .is_some_and(|v| !v.is_null() && v != false)
            || ["previous_response_id", "conversation"]
                .iter()
                .any(|k| value.get(*k).is_some_and(|v| !v.is_null()))
        {
            return Err(IrError::UnsupportedFeature);
        }
        Ok(())
    }

    fn restore_echoes(&self, value: &mut Value) -> Result<(), IrError> {
        // Echoes must describe the exact dispatched contract before restoring the client view.
        for key in ["tools", "tool_choice", "parallel_tool_calls"] {
            if let Some(echo) = value.get(key) {
                if !echo.is_null() {
                    let valid = match self.echoes.get(key) {
                        Some(sent) => sent == echo,
                        None => match key {
                            "tools" => echo == &json!([]),
                            "tool_choice" => echo == "auto" || echo == "none",
                            "parallel_tool_calls" => echo == true,
                            _ => unreachable!(),
                        },
                    };
                    if !valid {
                        return Err(IrError::InvalidToolMapping);
                    }
                }
                if let Some(original) = self.original.get(key) {
                    value[key] = original.clone();
                }
            }
        }
        Ok(())
    }

    fn restore_item(&self, item: &Value) -> Result<Value, IrError> {
        let id = ItemId::new(string(item, "id")?)?;
        match string(item, "type")? {
            "function_call" | "custom_tool_call" => {
                known_fields(
                    item,
                    &[
                        "id",
                        "type",
                        "call_id",
                        "name",
                        "namespace",
                        "arguments",
                        "input",
                        "status",
                    ],
                )?;
                if string(item, "status")? != "completed" {
                    return Err(IrError::InvalidEventOrder);
                }
                let custom = item["type"] == "custom_tool_call";
                if !custom {
                    decode(string(item, "arguments")?.as_bytes())?;
                }
                if item
                    .get(if custom { "arguments" } else { "input" })
                    .is_some()
                {
                    return Err(IrError::InvalidToolMapping);
                }
                let identity = identity(item)?;
                self.tools.resolve_identity(&identity)?;
                let restored = self.tools.registry.restore_call(&ToolCall {
                    status: Some(ToolCallStatus::Completed),
                    item_id: Some(id),
                    call_id: CallId::new(string(item, "call_id")?)?,
                    tool: identity,
                    input: if custom {
                        ToolInput::Freeform(string(item, "input")?.into())
                    } else {
                        ToolInput::Json(string(item, "arguments")?.into())
                    },
                    extensions: Extensions::responses(),
                })?;
                Ok(tool_output(&restored, "completed"))
            }
            "message" => {
                known_fields(item, &["id", "type", "role", "status", "content", "phase"])?;
                if item.get("phase").is_some_and(|v| !valid_phase(v)) {
                    return Err(IrError::UnsupportedFeature);
                }
                if item["role"] != "assistant"
                    || !matches!(item["status"].as_str(), Some("completed" | "incomplete"))
                {
                    return Err(IrError::InvalidEventOrder);
                }
                for part in item["content"]
                    .as_array()
                    .ok_or(IrError::InvalidField("content"))?
                {
                    validate_part(part, false)?;
                }
                Ok(item.clone())
            }
            "reasoning" => {
                if self.profile.support(Feature::ReasoningItems) != Support::Native {
                    return Err(IrError::UnsupportedFeature);
                }
                known_fields(
                    item,
                    &["id", "type", "status", "summary", "encrypted_content"],
                )?;
                if item.get("encrypted_content").is_some_and(|v| !v.is_null())
                    || item
                        .get("status")
                        .is_some_and(|v| !matches!(v.as_str(), Some("completed" | "incomplete")))
                {
                    return Err(IrError::UnsupportedFeature);
                }
                for part in item["summary"]
                    .as_array()
                    .ok_or(IrError::InvalidField("summary"))?
                {
                    validate_part(part, true)?;
                }
                Ok(item.clone())
            }
            _ => Err(IrError::UnsupportedFeature),
        }
    }
}

fn valid_phase(value: &Value) -> bool {
    value.is_null() || matches!(value.as_str(), Some("commentary" | "final_answer"))
}

fn identity(value: &Value) -> Result<ToolIdentity, IrError> {
    let namespace = value
        .get("namespace")
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or(IrError::InvalidToolMapping)
        })
        .transpose()?;
    ToolIdentity::new(namespace, string(value, "name")?)
}

fn is_tool(value: &Value) -> bool {
    matches!(
        value["type"].as_str(),
        Some("function_call" | "custom_tool_call")
    )
}

fn terminal(value: &str) -> Result<Terminal, IrError> {
    match value {
        "completed" => Ok(Terminal::Completed),
        "incomplete" => Ok(Terminal::Incomplete),
        "failed" => Ok(Terminal::Failed),
        _ => Err(IrError::InvalidEventOrder),
    }
}

fn validate_part(part: &Value, reasoning: bool) -> Result<(), IrError> {
    known_fields(
        part,
        if reasoning {
            &["type", "text"]
        } else {
            &["type", "text", "annotations", "logprobs"]
        },
    )?;
    if part["type"]
        != if reasoning {
            "summary_text"
        } else {
            "output_text"
        }
        || part
            .get("annotations")
            .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
        || part
            .get("logprobs")
            .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
    {
        return Err(IrError::UnsupportedFeature);
    }
    string(part, "text")?;
    Ok(())
}

fn validate_complete_item(
    validator: &mut EventValidator,
    item: &Value,
    index: usize,
) -> Result<(), IrError> {
    let id = ItemId::new(string(item, "id")?)?;
    let kind = match item["type"].as_str() {
        Some("message") => OutputKind::Message,
        Some("reasoning") => OutputKind::Reasoning,
        Some("function_call" | "custom_tool_call") => OutputKind::Tool {
            tool: identity(item)?,
            call_id: CallId::new(string(item, "call_id")?)?,
            kind: if item["type"] == "custom_tool_call" {
                crate::ir::ToolKind::Custom
            } else {
                crate::ir::ToolKind::Function
            },
        },
        _ => return Err(IrError::UnsupportedFeature),
    };
    validator.apply(EventIR::ItemStarted {
        id: id.clone(),
        index: OutputIndex(u32::try_from(index).map_err(|_| IrError::SizeLimit)?),
        kind,
    })?;
    if is_tool(item) {
        let text = string(
            item,
            if item["type"] == "custom_tool_call" {
                "input"
            } else {
                "arguments"
            },
        )?;
        for chunk in event_text_chunks(text) {
            validator.apply(EventIR::ArgumentsDelta {
                item: id.clone(),
                text: chunk.into(),
            })?;
        }
    } else {
        let reasoning = item["type"] == "reasoning";
        for (index, part) in item[if reasoning { "summary" } else { "content" }]
            .as_array()
            .ok_or(IrError::InvalidField("content"))?
            .iter()
            .enumerate()
        {
            let index = ContentIndex(u32::try_from(index).map_err(|_| IrError::SizeLimit)?);
            validator.apply(EventIR::PartStarted {
                item: id.clone(),
                index,
                kind: if reasoning {
                    PartKind::ReasoningText
                } else {
                    PartKind::Text
                },
            })?;
            for chunk in event_text_chunks(string(part, "text")?) {
                validator.apply(EventIR::TextDelta {
                    item: id.clone(),
                    index,
                    text: chunk.into(),
                })?;
            }
            validator.apply(EventIR::PartFinished {
                item: id.clone(),
                index,
            })?;
        }
    }
    validator.apply(EventIR::ItemFinished { item: id })
}
