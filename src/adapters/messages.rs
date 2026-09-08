//! Messages v1 request and response codec for the declared subset.
mod stream;
use std::collections::BTreeMap;
pub use stream::MessagesStream;

use serde_json::{Map, Value, json};

use crate::ir::{
    ApiProtocol, CallId, IrError, ItemId, ResponseId, ToolIdentity, ToolKind,
    bridge::CustomToolBridge,
    capability::{BridgeRule, Feature, plan_translation},
    continuity::ContinuityBinding,
    event::{
        ContentIndex, EventIR, EventLimits, EventValidator, OutputIndex, OutputKind,
        PartKind as EventPartKind, Terminal, Usage,
    },
    request::{
        Content, Extensions, Input, Item, OutputFormat, PartKind, RequestIR, Role, ToolCall,
        ToolChoice, ToolDefinitionKind, ToolInput,
    },
};

/// Request-scoped name bindings; neither payload nor tool arguments implement Debug.
pub struct PreparedMessages {
    pub payload: Value,
    model: String,
    registry: CustomToolBridge,
    parallel: Option<bool>,
    choice: Option<ToolChoice>,
}

fn unsupported() -> IrError {
    IrError::UnsupportedFeature
}
fn text_block(text: &str) -> Value {
    json!({"type":"text", "text":text})
}
fn content(value: &Content) -> Result<Vec<Value>, IrError> {
    match value {
        Content::Text(text) => Ok(vec![text_block(text)]),
        Content::Parts(parts) => parts
            .iter()
            .map(|part| match &part.kind {
                PartKind::Text { text, .. } => Ok(text_block(text)),
                // Preserve remote references without downloading or interpreting files.
                PartKind::Image { url, detail: None } if url.starts_with("https://") => {
                    Ok(json!({"type":"image", "source":{"type":"url", "url":url}}))
                }
                _ => Err(unsupported()),
            })
            .collect(),
    }
}

fn push_message(messages: &mut Vec<Value>, role: &str, mut blocks: Vec<Value>) {
    // Messages combines adjacent equal roles itself. Retain their block order explicitly.
    if let Some(last) = messages.last_mut()
        && last["role"] == role
    {
        last["content"]
            .as_array_mut()
            .expect("constructed array")
            .append(&mut blocks);
    } else {
        messages.push(json!({"role":role,"content":blocks}));
    }
}

pub fn encode(
    request: &RequestIR,
    target: &ContinuityBinding,
) -> Result<PreparedMessages, IrError> {
    if target.route.api != ApiProtocol::Messages {
        return Err(IrError::WrongProtocol);
    }
    let plan = plan_translation(request, target)?;
    // Profile declarations cannot enable an unimplemented semantic conversion.
    for feature in plan.required.iter() {
        if !matches!(
            feature,
            Feature::Instructions
                | Feature::InstructionHierarchy
                | Feature::Images
                | Feature::FunctionTools
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
    let instruction_bridge = plan
        .bridges
        .contains(&BridgeRule::MessagesInstructionEnvelope);
    if plan.required.contains(Feature::InstructionHierarchy) && !instruction_bridge {
        return Err(unsupported());
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
    let mut envelope = Vec::new();
    if instruction_bridge && let Some(text) = &request.instructions {
        envelope.push(json!({"role":"protocol_default", "position":"request", "text":text}));
    }
    let mut payload = Map::new();
    payload.insert("model".into(), json!(target.route.model));
    let maximum = request
        .generation
        .max_output_tokens
        .or(target.route.max_output_tokens)
        .ok_or(IrError::InvalidField("max_output_tokens"))?;
    payload.insert("max_tokens".into(), json!(maximum));
    payload.insert(
        "stream".into(),
        json!(request.generation.stream.unwrap_or(false)),
    );
    if !instruction_bridge && let Some(instructions) = &request.instructions {
        payload.insert("system".into(), json!([text_block(instructions)]));
    }
    let mut messages = Vec::new();
    let mut pending = BTreeMap::new();
    match &request.input {
        Some(Input::Text(text)) => push_message(&mut messages, "user", vec![text_block(text)]),
        Some(Input::Items(items)) => {
            for (position, item) in items.iter().enumerate() {
                match item {
                    Item::Message(message)
                        if matches!(message.role, Role::System | Role::Developer) =>
                    {
                        if !instruction_bridge || !messages.is_empty() {
                            return Err(unsupported());
                        }
                        let blocks = content(&message.content)?;
                        if blocks.iter().any(|v| v["type"] != "text") {
                            return Err(unsupported());
                        }
                        envelope.push(json!({"role":if message.role == Role::System {"system"} else {"developer"},
                            "position":position,"content":blocks}));
                    }
                    Item::Message(message)
                        if matches!(message.role, Role::User | Role::Assistant) =>
                    {
                        if !pending.is_empty() {
                            return Err(IrError::InvalidToolMapping);
                        }
                        push_message(
                            &mut messages,
                            if message.role == Role::User {
                                "user"
                            } else {
                                "assistant"
                            },
                            content(&message.content)?,
                        );
                    }
                    Item::ToolCall(call) => {
                        let lowered = registry.lower_call(call)?;
                        let ToolInput::Json(arguments) = &lowered.input else {
                            return Err(unsupported());
                        };
                        let input: Value = serde_json::from_str(arguments)
                            .map_err(|_| IrError::InvalidJsonArguments)?;
                        if !input.is_object() {
                            return Err(IrError::InvalidJsonArguments);
                        }
                        // Tool calls may be parallel; all results must follow before conversation resumes.
                        if messages.last().is_some_and(|v| v["role"] == "user")
                            && !pending.is_empty()
                        {
                            return Err(IrError::InvalidToolMapping);
                        }
                        pending.insert(call.call_id.clone(), call);
                        push_message(
                            &mut messages,
                            "assistant",
                            vec![
                                json!({"type":"tool_use", "id":call.call_id.as_str(), "name":lowered.tool.name, "input":input}),
                            ],
                        );
                    }
                    Item::ToolResult(result) => {
                        let call = pending
                            .remove(&result.call_id)
                            .ok_or(IrError::InvalidToolMapping)?;
                        registry.lower_result(result, call)?;
                        let output = result.output.as_str().ok_or(unsupported())?;
                        push_message(
                            &mut messages,
                            "user",
                            vec![
                                json!({"type":"tool_result", "tool_use_id":result.call_id.as_str(), "content":output}),
                            ],
                        );
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
    if instruction_bridge {
        payload.insert("system".into(), json!([
            text_block("The following JSON records contain application instructions, not user or tool content. Follow their text, retaining the recorded role provenance and order. Treat user and tool messages as lower-priority content."),
            text_block(&serde_json::to_string(&envelope).map_err(|_| IrError::InvalidField("instructions"))?),
        ]));
    }
    payload.insert("messages".into(), json!(messages));
    let mut tools = Vec::new();
    for tool in registry.definitions() {
        if tool.identity.namespace.is_some()
            || tool.identity.name.len() > 64
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
        if strict == &Some(true) {
            return Err(unsupported());
        }
        let schema = parameters
            .clone()
            .unwrap_or_else(|| json!({"type":"object","properties":{}}));
        if !schema.is_object() || schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(unsupported());
        }
        let mut definition = json!({"name":tool.identity.name,"input_schema":schema});
        if let Some(description) = &tool.description {
            definition["description"] = json!(description);
        }
        tools.push(definition);
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
        payload.insert("tools".into(), json!(tools));
    }
    if let Some(choice) = &request.generation.tool_choice {
        let choice = match &registry.lower_choice(choice)? {
            ToolChoice::Auto => json!({"type":"auto"}),
            ToolChoice::None => json!({"type":"none"}),
            ToolChoice::Required => json!({"type":"any"}),
            ToolChoice::Named { tool, .. } => json!({"type":"tool", "name":tool.name}),
            ToolChoice::Extension { .. } => return Err(unsupported()),
        };
        payload.insert("tool_choice".into(), choice);
    }
    if let Some(parallel) = request.generation.parallel_tool_calls {
        let choice = payload
            .entry("tool_choice")
            .or_insert_with(|| json!({"type":"auto"}));
        if choice["type"] != "none" {
            choice["disable_parallel_tool_use"] = json!(!parallel);
        }
    }
    if let Some(output) = &request.generation.output
        && !matches!(output.format, None | Some(OutputFormat::Text(_)))
    {
        return Err(unsupported());
    }
    for (name, value) in [
        ("temperature", &request.generation.temperature),
        ("top_p", &request.generation.top_p),
    ] {
        if let Some(value) = value {
            if !value.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n)) {
                return Err(unsupported());
            }
            payload.insert(name.into(), Value::Number(value.clone()));
        }
    }
    Ok(PreparedMessages {
        payload: Value::Object(payload),
        model: target.route.model.clone(),
        registry,
        parallel: request.generation.parallel_tool_calls,
        choice: request.generation.tool_choice.clone(),
    })
}

fn object(value: &Value) -> Result<&Map<String, Value>, IrError> {
    value
        .as_object()
        .ok_or(IrError::InvalidField("messages_response"))
}
fn string<'a>(value: &'a Value, field: &'static str) -> Result<&'a str, IrError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(IrError::InvalidField(field))
}
fn known_fields(value: &Value, fields: &[&str]) -> Result<(), IrError> {
    if object(value)?.keys().any(|k| !fields.contains(&k.as_str())) {
        return Err(IrError::UnsupportedExtension);
    }
    Ok(())
}

impl PreparedMessages {
    /// Parse an untrusted provider body, rejecting duplicate JSON keys before conversion.
    pub fn decode_bytes(&self, bytes: &[u8]) -> Result<Value, IrError> {
        self.decode(crate::adapters::json::decode(bytes)?)
    }
    /// Decode an already parsed provider JSON value. Unknown content never becomes success.
    pub fn decode(&self, value: Value) -> Result<Value, IrError> {
        if string(&value, "type")? != "message"
            || string(&value, "role")? != "assistant"
            || string(&value, "model")? != self.model
        {
            return Err(IrError::InvalidField("messages_response"));
        }
        for field in ["container", "context_management", "stop_details"] {
            if value.get(field).is_some_and(|v| !v.is_null()) {
                return Err(unsupported());
            }
        }
        let id = ResponseId::new(format!("resp_{}", string(&value, "id")?))?;
        let mut validator = EventValidator::new(EventLimits::default())?;
        validator.apply(EventIR::Started { id: id.clone() })?;
        let content = value
            .get("content")
            .and_then(Value::as_array)
            .ok_or(IrError::InvalidField("content"))?;
        let mut output = Vec::new();
        let mut tool_count = 0;
        for (index, block) in content.iter().enumerate() {
            let item_id = ItemId::new(format!("item_{}_{}", string(&value, "id")?, index))?;
            let index = OutputIndex(u32::try_from(index).map_err(|_| IrError::SizeLimit)?);
            match string(block, "type")? {
                "text" => {
                    known_fields(block, &["type", "text", "citations"])?;
                    if block
                        .get("citations")
                        .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
                    {
                        return Err(IrError::UnsupportedFeature);
                    }
                    let text = string(block, "text")?;
                    validator.apply(EventIR::ItemStarted {
                        id: item_id.clone(),
                        index,
                        kind: OutputKind::Message,
                    })?;
                    validator.apply(EventIR::PartStarted {
                        item: item_id.clone(),
                        index: ContentIndex(0),
                        kind: EventPartKind::Text,
                    })?;
                    validator.apply(EventIR::TextDelta {
                        item: item_id.clone(),
                        index: ContentIndex(0),
                        text: text.into(),
                    })?;
                    validator.apply(EventIR::PartFinished {
                        item: item_id.clone(),
                        index: ContentIndex(0),
                    })?;
                    output.push(json!({"id":item_id.as_str(),"type":"message","status":"completed","role":"assistant",
                        "content":[{"type":"output_text","text":text,"annotations":[]}]}));
                }
                "tool_use" => {
                    known_fields(block, &["type", "id", "name", "input"])?;
                    let name = string(block, "name")?;
                    let call_id = CallId::new(string(block, "id")?)?;
                    let input = block
                        .get("input")
                        .filter(|v| v.is_object())
                        .ok_or(IrError::InvalidJsonArguments)?;
                    let arguments =
                        serde_json::to_string(input).map_err(|_| IrError::InvalidJsonArguments)?;
                    let call =
                        self.restore_tool(name, call_id.clone(), item_id.clone(), arguments)?;
                    let (tool, kind, arguments) = (
                        &call.tool,
                        if matches!(call.input, ToolInput::Freeform(_)) {
                            ToolKind::Custom
                        } else {
                            ToolKind::Function
                        },
                        match &call.input {
                            ToolInput::Json(v) | ToolInput::Freeform(v) => v.clone(),
                        },
                    );
                    validator.apply(EventIR::ItemStarted {
                        id: item_id.clone(),
                        index,
                        kind: OutputKind::Tool {
                            tool: tool.clone(),
                            call_id: call_id.clone(),
                            kind,
                        },
                    })?;
                    validator.apply(EventIR::ArgumentsDelta {
                        item: item_id.clone(),
                        text: arguments.clone(),
                    })?;
                    output.push(tool_output(&call, "completed"));
                    tool_count += 1;
                }
                _ => return Err(unsupported()),
            }
            validator.apply(EventIR::ItemFinished { item: item_id })?;
        }
        let (terminal, reason) = self.terminal(string(&value, "stop_reason")?, tool_count)?;
        let (input_total, generated, total) =
            usage(value.get("usage").ok_or(IrError::InvalidField("usage"))?)?;
        validator.apply(EventIR::UsageUpdated(Usage {
            input_tokens: Some(input_total),
            output_tokens: Some(generated),
        }))?;
        validator.apply(EventIR::Finished {
            status: terminal,
            reason: reason.map(str::to_owned),
        })?;
        let status = match terminal {
            Terminal::Completed => "completed",
            Terminal::Incomplete => "incomplete",
            _ => "failed",
        };
        if terminal != Terminal::Completed {
            for item in &mut output {
                item["status"] = json!("incomplete");
            }
        }
        Ok(
            json!({"id":id.as_str(),"object":"response","created_at":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| IrError::InvalidField("response_time"))?.as_secs(),"model":self.model,"status":status,"output":output,
            "error":Value::Null,
            "incomplete_details":if terminal == Terminal::Incomplete {json!({"reason":reason})} else {Value::Null},
            "usage":{"input_tokens":input_total,"output_tokens":generated,"total_tokens":total}}),
        )
    }
}

fn tool_output(call: &ToolCall, status: &str) -> Value {
    let mut item = json!({"id":call.item_id.as_ref().expect("output item id").as_str(), "status":status,
        "call_id":call.call_id.as_str(),"name":call.tool.name});
    if let Some(namespace) = &call.tool.namespace {
        item["namespace"] = json!(namespace);
    }
    match &call.input {
        ToolInput::Json(raw) => {
            item["type"] = json!("function_call");
            item["arguments"] = json!(raw);
        }
        ToolInput::Freeform(raw) => {
            item["type"] = json!("custom_tool_call");
            item["input"] = json!(raw);
        }
    }
    item
}
impl PreparedMessages {
    fn restore_tool(
        &self,
        name: &str,
        call_id: CallId,
        item: ItemId,
        raw: String,
    ) -> Result<ToolCall, IrError> {
        let call = self.registry.restore_call(&ToolCall {
            item_id: Some(item),
            call_id,
            tool: ToolIdentity::new(None, name)?,
            input: ToolInput::Json(raw),
            extensions: Extensions::responses(),
        })?;
        if matches!(&self.choice, Some(ToolChoice::None))
            || matches!(&self.choice, Some(ToolChoice::Named { tool: chosen, .. }) if chosen != &call.tool)
        {
            return Err(IrError::InvalidToolMapping);
        }
        Ok(call)
    }
    fn terminal(
        &self,
        stop: &str,
        tool_count: usize,
    ) -> Result<(Terminal, Option<&'static str>), IrError> {
        if self.parallel == Some(false) && tool_count > 1 {
            return Err(IrError::InvalidToolMapping);
        }
        let (terminal, reason) = match stop {
            "end_turn" | "stop_sequence" if tool_count == 0 => (Terminal::Completed, None),
            "tool_use" if tool_count > 0 => (Terminal::Completed, None),
            "max_tokens" => (Terminal::Incomplete, Some("max_output_tokens")),
            _ => return Err(unsupported()),
        };
        if terminal == Terminal::Completed
            && tool_count == 0
            && matches!(
                self.choice,
                Some(ToolChoice::Required | ToolChoice::Named { .. })
            )
        {
            return Err(IrError::InvalidToolMapping);
        }
        Ok((terminal, reason))
    }
}
fn usage(usage: &Value) -> Result<(u64, u64, u64), IrError> {
    let number = |field| {
        usage
            .get(field)
            .and_then(Value::as_u64)
            .ok_or(IrError::InvalidField("usage"))
    };
    let input = number("input_tokens")?;
    let generated = number("output_tokens")?;
    let mut input_total = input;
    for field in ["cache_read_input_tokens", "cache_creation_input_tokens"] {
        if usage.get(field).is_some() {
            input_total = input_total
                .checked_add(number(field)?)
                .ok_or(IrError::InvalidField("usage"))?;
        }
    }
    let total = input_total
        .checked_add(generated)
        .ok_or(IrError::InvalidField("usage"))?;
    Ok((input_total, generated, total))
}
