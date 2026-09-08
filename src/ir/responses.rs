//! Pure Responses request projection. This codec is intentionally not in the HTTP path.
use serde_json::{Map, Number, Value};

use super::{
    ApiProtocol, CallId, IrError, ItemId, ToolIdentity, ToolKind, VERSION,
    continuity::{ContinuityBinding, OpaqueState},
    request::*,
};

pub const OPAQUE_FORMAT: &str = "responses.encrypted_content/v1";

fn object(value: Value, field: &'static str) -> Result<Map<String, Value>, IrError> {
    match value {
        Value::Object(value) => Ok(value),
        _ => Err(IrError::InvalidField(field)),
    }
}
fn take(fields: &mut Map<String, Value>, key: &str) -> Option<Value> {
    // Preserve an explicit optional null as an extension instead of erasing it.
    if fields.get(key).is_some_and(Value::is_null) {
        None
    } else {
        fields.remove(key)
    }
}
fn string(fields: &mut Map<String, Value>, key: &'static str) -> Result<Option<String>, IrError> {
    take(fields, key)
        .map(|v| match v {
            Value::String(s) => Ok(s),
            _ => Err(IrError::InvalidField(key)),
        })
        .transpose()
}
fn required(fields: &mut Map<String, Value>, key: &'static str) -> Result<String, IrError> {
    string(fields, key)?.ok_or(IrError::InvalidField(key))
}
fn boolean(fields: &mut Map<String, Value>, key: &'static str) -> Result<Option<bool>, IrError> {
    take(fields, key)
        .map(|v| v.as_bool().ok_or(IrError::InvalidField(key)))
        .transpose()
}
fn number(fields: &mut Map<String, Value>, key: &'static str) -> Result<Option<Number>, IrError> {
    take(fields, key)
        .map(|v| match v {
            Value::Number(n) => Ok(n),
            _ => Err(IrError::InvalidField(key)),
        })
        .transpose()
}
fn item_id(fields: &mut Map<String, Value>) -> Result<Option<ItemId>, IrError> {
    string(fields, "id")?.map(ItemId::new).transpose()
}
fn ext(fields: Map<String, Value>) -> Extensions {
    Extensions {
        protocol: ApiProtocol::Responses,
        fields,
    }
}
fn tool_identity(fields: &mut Map<String, Value>) -> Result<ToolIdentity, IrError> {
    ToolIdentity::new(string(fields, "namespace")?, required(fields, "name")?)
}
fn array(value: Value, field: &'static str) -> Result<Vec<Value>, IrError> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(IrError::InvalidField(field)),
    }
}

fn decode_part(value: Value) -> Result<Part, IrError> {
    let mut fields = object(value.clone(), "content_part")?;
    let kind = match required(&mut fields, "type")?.as_str() {
        "input_text" => PartKind::Text {
            kind: TextKind::Input,
            text: required(&mut fields, "text")?,
        },
        "output_text" => PartKind::Text {
            kind: TextKind::Output,
            text: required(&mut fields, "text")?,
        },
        "summary_text" => PartKind::Text {
            kind: TextKind::Summary,
            text: required(&mut fields, "text")?,
        },
        "input_image" => PartKind::Image {
            url: required(&mut fields, "image_url")?,
            detail: string(&mut fields, "detail")?,
        },
        _ => {
            return Ok(Part {
                kind: PartKind::Extension(value),
                extensions: Extensions::responses(),
            });
        }
    };
    Ok(Part {
        kind,
        extensions: ext(fields),
    })
}
fn decode_content(value: Value) -> Result<Content, IrError> {
    match value {
        Value::String(text) => Ok(Content::Text(text)),
        Value::Array(parts) => Ok(Content::Parts(
            parts
                .into_iter()
                .map(decode_part)
                .collect::<Result<_, _>>()?,
        )),
        _ => Err(IrError::InvalidField("content")),
    }
}

fn decode_item(value: Value, binding: Option<&ContinuityBinding>) -> Result<Item, IrError> {
    let mut fields = object(value.clone(), "input_item")?;
    let tag = string(&mut fields, "type")?;
    if tag.as_deref() == Some("message") || (tag.is_none() && fields.contains_key("role")) {
        let role = match required(&mut fields, "role")?.as_str() {
            "system" => Role::System,
            "developer" => Role::Developer,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => return Err(IrError::InvalidField("role")),
        };
        return Ok(Item::Message(Message {
            id: item_id(&mut fields)?,
            role,
            content: decode_content(
                fields
                    .remove("content")
                    .ok_or(IrError::InvalidField("content"))?,
            )?,
            explicit_type: tag.is_some(),
            extensions: ext(fields),
        }));
    }
    match tag.as_deref() {
        Some("function_call" | "custom_tool_call") => {
            let item_id = item_id(&mut fields)?;
            let call_id = CallId::new(required(&mut fields, "call_id")?)?;
            let tool = tool_identity(&mut fields)?;
            let input = if tag.as_deref() == Some("function_call") {
                ToolInput::Json(required(&mut fields, "arguments")?)
            } else {
                ToolInput::Freeform(required(&mut fields, "input")?)
            };
            Ok(Item::ToolCall(ToolCall {
                item_id,
                call_id,
                tool,
                input,
                extensions: ext(fields),
            }))
        }
        Some("function_call_output" | "custom_tool_call_output") => {
            let item_id = item_id(&mut fields)?;
            let call_id = CallId::new(required(&mut fields, "call_id")?)?;
            let output = fields
                .remove("output")
                .ok_or(IrError::InvalidField("output"))?;
            Ok(Item::ToolResult(ToolResult {
                item_id,
                call_id,
                output,
                kind: if tag.as_deref() == Some("function_call_output") {
                    ToolKind::Function
                } else {
                    ToolKind::Custom
                },
                extensions: ext(fields),
            }))
        }
        Some("reasoning") => {
            let id = item_id(&mut fields)?;
            let summary = take(&mut fields, "summary")
                .map(|v| {
                    array(v, "summary")?
                        .into_iter()
                        .map(decode_part)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            let opaque = string(&mut fields, "encrypted_content")?
                .map(|text| {
                    let binding = binding.ok_or(IrError::UnboundOpaqueState)?;
                    if binding.route.api != ApiProtocol::Responses {
                        return Err(IrError::WrongProtocol);
                    }
                    OpaqueState::new(binding.clone(), OPAQUE_FORMAT, text.into_bytes())
                })
                .transpose()?;
            Ok(Item::Reasoning(Box::new(ReasoningItem {
                id,
                summary,
                opaque,
                extensions: ext(fields),
            })))
        }
        Some(_) => Ok(Item::Extension {
            value,
            protocol: ApiProtocol::Responses,
        }),
        None => Err(IrError::InvalidField("input_item_type")),
    }
}

fn decode_tool(value: Value) -> Result<ToolDefinition, IrError> {
    let mut fields = object(value, "tool")?;
    let tag = required(&mut fields, "type")?;
    let identity = tool_identity(&mut fields)?;
    let description = string(&mut fields, "description")?;
    let kind = match tag.as_str() {
        "function" => ToolDefinitionKind::Function {
            parameters: take(&mut fields, "parameters"),
            strict: boolean(&mut fields, "strict")?,
        },
        "custom" => ToolDefinitionKind::Custom {
            format: take(&mut fields, "format"),
        },
        _ => return Err(IrError::UnsupportedFeature),
    };
    Ok(ToolDefinition {
        identity,
        description,
        kind,
        extensions: ext(fields),
    })
}
fn decode_choice(value: Value) -> Result<ToolChoice, IrError> {
    match value.as_str() {
        Some("auto") => return Ok(ToolChoice::Auto),
        Some("none") => return Ok(ToolChoice::None),
        Some("required") => return Ok(ToolChoice::Required),
        _ => {}
    }
    if let Value::Object(mut fields) = value.clone() {
        let kind = match fields.get("type").and_then(Value::as_str) {
            Some("function") => Some(ToolKind::Function),
            Some("custom") => Some(ToolKind::Custom),
            _ => None,
        };
        if let Some(kind) = kind {
            fields.remove("type");
            return Ok(ToolChoice::Named {
                tool: tool_identity(&mut fields)?,
                kind,
                extensions: ext(fields),
            });
        }
    }
    Ok(ToolChoice::Extension {
        value,
        protocol: ApiProtocol::Responses,
    })
}
fn decode_output(value: Value) -> Result<OutputOptions, IrError> {
    let mut fields = object(value, "text")?;
    let format = take(&mut fields, "format")
        .map(|value| {
            let mut fields = object(value.clone(), "format")?;
            match required(&mut fields, "type")?.as_str() {
                "text" => Ok(OutputFormat::Text(ext(fields))),
                "json_object" => Ok(OutputFormat::JsonObject(ext(fields))),
                "json_schema" => Ok(OutputFormat::JsonSchema {
                    name: required(&mut fields, "name")?,
                    schema: fields
                        .remove("schema")
                        .ok_or(IrError::InvalidField("schema"))?,
                    strict: boolean(&mut fields, "strict")?,
                    extensions: ext(fields),
                }),
                _ => Ok(OutputFormat::Extension {
                    value,
                    protocol: ApiProtocol::Responses,
                }),
            }
        })
        .transpose()?;
    Ok(OutputOptions {
        format,
        extensions: ext(fields),
    })
}

pub fn decode(value: Value, binding: Option<&ContinuityBinding>) -> Result<RequestIR, IrError> {
    let (mut fields, model, _) =
        crate::responses_policy::normalize_stateless(value).map_err(|e| {
            if e.code == "unsupported_feature" {
                IrError::UnsupportedFeature
            } else {
                IrError::InvalidField("responses_request")
            }
        })?;
    fields.remove("model");
    fields.remove("store");
    let instructions = string(&mut fields, "instructions")?;
    let input = take(&mut fields, "input")
        .map(|value| match value {
            Value::String(text) => Ok(Input::Text(text)),
            Value::Array(items) => Ok(Input::Items(
                items
                    .into_iter()
                    .map(|v| decode_item(v, binding))
                    .collect::<Result<_, _>>()?,
            )),
            _ => Err(IrError::InvalidField("input")),
        })
        .transpose()?;
    let tools = take(&mut fields, "tools")
        .map(|value| {
            array(value, "tools")?
                .into_iter()
                .map(decode_tool)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let generation = GenerationOptions {
        stream: boolean(&mut fields, "stream")?,
        max_output_tokens: take(&mut fields, "max_output_tokens")
            .map(|v| v.as_u64().ok_or(IrError::InvalidField("max_output_tokens")))
            .transpose()?,
        temperature: number(&mut fields, "temperature")?,
        top_p: number(&mut fields, "top_p")?,
        parallel_tool_calls: boolean(&mut fields, "parallel_tool_calls")?,
        tool_choice: take(&mut fields, "tool_choice")
            .map(decode_choice)
            .transpose()?,
        output: take(&mut fields, "text").map(decode_output).transpose()?,
        reasoning: take(&mut fields, "reasoning")
            .map(|value| {
                let mut fields = object(value, "reasoning")?;
                Ok::<_, IrError>(ReasoningOptions {
                    effort: string(&mut fields, "effort")?,
                    summary: string(&mut fields, "summary")?,
                    extensions: ext(fields),
                })
            })
            .transpose()?,
    };
    let request = RequestIR {
        version: VERSION,
        source: ApiProtocol::Responses,
        model,
        instructions,
        input,
        tools,
        generation,
        extensions: ext(fields),
    };
    request.validate()?;
    Ok(request)
}

fn fields(ext: &Extensions) -> Result<Map<String, Value>, IrError> {
    if ext.protocol != ApiProtocol::Responses {
        return Err(IrError::WrongProtocol);
    }
    Ok(ext.fields.clone())
}
fn put(fields: &mut Map<String, Value>, key: &str, value: impl Into<Value>) -> Result<(), IrError> {
    if fields.contains_key(key) {
        return Err(IrError::ExtensionConflict);
    }
    fields.insert(key.into(), value.into());
    Ok(())
}
fn optional(
    fields: &mut Map<String, Value>,
    key: &str,
    value: Option<Value>,
) -> Result<(), IrError> {
    if let Some(value) = value {
        put(fields, key, value)?;
    }
    Ok(())
}
fn put_id(fields: &mut Map<String, Value>, id: Option<&ItemId>) -> Result<(), IrError> {
    optional(fields, "id", id.map(|id| Value::String(id.as_str().into())))
}
fn put_tool(fields: &mut Map<String, Value>, tool: &ToolIdentity) -> Result<(), IrError> {
    put(fields, "name", tool.name.clone())?;
    optional(
        fields,
        "namespace",
        tool.namespace.clone().map(Value::String),
    )
}
fn retained(value: &Value, protocol: ApiProtocol) -> Result<Value, IrError> {
    if protocol != ApiProtocol::Responses {
        return Err(IrError::WrongProtocol);
    }
    Ok(value.clone())
}

fn encode_part(part: &Part) -> Result<Value, IrError> {
    let mut data = fields(&part.extensions)?;
    match &part.kind {
        PartKind::Text { kind, text } => {
            put(
                &mut data,
                "type",
                match kind {
                    TextKind::Input => "input_text",
                    TextKind::Output => "output_text",
                    TextKind::Summary => "summary_text",
                },
            )?;
            put(&mut data, "text", text.clone())?;
        }
        PartKind::Image { url, detail } => {
            put(&mut data, "type", "input_image")?;
            put(&mut data, "image_url", url.clone())?;
            optional(&mut data, "detail", detail.clone().map(Value::String))?;
        }
        PartKind::Extension(value) => {
            if !data.is_empty() {
                return Err(IrError::ExtensionConflict);
            }
            return Ok(value.clone());
        }
    }
    Ok(data.into())
}
fn encode_content(content: &Content) -> Result<Value, IrError> {
    match content {
        Content::Text(text) => Ok(text.clone().into()),
        Content::Parts(parts) => Ok(parts
            .iter()
            .map(encode_part)
            .collect::<Result<Vec<_>, _>>()?
            .into()),
    }
}
fn encode_item(item: &Item, binding: Option<&ContinuityBinding>) -> Result<Value, IrError> {
    let mut data;
    match item {
        Item::Message(message) => {
            data = fields(&message.extensions)?;
            if message.explicit_type {
                put(&mut data, "type", "message")?;
            }
            put_id(&mut data, message.id.as_ref())?;
            put(
                &mut data,
                "role",
                match message.role {
                    Role::System => "system",
                    Role::Developer => "developer",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
            )?;
            put(&mut data, "content", encode_content(&message.content)?)?;
        }
        Item::ToolCall(call) => {
            data = fields(&call.extensions)?;
            put_id(&mut data, call.item_id.as_ref())?;
            put(&mut data, "call_id", call.call_id.as_str())?;
            put_tool(&mut data, &call.tool)?;
            match &call.input {
                ToolInput::Json(raw) => {
                    put(&mut data, "type", "function_call")?;
                    put(&mut data, "arguments", raw.clone())?;
                }
                ToolInput::Freeform(raw) => {
                    put(&mut data, "type", "custom_tool_call")?;
                    put(&mut data, "input", raw.clone())?;
                }
            }
        }
        Item::ToolResult(result) => {
            data = fields(&result.extensions)?;
            put_id(&mut data, result.item_id.as_ref())?;
            put(&mut data, "call_id", result.call_id.as_str())?;
            put(
                &mut data,
                "type",
                match result.kind {
                    ToolKind::Function => "function_call_output",
                    ToolKind::Custom => "custom_tool_call_output",
                },
            )?;
            put(&mut data, "output", result.output.clone())?;
        }
        Item::Reasoning(reasoning) => {
            data = fields(&reasoning.extensions)?;
            put_id(&mut data, reasoning.id.as_ref())?;
            put(&mut data, "type", "reasoning")?;
            if let Some(parts) = &reasoning.summary {
                put(
                    &mut data,
                    "summary",
                    parts
                        .iter()
                        .map(encode_part)
                        .collect::<Result<Vec<_>, _>>()?,
                )?;
            }
            if let Some(opaque) = &reasoning.opaque {
                let target = binding.ok_or(IrError::UnboundOpaqueState)?;
                let raw = opaque.replay(target, OPAQUE_FORMAT)?;
                put(
                    &mut data,
                    "encrypted_content",
                    std::str::from_utf8(raw)
                        .map_err(|_| IrError::InvalidField("opaque_encoding"))?,
                )?;
            }
        }
        Item::Extension { value, protocol } => return retained(value, *protocol),
    }
    Ok(data.into())
}
fn encode_tool(tool: &ToolDefinition) -> Result<Value, IrError> {
    let mut data = fields(&tool.extensions)?;
    put_tool(&mut data, &tool.identity)?;
    optional(
        &mut data,
        "description",
        tool.description.clone().map(Value::String),
    )?;
    match &tool.kind {
        ToolDefinitionKind::Function { parameters, strict } => {
            put(&mut data, "type", "function")?;
            optional(&mut data, "parameters", parameters.clone())?;
            optional(&mut data, "strict", strict.map(Value::Bool))?;
        }
        ToolDefinitionKind::Custom { format } => {
            put(&mut data, "type", "custom")?;
            optional(&mut data, "format", format.clone())?;
        }
    }
    Ok(data.into())
}
fn encode_choice(choice: &ToolChoice) -> Result<Value, IrError> {
    match choice {
        ToolChoice::Auto => Ok("auto".into()),
        ToolChoice::None => Ok("none".into()),
        ToolChoice::Required => Ok("required".into()),
        ToolChoice::Extension { value, protocol } => retained(value, *protocol),
        ToolChoice::Named {
            tool,
            kind,
            extensions,
        } => {
            let mut data = fields(extensions)?;
            put_tool(&mut data, tool)?;
            put(
                &mut data,
                "type",
                match kind {
                    ToolKind::Function => "function",
                    ToolKind::Custom => "custom",
                },
            )?;
            Ok(data.into())
        }
    }
}
fn encode_format(format: &OutputFormat) -> Result<Value, IrError> {
    let (mut data, tag) = match format {
        OutputFormat::Text(ext) => (fields(ext)?, "text"),
        OutputFormat::JsonObject(ext) => (fields(ext)?, "json_object"),
        OutputFormat::JsonSchema {
            name,
            schema,
            strict,
            extensions,
        } => {
            let mut data = fields(extensions)?;
            put(&mut data, "name", name.clone())?;
            put(&mut data, "schema", schema.clone())?;
            optional(&mut data, "strict", strict.map(Value::Bool))?;
            (data, "json_schema")
        }
        OutputFormat::Extension { value, protocol } => return retained(value, *protocol),
    };
    put(&mut data, "type", tag)?;
    Ok(data.into())
}

pub fn encode(request: &RequestIR, binding: Option<&ContinuityBinding>) -> Result<Value, IrError> {
    request.validate()?;
    if request.source != ApiProtocol::Responses {
        return Err(IrError::WrongProtocol);
    }
    let mut data = fields(&request.extensions)?;
    put(&mut data, "model", request.model.clone())?;
    optional(
        &mut data,
        "instructions",
        request.instructions.clone().map(Value::String),
    )?;
    if let Some(input) = &request.input {
        put(
            &mut data,
            "input",
            match input {
                Input::Text(text) => Value::String(text.clone()),
                Input::Items(items) => Value::Array(
                    items
                        .iter()
                        .map(|item| encode_item(item, binding))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            },
        )?;
    }
    if let Some(tools) = &request.tools {
        put(
            &mut data,
            "tools",
            tools
                .iter()
                .map(encode_tool)
                .collect::<Result<Vec<_>, _>>()?,
        )?;
    }
    let g = &request.generation;
    optional(&mut data, "stream", g.stream.map(Value::Bool))?;
    optional(
        &mut data,
        "max_output_tokens",
        g.max_output_tokens.map(Value::from),
    )?;
    optional(
        &mut data,
        "temperature",
        g.temperature.clone().map(Value::Number),
    )?;
    optional(&mut data, "top_p", g.top_p.clone().map(Value::Number))?;
    optional(
        &mut data,
        "parallel_tool_calls",
        g.parallel_tool_calls.map(Value::Bool),
    )?;
    if let Some(choice) = &g.tool_choice {
        put(&mut data, "tool_choice", encode_choice(choice)?)?;
    }
    if let Some(output) = &g.output {
        let mut fields = fields(&output.extensions)?;
        if let Some(format) = &output.format {
            put(&mut fields, "format", encode_format(format)?)?;
        }
        put(&mut data, "text", fields)?;
    }
    if let Some(reasoning) = &g.reasoning {
        let mut fields = fields(&reasoning.extensions)?;
        optional(
            &mut fields,
            "effort",
            reasoning.effort.clone().map(Value::String),
        )?;
        optional(
            &mut fields,
            "summary",
            reasoning.summary.clone().map(Value::String),
        )?;
        put(&mut data, "reasoning", fields)?;
    }
    let (data, _, _) = crate::responses_policy::normalize_stateless(data.into())
        .map_err(|_| IrError::UnsupportedFeature)?;
    Ok(data.into())
}
