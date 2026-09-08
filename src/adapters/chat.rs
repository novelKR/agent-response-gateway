//! Chat Completions request/non-streaming codec for an explicit function-tool wire profile.
//! Library availability does not activate HTTP dispatch before stream/Codex qualification.
use super::{
    json::{known_fields, object, string},
    toolset::{PreparedTools, tool_output},
};
use crate::ir::{
    ApiProtocol, CallId, IrError, ItemId, ResponseId,
    bridge::CustomToolBridge,
    capability::{BridgeRule, Feature, TranslationPlan, plan_translation},
    continuity::ContinuityBinding,
    event::{
        ContentIndex, EventIR, EventLimits, EventValidator, OutputIndex, OutputKind,
        PartKind as EventPartKind, Terminal, Usage,
    },
    request::{
        Content, Input, Item, OutputFormat, PartKind, RequestIR, Role, ToolChoice,
        ToolDefinitionKind, ToolInput,
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct PreparedChat {
    pub payload: Value,
    model: String,
    tools: PreparedTools,
}
fn unsupported() -> IrError {
    IrError::UnsupportedFeature
}
fn role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::Developer => "developer",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}
fn content(content: &Content, role: Role) -> Result<Value, IrError> {
    match content {
        Content::Text(text) => Ok(json!(text)),
        Content::Parts(parts) => {
            let parts = parts
                .iter()
                .map(|part| match &part.kind {
                    PartKind::Text { text, .. } => Ok(json!({"type":"text","text":text})),
                    PartKind::Image { url, detail }
                        if role == Role::User && url.starts_with("https://") =>
                    {
                        let mut image = json!({"url":url});
                        if let Some(detail) = detail {
                            if !matches!(detail.as_str(), "auto" | "low" | "high") {
                                return Err(unsupported());
                            }
                            image["detail"] = json!(detail);
                        }
                        Ok(json!({"type":"image_url","image_url":image}))
                    }
                    _ => Err(unsupported()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!(parts))
        }
    }
}
pub fn encode(request: &RequestIR, target: &ContinuityBinding) -> Result<PreparedChat, IrError> {
    if target.route.api != ApiProtocol::ChatCompletions {
        return Err(IrError::WrongProtocol);
    }
    let plan = plan_translation(request, target)?;
    encode_admitted(request, &plan)
}
fn encode_admitted(request: &RequestIR, plan: &TranslationPlan) -> Result<PreparedChat, IrError> {
    for feature in plan.required.iter() {
        if !matches!(
            feature,
            Feature::Instructions
                | Feature::InstructionHierarchy
                | Feature::Images
                | Feature::FunctionTools
                | Feature::StrictToolArguments
                | Feature::StructuredOutput
                | Feature::StrictStructuredOutput
                | Feature::ReasoningEffort
                | Feature::CustomTools
                | Feature::CustomGrammar
                | Feature::NamespacedTools
                | Feature::MaxOutputTokens
                | Feature::Temperature
                | Feature::TopP
                | Feature::ToolChoice
                | Feature::ParallelToolControl
        ) {
            return Err(unsupported());
        }
    }
    for (feature, rule) in [
        (Feature::CustomTools, BridgeRule::CustomToolJson),
        (Feature::NamespacedTools, BridgeRule::ToolNamespace),
        (Feature::CustomGrammar, BridgeRule::CodexPatchGrammar),
    ] {
        if plan.required.contains(feature) && !plan.bridges.contains(&rule) {
            return Err(unsupported());
        }
    }
    let registry = CustomToolBridge::new(request.tools.as_deref().unwrap_or(&[]))?;
    let mut messages = Vec::new();
    if let Some(instructions) = &request.instructions {
        messages.push(json!({"role":"system","content":instructions}));
    }
    let mut pending = BTreeMap::new();
    match &request.input {
        Some(Input::Text(text)) => messages.push(json!({"role":"user","content":text})),
        Some(Input::Items(items)) => {
            for item in items {
                match item {
                    Item::Message(message) => {
                        if !pending.is_empty() {
                            return Err(IrError::InvalidToolMapping);
                        }
                        messages.push(json!({"role":role(message.role),"content":content(&message.content,message.role)?}));
                    }
                    Item::ToolCall(call) => {
                        if messages.last().is_some_and(|v| v["role"] == "tool")
                            && !pending.is_empty()
                        {
                            return Err(IrError::InvalidToolMapping);
                        }
                        let lowered = registry.lower_call(call)?;
                        let ToolInput::Json(raw) = &lowered.input else {
                            return Err(unsupported());
                        };
                        if !serde_json::from_str::<Value>(raw)
                            .map_err(|_| IrError::InvalidJsonArguments)?
                            .is_object()
                        {
                            return Err(IrError::InvalidJsonArguments);
                        }
                        let tool = json!({"id":call.call_id.as_str(),"type":"function","function":{"name":lowered.tool.name,"arguments":raw}});
                        if let Some(last) = messages.last_mut()
                            && last["role"] == "assistant"
                        {
                            if last.get("tool_calls").is_none() {
                                last["tool_calls"] = json!([]);
                            }
                            last["tool_calls"]
                                .as_array_mut()
                                .expect("constructed tool array")
                                .push(tool);
                        } else {
                            messages.push(json!({"role":"assistant","content":Value::Null,"tool_calls":[tool]}));
                        }
                        pending.insert(call.call_id.clone(), call);
                    }
                    Item::ToolResult(result) => {
                        let call = pending
                            .remove(&result.call_id)
                            .ok_or(IrError::InvalidToolMapping)?;
                        registry.lower_result(result, call)?;
                        let text = result.output.as_str().ok_or(unsupported())?;
                        messages.push(json!({"role":"tool","tool_call_id":result.call_id.as_str(),"content":text}));
                    }
                    _ => return Err(unsupported()),
                }
            }
        }
        None => {}
    }
    if messages.is_empty() || !pending.is_empty() {
        return Err(IrError::InvalidToolMapping);
    }
    let mut payload = json!({"model":plan.route.model,"messages":messages,"store":false,"n":1,
        "stream":request.generation.stream.unwrap_or(false),
        "max_completion_tokens":request.generation.max_output_tokens.or(plan.route.max_output_tokens).ok_or(IrError::InvalidField("max_output_tokens"))?});
    if request.generation.stream == Some(true) {
        payload["stream_options"] = json!({"include_usage":true});
    }
    let mut tools = Vec::new();
    for tool in registry.definitions() {
        if tool.identity.name.len() > 64
            || !tool
                .identity
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            return Err(unsupported());
        }
        let ToolDefinitionKind::Function { parameters, strict } = &tool.kind else {
            return Err(unsupported());
        };
        let parameters = parameters.clone().unwrap_or_else(
            || json!({"type":"object","properties":{},"additionalProperties":false}),
        );
        if !parameters.is_object()
            || parameters.get("type").and_then(Value::as_str) != Some("object")
        {
            return Err(unsupported());
        }
        let mut function = json!({"name":tool.identity.name,"parameters":parameters});
        if let Some(description) = &tool.description {
            function["description"] = json!(description);
        }
        if let Some(strict) = strict {
            function["strict"] = json!(strict);
        }
        tools.push(json!({"type":"function","function":function}));
    }
    if tools.is_empty()
        && matches!(
            request.generation.tool_choice,
            Some(ToolChoice::Required | ToolChoice::Named { .. })
        )
    {
        return Err(IrError::InvalidToolMapping);
    }
    if request.tools.is_some() {
        payload["tools"] = json!(tools);
    }
    if let Some(choice) = &request.generation.tool_choice {
        payload["tool_choice"] = match registry.lower_choice(choice)? {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Required => json!("required"),
            ToolChoice::Named { tool, .. } => {
                json!({"type":"function","function":{"name":tool.name}})
            }
            _ => return Err(unsupported()),
        };
    }
    if let Some(parallel) = request.generation.parallel_tool_calls {
        payload["parallel_tool_calls"] = json!(parallel);
    }
    if let Some(output) = &request.generation.output
        && let Some(format) = &output.format
    {
        payload["response_format"] = match format {
            OutputFormat::Text(_) => json!({"type":"text"}),
            OutputFormat::JsonObject(_) => json!({"type":"json_object"}),
            OutputFormat::JsonSchema {
                name,
                schema,
                strict,
                ..
            } => {
                if name.is_empty()
                    || name.len() > 64
                    || !name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                    || !schema.is_object()
                {
                    return Err(IrError::InvalidField("response_format"));
                }
                let mut json_schema = json!({"name":name,"schema":schema});
                if let Some(strict) = strict {
                    json_schema["strict"] = json!(strict);
                }
                json!({"type":"json_schema","json_schema":json_schema})
            }
            _ => return Err(unsupported()),
        };
    }
    if let Some(reasoning) = &request.generation.reasoning
        && let Some(effort) = &reasoning.effort
    {
        if !matches!(
            effort.as_str(),
            "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
        ) {
            return Err(unsupported());
        }
        payload["reasoning_effort"] = json!(effort);
    }
    for (key, value, maximum) in [
        ("temperature", &request.generation.temperature, 2.0),
        ("top_p", &request.generation.top_p, 1.0),
    ] {
        if let Some(value) = value {
            if !value.as_f64().is_some_and(|n| (0.0..=maximum).contains(&n)) {
                return Err(unsupported());
            }
            payload[key] = Value::Number(value.clone());
        }
    }
    Ok(PreparedChat {
        payload,
        model: plan.route.model.clone(),
        tools: PreparedTools::new(registry, request),
    })
}
impl PreparedChat {
    pub fn decode_bytes(&self, bytes: &[u8]) -> Result<Value, IrError> {
        self.decode(super::json::decode(bytes)?)
    }
    pub fn decode(&self, value: Value) -> Result<Value, IrError> {
        if string(&value, "object")? != "chat.completion" || string(&value, "model")? != self.model
        {
            return Err(IrError::InvalidField("chat_response"));
        }
        let response_id = ResponseId::new(format!("resp_{}", string(&value, "id")?))?;
        let created = value
            .get("created")
            .and_then(Value::as_u64)
            .ok_or(IrError::InvalidField("created"))?;
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .filter(|v| v.len() == 1)
            .ok_or(IrError::InvalidField("choices"))?;
        let choice = &choices[0];
        if choice.get("index").and_then(Value::as_u64) != Some(0)
            || choice.get("logprobs").is_some_and(|v| !v.is_null())
        {
            return Err(unsupported());
        }
        let message = choice
            .get("message")
            .ok_or(IrError::InvalidField("message"))?;
        known_fields(
            message,
            &[
                "role",
                "content",
                "tool_calls",
                "refusal",
                "annotations",
                "audio",
                "function_call",
            ],
        )?;
        if string(message, "role")? != "assistant" {
            return Err(IrError::InvalidField("role"));
        }
        for field in ["audio", "function_call"] {
            if message.get(field).is_some_and(|v| !v.is_null()) {
                return Err(unsupported());
            }
        }
        if message
            .get("refusal")
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            || message
                .get("annotations")
                .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
        {
            return Err(unsupported());
        }
        let mut validator = EventValidator::new(EventLimits::default())?;
        validator.apply(EventIR::Started {
            id: response_id.clone(),
        })?;
        let mut output = Vec::new();
        if let Some(text) = message.get("content").filter(|v| !v.is_null()) {
            let text = text.as_str().ok_or(unsupported())?;
            let item = ItemId::new(format!("item_{}_0", string(&value, "id")?))?;
            validator.apply(EventIR::ItemStarted {
                id: item.clone(),
                index: OutputIndex(0),
                kind: OutputKind::Message,
            })?;
            validator.apply(EventIR::PartStarted {
                item: item.clone(),
                index: ContentIndex(0),
                kind: EventPartKind::Text,
            })?;
            validator.apply(EventIR::TextDelta {
                item: item.clone(),
                index: ContentIndex(0),
                text: text.into(),
            })?;
            validator.apply(EventIR::PartFinished {
                item: item.clone(),
                index: ContentIndex(0),
            })?;
            validator.apply(EventIR::ItemFinished { item: item.clone() })?;
            output.push(json!({"id":item.as_str(),"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}));
        }
        let mut tool_count = 0;
        if let Some(calls) = message.get("tool_calls").filter(|v| !v.is_null()) {
            let calls = calls
                .as_array()
                .ok_or(IrError::InvalidField("tool_calls"))?;
            for tool in calls {
                known_fields(tool, &["id", "type", "function"])?;
                if string(tool, "type")? != "function" {
                    return Err(unsupported());
                }
                let function = tool
                    .get("function")
                    .ok_or(IrError::InvalidField("function"))?;
                known_fields(function, &["name", "arguments"])?;
                let index = output.len();
                let item = ItemId::new(format!("item_{}_{}", string(&value, "id")?, index))?;
                let call = self.tools.restore(
                    string(function, "name")?,
                    CallId::new(string(tool, "id")?)?,
                    item.clone(),
                    string(function, "arguments")?.into(),
                )?;
                validator.apply(EventIR::ItemStarted {
                    id: item.clone(),
                    index: OutputIndex(u32::try_from(index).map_err(|_| IrError::SizeLimit)?),
                    kind: OutputKind::Tool {
                        tool: call.tool.clone(),
                        call_id: call.call_id.clone(),
                        kind: call.input.kind(),
                    },
                })?;
                let text = match &call.input {
                    ToolInput::Json(text) | ToolInput::Freeform(text) => text,
                };
                validator.apply(EventIR::ArgumentsDelta {
                    item: item.clone(),
                    text: text.clone(),
                })?;
                validator.apply(EventIR::ItemFinished { item })?;
                output.push(tool_output(&call, "completed"));
                tool_count += 1;
            }
        }
        let (terminal, reason) = self.terminal(string(choice, "finish_reason")?, tool_count)?;
        let usage = value
            .get("usage")
            .filter(|v| !v.is_null())
            .map(usage)
            .transpose()?;
        if let Some(usage) = &usage {
            validator.apply(EventIR::UsageUpdated(Usage {
                input_tokens: usage["input_tokens"].as_u64(),
                output_tokens: usage["output_tokens"].as_u64(),
            }))?;
        }
        validator.apply(EventIR::Finished {
            status: terminal,
            reason: reason.map(str::to_owned),
        })?;
        let status = if terminal == Terminal::Completed {
            "completed"
        } else {
            "incomplete"
        };
        if terminal == Terminal::Incomplete {
            for item in &mut output {
                item["status"] = json!("incomplete");
            }
        }
        Ok(
            json!({"id":response_id.as_str(),"object":"response","created_at":created,"model":self.model,"status":status,"output":output,"usage":usage,"error":Value::Null,
            "incomplete_details":reason.map(|reason|json!({"reason":reason}))}),
        )
    }
    fn terminal(
        &self,
        finish: &str,
        count: usize,
    ) -> Result<(Terminal, Option<&'static str>), IrError> {
        let value = match finish {
            "stop" if count == 0 => (Terminal::Completed, None),
            "tool_calls" if count > 0 => (Terminal::Completed, None),
            "length" => (Terminal::Incomplete, Some("max_output_tokens")),
            _ => return Err(unsupported()),
        };
        self.tools
            .validate_count(count, value.0 == Terminal::Completed)?;
        Ok(value)
    }
}
fn usage(value: &Value) -> Result<Value, IrError> {
    let number = |key| {
        value
            .get(key)
            .and_then(Value::as_u64)
            .ok_or(IrError::InvalidField("usage"))
    };
    let input = number("prompt_tokens")?;
    let output = number("completion_tokens")?;
    let total = number("total_tokens")?;
    if input.checked_add(output) != Some(total) {
        return Err(IrError::InvalidField("usage"));
    }
    let mut result = json!({"input_tokens":input,"output_tokens":output,"total_tokens":total});
    for (source, key, target, maximum) in [
        (
            "prompt_tokens_details",
            "cached_tokens",
            "input_tokens_details",
            input,
        ),
        (
            "completion_tokens_details",
            "reasoning_tokens",
            "output_tokens_details",
            output,
        ),
    ] {
        if let Some(details) = value.get(source).filter(|v| !v.is_null()) {
            object(details)?;
            if let Some(count) = details.get(key) {
                let count = count
                    .as_u64()
                    .filter(|v| *v <= maximum)
                    .ok_or(IrError::InvalidField("usage"))?;
                result[target] = json!({key:count});
            }
        }
    }
    Ok(result)
}
