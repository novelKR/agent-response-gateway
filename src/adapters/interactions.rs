//! Gemini Interactions v1 model codec. No SDK, provider storage or tool execution.
use super::{
    json::{decode, known_fields, string},
    toolset::{PreparedTools, tool_output},
};
use crate::ir::{
    capability::*,
    event::{
        ContentIndex, EventIR, EventLimits, EventValidator, OutputIndex, OutputKind,
        PartKind as EventPartKind, Terminal, Usage,
    },
    request::*,
    *,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
mod stream;
pub use stream::InteractionsStream;

pub use crate::ir::continuity::VerifiedProviderHistory as ProviderHistory;
pub struct PreparedInteractions {
    pub payload: Value,
    model: String,
    tools: PreparedTools,
}
pub struct DecodedInteraction {
    pub response: Value,
    pub steps: Vec<Value>,
    pub provider_status: String,
}
fn unsupported() -> IrError {
    IrError::UnsupportedFeature
}
fn texts(c: &Content) -> Result<Vec<Value>, IrError> {
    match c {
        Content::Text(t) => Ok(vec![json!({"type":"text","text":t})]),
        Content::Parts(ps) => ps
            .iter()
            .map(|p| match &p.kind {
                PartKind::Text { text, .. } => Ok(json!({"type":"text","text":text})),
                _ => Err(unsupported()),
            })
            .collect(),
    }
}
/// Conservative schema subset. Reject unsupported constraints rather than weakening them.
fn schema(value: &Value, depth: usize) -> Result<(), IrError> {
    if depth > 64 {
        return Err(IrError::SizeLimit);
    }
    let o = value.as_object().ok_or(unsupported())?;
    for (k, v) in o {
        match k.as_str() {
            "type" => {
                if !v.as_str().is_some_and(|s| {
                    matches!(
                        s,
                        "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
                    )
                }) {
                    return Err(unsupported());
                }
            }
            "title" | "description" => {
                if !v.is_string() {
                    return Err(unsupported());
                }
            }
            "enum" => {
                if !v.as_array().is_some_and(|a| !a.is_empty()) {
                    return Err(unsupported());
                }
            }
            "required" => {
                if !v.as_array().is_some_and(|a| a.iter().all(Value::is_string)) {
                    return Err(unsupported());
                }
            }
            "additionalProperties" => {
                if !v.is_boolean() {
                    return Err(unsupported());
                }
            }
            "properties" => {
                for child in v.as_object().ok_or(unsupported())?.values() {
                    schema(child, depth + 1)?;
                }
            }
            "items" => schema(v, depth + 1)?,
            "anyOf" => {
                if !v.as_array().is_some_and(|a| !a.is_empty()) {
                    return Err(unsupported());
                }
                for child in v.as_array().ok_or(unsupported())? {
                    schema(child, depth + 1)?;
                }
            }
            _ => return Err(unsupported()),
        }
    }
    Ok(())
}
impl PreparedInteractions {
    pub fn encode(
        request: &RequestIR,
        plan: &TranslationPlan,
        history: &ProviderHistory,
    ) -> Result<Self, IrError> {
        if plan.route.api != ApiProtocol::GeminiInteractions {
            return Err(IrError::WrongProtocol);
        }
        for feature in plan.required.iter() {
            if !matches!(
                feature,
                Feature::Instructions
                    | Feature::InstructionHierarchy
                    | Feature::FunctionTools
                    | Feature::CustomTools
                    | Feature::NamespacedTools
                    | Feature::CustomGrammar
                    | Feature::ToolChoice
                    | Feature::ParallelToolControl
                    | Feature::MaxOutputTokens
                    | Feature::ReasoningEffort
                    | Feature::StructuredOutput
                    | Feature::StrictStructuredOutput
            ) {
                return Err(unsupported());
            }
        }
        if plan.required.contains(Feature::InstructionHierarchy)
            && !plan
                .bridges
                .contains(&BridgeRule::GeminiInstructionEnvelope)
        {
            return Err(unsupported());
        }
        for (f, b) in [
            (Feature::CustomTools, BridgeRule::CustomToolJson),
            (Feature::NamespacedTools, BridgeRule::ToolNamespace),
            (Feature::CustomGrammar, BridgeRule::CodexPatchGrammar),
        ] {
            if plan.required.contains(f) && !plan.bridges.contains(&b) {
                return Err(unsupported());
            }
        }
        let registry = bridge::CustomToolBridge::new(request.tools.as_deref().unwrap_or(&[]))?;
        let mut input = Vec::new();
        let mut instructions = Vec::new();
        if let Some(t) = &request.instructions {
            instructions.push(json!({"role":"protocol_default","position":"request","text":t}));
        }
        let mut pending = BTreeMap::new();
        if let Some(Input::Text(t)) = &request.input {
            input.push(json!({"type":"user_input","content":[{"type":"text","text":t}]}));
        }
        if let Some(Input::Items(items)) = &request.input {
            let mut position = 0;
            while position < items.len() {
                if let Some((end, native)) = history.segments.get(&position) {
                    let crate::ir::continuity::NativeReplay::Gemini { version: 1, steps } = native
                    else {
                        return Err(IrError::ContinuityMismatch);
                    };
                    if *end < position || *end > items.len() {
                        return Err(IrError::InvalidToolMapping);
                    }
                    // The public call entries remain available for result identity validation.
                    for item in &items[position..*end] {
                        if let Item::ToolCall(c) = item {
                            pending.insert(c.call_id.clone(), (c, true));
                        }
                    }
                    input.extend(steps.clone());
                    if *end > position {
                        position = *end;
                        continue;
                    }
                }
                match &items[position] {
                    Item::Message(m) if matches!(m.role, Role::System | Role::Developer) => {
                        if !input.is_empty() {
                            return Err(unsupported());
                        }
                        instructions.push(json!({"role":if m.role==Role::System{"system"}else{"developer"},"position":position,"content":texts(&m.content)?}));
                    }
                    Item::Message(m) => {
                        if !pending.is_empty() {
                            return Err(IrError::InvalidToolMapping);
                        }
                        input.push(json!({"type":if m.role==Role::User{"user_input"}else{"model_output"},"content":texts(&m.content)?}));
                    }
                    Item::ToolCall(c) => {
                        let lowered = registry.lower_call(c)?;
                        let ToolInput::Json(raw) = lowered.input else {
                            return Err(unsupported());
                        };
                        let args: Value = serde_json::from_str(&raw)
                            .map_err(|_| IrError::InvalidJsonArguments)?;
                        if !args.is_object() {
                            return Err(IrError::InvalidJsonArguments);
                        }
                        pending.insert(c.call_id.clone(), (c, false));
                        input.push(json!({"type":"function_call","id":c.call_id.as_str(),"name":lowered.tool.name,"arguments":args}));
                    }
                    Item::ToolResult(r) => {
                        let (call, authenticated) = pending
                            .remove(&r.call_id)
                            .ok_or(IrError::InvalidToolMapping)?;
                        // A compact request can omit tool declarations. The durable replay
                        // already authenticated this exact call and its original kind.
                        let result = if authenticated {
                            let kind = match call.input {
                                ToolInput::Json(_) => crate::ir::ToolKind::Function,
                                ToolInput::Freeform(_) => crate::ir::ToolKind::Custom,
                            };
                            if r.kind != kind || !r.extensions.fields.is_empty() {
                                return Err(IrError::InvalidToolMapping);
                            }
                            r.clone()
                        } else {
                            registry.lower_result(r, call)?
                        };
                        input.push(json!({"type":"function_result","call_id":r.call_id.as_str(),"result":tool_result(&result.output)?}));
                    }
                    _ => return Err(unsupported()),
                }
                position += 1;
            }
            if let Some((end, native)) = history.segments.get(&items.len()) {
                let crate::ir::continuity::NativeReplay::Gemini { version: 1, steps } = native
                else {
                    return Err(IrError::ContinuityMismatch);
                };
                if *end != items.len() {
                    return Err(IrError::InvalidToolMapping);
                }
                input.extend(steps.clone());
            }
        }
        if !pending.is_empty() {
            return Err(IrError::InvalidToolMapping);
        }
        let mut tools = Vec::new();
        for t in registry.definitions() {
            let ToolDefinitionKind::Function { parameters, strict } = &t.kind else {
                return Err(unsupported());
            };
            if *strict == Some(true) {
                return Err(unsupported());
            }
            let parameters = parameters
                .clone()
                .unwrap_or(json!({"type":"object","properties":{},"additionalProperties":false}));
            schema(&parameters, 0)?;
            let mut tool =
                json!({"type":"function","name":t.identity.name,"parameters":parameters});
            if let Some(description) = &t.description {
                tool["description"] = json!(description);
            }
            tools.push(tool);
        }
        let g = &request.generation;
        if g.temperature.is_some()
            || g.top_p.is_some()
            || (g.parallel_tool_calls == Some(false) && !tools.is_empty())
        {
            return Err(unsupported());
        }
        if g.max_output_tokens
            .or(plan.route.max_output_tokens)
            .is_none_or(|n| n == 0 || n > i32::MAX as u64)
        {
            return Err(unsupported());
        }
        let mut generation = json!({"max_output_tokens":g.max_output_tokens.or(plan.route.max_output_tokens).ok_or(unsupported())?,"thinking_summaries":"none"});
        if let Some(reasoning) = &g.reasoning {
            if reasoning.summary.is_some() {
                return Err(unsupported());
            }
            if let Some(level) = &reasoning.effort {
                if !matches!(level.as_str(), "minimal" | "low" | "medium" | "high") {
                    return Err(unsupported());
                }
                generation["thinking_level"] = json!(level);
            }
        }
        if let Some(choice) = &g.tool_choice {
            generation["tool_choice"] = match registry.lower_choice(choice)? {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("any"),
                ToolChoice::Named { tool, .. } => {
                    json!({"allowed_tools":{"mode":"any","tools":[tool.name]}})
                }
                _ => return Err(unsupported()),
            };
        }
        if tools.is_empty()
            && matches!(
                g.tool_choice,
                Some(ToolChoice::Required | ToolChoice::Named { .. })
            )
        {
            return Err(IrError::InvalidToolMapping);
        }
        let mut payload = json!({"model":plan.route.model,"input":input,"tools":tools,"store":false,"background":false,"stream":g.stream.unwrap_or(false),"generation_config":generation});
        if !instructions.is_empty() {
            payload["system_instruction"] = json!(
                serde_json::to_string(
                    &json!({"schema":"gateway-gemini-instructions/v1","instructions":instructions})
                )
                .map_err(|_| unsupported())?
            );
        }
        if let Some(out) = &g.output
            && let Some(format) = &out.format
        {
            match format {
                OutputFormat::Text(_) => {}
                OutputFormat::JsonSchema { schema: s, .. } => {
                    schema(s, 0)?;
                    payload["response_format"] =
                        json!({"type":"text","mime_type":"application/json","schema":s});
                }
                _ => return Err(unsupported()),
            }
        }
        Ok(Self {
            payload,
            model: plan.route.model.clone(),
            tools: PreparedTools::new(registry, request),
        })
    }
    pub fn decode_bytes(
        &self,
        bytes: &[u8],
        response_id: &str,
    ) -> Result<DecodedInteraction, IrError> {
        self.decode(decode(bytes)?, response_id)
    }
    pub fn decode(&self, value: Value, response_id: &str) -> Result<DecodedInteraction, IrError> {
        known_fields(
            &value,
            &[
                "id", "object", "model", "status", "steps", "created", "updated", "usage",
            ],
        )?;
        if value
            .get("model")
            .is_some_and(|v| v.as_str() != Some(self.model.as_str()))
            || value
                .get("object")
                .is_some_and(|v| v.as_str() != Some("interaction"))
        {
            return Err(unsupported());
        }
        ItemId::new(string(&value, "id")?)?;
        let status = string(&value, "status")?;
        if !matches!(status, "completed" | "requires_action" | "incomplete") {
            return Err(unsupported());
        }
        let steps = value
            .get("steps")
            .and_then(Value::as_array)
            .ok_or(unsupported())?
            .clone();
        let mut output = Vec::new();
        let mut calls = BTreeSet::new();
        let mut validator = EventValidator::new(EventLimits::default())?;
        validator.apply(EventIR::Started {
            id: ResponseId::new(response_id)?,
        })?;
        for step in &steps {
            match string(step, "type")? {
                "thought" => {
                    known_fields(step, &["type", "signature", "summary"])?;
                    if step.get("signature").is_some_and(|v| !v.is_string())
                        || step
                            .get("summary")
                            .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
                    {
                        return Err(unsupported());
                    }
                }
                "model_output" => {
                    known_fields(step, &["type", "content"])?;
                    let parts = step
                        .get("content")
                        .and_then(Value::as_array)
                        .ok_or(unsupported())?;
                    let mut content = Vec::new();
                    let id = ItemId::new(format!("{response_id}_{}", output.len()))?;
                    let index =
                        OutputIndex(u32::try_from(output.len()).map_err(|_| IrError::SizeLimit)?);
                    validator.apply(EventIR::ItemStarted {
                        id: id.clone(),
                        index,
                        kind: OutputKind::Message,
                    })?;
                    for (i, p) in parts.iter().enumerate() {
                        known_fields(p, &["type", "text", "annotations"])?;
                        if string(p, "type")? != "text"
                            || p.get("annotations")
                                .is_some_and(|a| !a.as_array().is_some_and(Vec::is_empty))
                        {
                            return Err(unsupported());
                        }
                        let text = string(p, "text")?;
                        let pi = ContentIndex(u32::try_from(i).map_err(|_| IrError::SizeLimit)?);
                        validator.apply(EventIR::PartStarted {
                            item: id.clone(),
                            index: pi,
                            kind: EventPartKind::Text,
                        })?;
                        for chunk in super::json::event_text_chunks(text) {
                            validator.apply(EventIR::TextDelta {
                                item: id.clone(),
                                index: pi,
                                text: chunk.into(),
                            })?;
                        }
                        validator.apply(EventIR::PartFinished {
                            item: id.clone(),
                            index: pi,
                        })?;
                        content.push(json!({"type":"output_text","text":text,"annotations":[]}));
                    }
                    validator.apply(EventIR::ItemFinished { item: id.clone() })?;
                    output.push(json!({"id":id.as_str(),"type":"message","role":"assistant","status":"completed","content":content}));
                }
                "function_call" => {
                    known_fields(step, &["type", "id", "name", "arguments"])?;
                    let call_id = CallId::new(string(step, "id")?)?;
                    if !calls.insert(call_id.clone()) {
                        return Err(IrError::DuplicateId);
                    }
                    let args = step
                        .get("arguments")
                        .filter(|v| v.is_object())
                        .ok_or(IrError::InvalidJsonArguments)?;
                    let id = ItemId::new(format!("{response_id}_{}", output.len()))?;
                    let name = string(step, "name")?;
                    let definition = self.payload["tools"]
                        .as_array()
                        .and_then(|tools| tools.iter().find(|t| t["name"] == name))
                        .ok_or(IrError::InvalidToolMapping)?;
                    if !schema_matches(&definition["parameters"], args) {
                        return Err(IrError::InvalidJsonArguments);
                    }
                    let c = self.tools.restore(
                        string(step, "name")?,
                        call_id,
                        id.clone(),
                        serde_json::to_string(args).map_err(|_| unsupported())?,
                    )?;
                    validator.apply(EventIR::ItemStarted {
                        id: id.clone(),
                        index: OutputIndex(
                            u32::try_from(output.len()).map_err(|_| IrError::SizeLimit)?,
                        ),
                        kind: OutputKind::Tool {
                            tool: c.tool.clone(),
                            call_id: c.call_id.clone(),
                            kind: c.input.kind(),
                        },
                    })?;
                    let raw = match &c.input {
                        ToolInput::Json(s) | ToolInput::Freeform(s) => s,
                    };
                    for chunk in super::json::event_text_chunks(raw) {
                        validator.apply(EventIR::ArgumentsDelta {
                            item: id.clone(),
                            text: chunk.into(),
                        })?;
                    }
                    validator.apply(EventIR::ItemFinished { item: id })?;
                    output.push(tool_output(&c, "completed"));
                }
                _ => return Err(unsupported()),
            }
        }
        if (status == "requires_action") != !calls.is_empty() {
            return Err(IrError::InvalidToolMapping);
        }
        self.tools
            .validate_count(calls.len(), status != "incomplete")?;
        if status == "completed"
            && let Some(schema) = self
                .payload
                .get("response_format")
                .and_then(|f| f.get("schema"))
        {
            let text: String = output
                .iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flatten()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            let document = decode(text.as_bytes())?;
            if !schema_matches(schema, &document) {
                return Err(IrError::InvalidField("structured_output"));
            }
        }
        let usage = usage(value.get("usage"))?;
        validator.apply(EventIR::UsageUpdated(Usage {
            input_tokens: usage["input_tokens"].as_u64(),
            output_tokens: usage["output_tokens"].as_u64(),
        }))?;
        validator.apply(EventIR::Finished {
            status: if status == "incomplete" {
                Terminal::Incomplete
            } else {
                Terminal::Completed
            },
            reason: if status == "incomplete" {
                Some("max_output_tokens".into())
            } else {
                None
            },
        })?;
        Ok(DecodedInteraction {
            steps,
            provider_status: status.into(),
            response: json!({"id":response_id,"object":"response","model":self.model,"created_at":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|unsupported())?.as_secs(),"status":if status=="incomplete"{"incomplete"}else{"completed"},"output":output,"usage":usage,"error":null,"incomplete_details":if status=="incomplete"{json!({"reason":"max_output_tokens"})}else{Value::Null}}),
        })
    }
    pub fn stream(&self, limit: usize, response_id: String) -> InteractionsStream<'_> {
        InteractionsStream::new(self, limit, response_id)
    }
}
fn usage(v: Option<&Value>) -> Result<Value, IrError> {
    let Some(v) = v else {
        return Ok(json!({"input_tokens":null,"output_tokens":null,"total_tokens":null}));
    };
    known_fields(
        v,
        &[
            "total_input_tokens",
            "total_output_tokens",
            "total_thought_tokens",
            "total_cached_tokens",
            "total_tokens",
            "total_tool_use_tokens",
            "input_tokens_by_modality",
            "output_tokens_by_modality",
            "cached_tokens_by_modality",
        ],
    )?;
    let count = |k: &str| -> Result<Option<u64>, IrError> {
        v.get(k)
            .map(|n| n.as_u64().ok_or(unsupported()))
            .transpose()
    };
    let input = count("total_input_tokens")?;
    let visible = count("total_output_tokens")?;
    let thought = count("total_thought_tokens")?;
    let total = count("total_tokens")?;
    let cached = count("total_cached_tokens")?;
    if count("total_tool_use_tokens")?.is_some_and(|n| n != 0) {
        return Err(unsupported());
    }
    let output = visible
        .zip(thought)
        .map(|(a, b)| a.checked_add(b).ok_or(IrError::SizeLimit))
        .transpose()?;
    if input
        .zip(output)
        .zip(total)
        .is_some_and(|((a, b), t)| a.checked_add(b) != Some(t))
        || cached.zip(input).is_some_and(|(a, b)| a > b)
    {
        return Err(unsupported());
    }
    Ok(
        json!({"input_tokens":input,"output_tokens":output,"total_tokens":total,"input_tokens_details":{"cached_tokens":cached},"output_tokens_details":{"reasoning_tokens":thought}}),
    )
}

fn tool_result(value: &Value) -> Result<Value, IrError> {
    match value {
        Value::String(_) | Value::Object(_) => Ok(value.clone()),
        Value::Array(parts) => Ok(Value::Array(
            parts
                .iter()
                .map(|part| {
                    known_fields(part, &["type", "text"])?;
                    if !matches!(string(part, "type")?, "input_text" | "output_text" | "text") {
                        return Err(unsupported());
                    }
                    Ok(json!({"type":"text","text":string(part,"text")?}))
                })
                .collect::<Result<Vec<_>, IrError>>()?,
        )),
        _ => Err(unsupported()),
    }
}

fn schema_matches(schema: &Value, value: &Value) -> bool {
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.contains(value))
        || schema
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.iter().any(|s| schema_matches(s, value)))
    {
        return false;
    }
    if let Some(t) = schema.get("type").and_then(Value::as_str) {
        let matches = match t {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => {
                value.as_i64().is_some()
                    || value.as_u64().is_some()
                    || value.as_f64().is_some_and(|n| n.fract() == 0.0)
            }
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    if let Some(object) = value.as_object() {
        if schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| {
                required
                    .iter()
                    .any(|k| !object.contains_key(k.as_str().expect("validated schema")))
            })
        {
            return false;
        }
        for (key, value) in object {
            if let Some(child) = schema.get("properties").and_then(|p| p.get(key)) {
                if !schema_matches(child, value) {
                    return false;
                }
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return false;
            }
        }
    }
    if let (Some(items), Some(child)) = (value.as_array(), schema.get("items"))
        && items.iter().any(|value| !schema_matches(child, value))
    {
        return false;
    }
    true
}
