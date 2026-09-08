use std::collections::BTreeSet;

use serde_json::{Map, Number, Value};

use super::{
    ApiProtocol, CallId, IrError, ItemId, ToolIdentity, ToolKind, VERSION, continuity::OpaqueState,
};

/// Unknown fields retain their source protocol. No default Debug/serialization of bodies.
#[derive(Clone, PartialEq, Eq)]
pub struct Extensions {
    pub protocol: ApiProtocol,
    pub fields: Map<String, Value>,
}

impl Extensions {
    pub fn responses() -> Self {
        Self {
            protocol: ApiProtocol::Responses,
            fields: Map::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Content {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    Input,
    Output,
    Summary,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PartKind {
    Text { kind: TextKind, text: String },
    Image { url: String, detail: Option<String> },
    Extension(Value),
}

#[derive(Clone, PartialEq, Eq)]
pub struct Part {
    pub annotations: Option<Vec<Value>>,
    pub kind: PartKind,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Message {
    pub status: Option<ToolCallStatus>,
    pub id: Option<ItemId>,
    pub role: Role,
    pub content: Content,
    pub explicit_type: bool,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ToolInput {
    Json(String),
    Freeform(String),
}

impl ToolInput {
    pub fn kind(&self) -> ToolKind {
        match self {
            Self::Json(_) => ToolKind::Function,
            Self::Freeform(_) => ToolKind::Custom,
        }
    }
    pub fn validate(&self) -> Result<(), IrError> {
        if let Self::Json(raw) = self {
            serde_json::from_str::<Value>(raw).map_err(|_| IrError::InvalidJsonArguments)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    InProgress,
    Completed,
    Incomplete,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub status: Option<ToolCallStatus>,
    pub item_id: Option<ItemId>,
    pub call_id: CallId,
    pub tool: ToolIdentity,
    pub input: ToolInput,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub item_id: Option<ItemId>,
    pub call_id: CallId,
    pub kind: ToolKind,
    pub output: Value,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReasoningItem {
    pub id: Option<ItemId>,
    pub summary: Option<Vec<Part>>,
    pub opaque: Option<OpaqueState>,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Item {
    Message(Message),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    Reasoning(Box<ReasoningItem>),
    Extension { value: Value, protocol: ApiProtocol },
}

#[derive(Clone, PartialEq, Eq)]
pub enum Input {
    Text(String),
    Items(Vec<Item>),
}

#[derive(Clone, PartialEq, Eq)]
pub enum ToolDefinitionKind {
    Function {
        parameters: Option<Value>,
        strict: Option<bool>,
    },
    Custom {
        format: Option<Value>,
    },
    Namespace {
        tools: Vec<ToolDefinition>,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub identity: ToolIdentity,
    pub description: Option<String>,
    pub kind: ToolDefinitionKind,
    pub extensions: Extensions,
}

impl ToolDefinition {
    pub fn kind(&self) -> Option<ToolKind> {
        match self.kind {
            ToolDefinitionKind::Function { .. } => Some(ToolKind::Function),
            ToolDefinitionKind::Custom { .. } => Some(ToolKind::Custom),
            ToolDefinitionKind::Namespace { .. } => None,
        }
    }
    pub fn needs_grammar(&self) -> bool {
        matches!(&self.kind, ToolDefinitionKind::Custom { format: Some(value) } if value.get("type").and_then(Value::as_str) != Some("text"))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum OutputFormat {
    Text(Extensions),
    JsonObject(Extensions),
    JsonSchema {
        name: String,
        schema: Value,
        strict: Option<bool>,
        extensions: Extensions,
    },
    Extension {
        value: Value,
        protocol: ApiProtocol,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct OutputOptions {
    pub format: Option<OutputFormat>,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReasoningOptions {
    pub effort: Option<String>,
    pub summary: Option<String>,
    pub extensions: Extensions,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named {
        tool: ToolIdentity,
        kind: ToolKind,
        extensions: Extensions,
    },
    Extension {
        value: Value,
        protocol: ApiProtocol,
    },
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct GenerationOptions {
    pub stream: Option<bool>,
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<Number>,
    pub top_p: Option<Number>,
    pub parallel_tool_calls: Option<bool>,
    pub tool_choice: Option<ToolChoice>,
    pub output: Option<OutputOptions>,
    pub reasoning: Option<ReasoningOptions>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RequestIR {
    pub version: u16,
    pub source: ApiProtocol,
    pub model: String,
    pub instructions: Option<String>,
    pub input: Option<Input>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub generation: GenerationOptions,
    pub extensions: Extensions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionRole {
    ProtocolDefault,
    System,
    Developer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionPosition {
    Request,
    Item(usize),
}
pub enum InstructionBody<'a> {
    Text(&'a str),
    Message(&'a Content),
}
pub struct InstructionRef<'a> {
    pub role: InstructionRole,
    pub position: InstructionPosition,
    pub body: InstructionBody<'a>,
}

fn validate_extensions(
    ext: &Extensions,
    source: ApiProtocol,
    reserved: &[&str],
) -> Result<(), IrError> {
    if ext.protocol != source {
        return Err(IrError::WrongProtocol);
    }
    if reserved
        .iter()
        .any(|key| ext.fields.get(*key).is_some_and(|value| !value.is_null()))
    {
        return Err(IrError::ExtensionConflict);
    }
    Ok(())
}

fn validate_parts(parts: &[Part], source: ApiProtocol) -> Result<(), IrError> {
    for part in parts {
        if part.annotations.is_some()
            && !matches!(
                part.kind,
                PartKind::Text {
                    kind: TextKind::Output,
                    ..
                }
            )
        {
            return Err(IrError::InvalidField("annotations"));
        }
        let reserved: &[&str] = match &part.kind {
            PartKind::Text {
                kind: TextKind::Output,
                ..
            } => &["type", "text", "annotations"],
            PartKind::Text { .. } => &["type", "text"],
            PartKind::Image { .. } => &["type", "image_url", "detail"],
            PartKind::Extension(_) => &[],
        };
        validate_extensions(&part.extensions, source, reserved)?;
        if let PartKind::Extension(value) = &part.kind
            && (!part.extensions.fields.is_empty()
                || matches!(
                    value.get("type").and_then(Value::as_str),
                    Some("input_text" | "output_text" | "summary_text" | "input_image")
                ))
        {
            return Err(IrError::ExtensionConflict);
        }
    }
    Ok(())
}

impl RequestIR {
    /// Ordered leaf definitions with their effective namespace/name identity.
    pub fn tool_definitions(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools
            .iter()
            .flatten()
            .flat_map(|tool| match &tool.kind {
                ToolDefinitionKind::Namespace { tools } => tools.as_slice(),
                _ => std::slice::from_ref(tool),
            })
    }
    /// A derived view, never a second mutable source of instruction ordering.
    pub fn instructions(&self) -> Vec<InstructionRef<'_>> {
        let mut result = Vec::new();
        if let Some(text) = &self.instructions {
            result.push(InstructionRef {
                role: InstructionRole::ProtocolDefault,
                position: InstructionPosition::Request,
                body: InstructionBody::Text(text),
            });
        }
        if let Some(Input::Items(items)) = &self.input {
            for (index, item) in items.iter().enumerate() {
                if let Item::Message(message) = item {
                    let role = match message.role {
                        Role::System => InstructionRole::System,
                        Role::Developer => InstructionRole::Developer,
                        _ => continue,
                    };
                    result.push(InstructionRef {
                        role,
                        position: InstructionPosition::Item(index),
                        body: InstructionBody::Message(&message.content),
                    });
                }
            }
        }
        result
    }

    pub fn validate(&self) -> Result<(), IrError> {
        if self.version != VERSION {
            return Err(IrError::UnsupportedVersion);
        }
        if self.model.is_empty() {
            return Err(IrError::InvalidField("model"));
        }
        if self.generation.max_output_tokens == Some(0) {
            return Err(IrError::InvalidField("max_output_tokens"));
        }
        validate_extensions(
            &self.extensions,
            self.source,
            &[
                "model",
                "instructions",
                "input",
                "tools",
                "stream",
                "store",
                "max_output_tokens",
                "temperature",
                "top_p",
                "parallel_tool_calls",
                "tool_choice",
                "text",
                "reasoning",
            ],
        )?;
        if let Some(output) = &self.generation.output {
            validate_extensions(&output.extensions, self.source, &["format"])?;
            if let Some(format) = &output.format {
                match format {
                    OutputFormat::Text(ext) | OutputFormat::JsonObject(ext) => {
                        validate_extensions(ext, self.source, &["type"])?
                    }
                    OutputFormat::JsonSchema { extensions, .. } => validate_extensions(
                        extensions,
                        self.source,
                        &["type", "name", "schema", "strict"],
                    )?,
                    OutputFormat::Extension { value, protocol } => {
                        if *protocol != self.source {
                            return Err(IrError::WrongProtocol);
                        }
                        if matches!(
                            value.get("type").and_then(Value::as_str),
                            Some("text" | "json_object" | "json_schema")
                        ) {
                            return Err(IrError::ExtensionConflict);
                        }
                    }
                }
            }
        }
        if let Some(reasoning) = &self.generation.reasoning {
            validate_extensions(&reasoning.extensions, self.source, &["effort", "summary"])?;
        }
        if let Some(ToolChoice::Named { extensions, .. }) = &self.generation.tool_choice {
            validate_extensions(extensions, self.source, &["type", "name", "namespace"])?;
        }
        if let Some(ToolChoice::Extension { value, protocol }) = &self.generation.tool_choice {
            if *protocol != self.source {
                return Err(IrError::WrongProtocol);
            }
            if matches!(value.as_str(), Some("auto" | "none" | "required"))
                || matches!(
                    value.get("type").and_then(Value::as_str),
                    Some("function" | "custom")
                )
            {
                return Err(IrError::ExtensionConflict);
            }
        }
        let mut groups = BTreeSet::new();
        for group in self.tools.iter().flatten() {
            if let ToolDefinitionKind::Namespace { tools } = &group.kind {
                group.identity.validate()?;
                if group.identity.namespace.is_some()
                    || tools.is_empty()
                    || !groups.insert(group.identity.name.clone())
                {
                    return Err(IrError::InvalidToolMapping);
                }
                validate_extensions(
                    &group.extensions,
                    self.source,
                    &["type", "name", "description", "tools"],
                )?;
                if tools.iter().any(|tool| {
                    matches!(tool.kind, ToolDefinitionKind::Namespace { .. })
                        || tool.identity.namespace.as_ref() != Some(&group.identity.name)
                }) {
                    return Err(IrError::InvalidToolMapping);
                }
            }
        }
        let mut definitions = BTreeSet::new();
        for tool in self.tool_definitions() {
            tool.identity.validate()?;
            let reserved: &[&str] = match tool.kind {
                ToolDefinitionKind::Function { .. } => &[
                    "type",
                    "name",
                    "namespace",
                    "description",
                    "parameters",
                    "strict",
                ],
                ToolDefinitionKind::Custom { .. } => {
                    &["type", "name", "namespace", "description", "format"]
                }
                ToolDefinitionKind::Namespace { .. } => return Err(IrError::InvalidToolMapping),
            };
            validate_extensions(&tool.extensions, self.source, reserved)?;
            if !definitions.insert(tool.identity.clone()) {
                return Err(IrError::DuplicateId);
            }
        }
        let mut ids = BTreeSet::new();
        let mut calls = std::collections::BTreeMap::new();
        let mut results = BTreeSet::new();
        if let Some(Input::Items(items)) = &self.input {
            for item in items {
                let id = match item {
                    Item::Message(value) => {
                        validate_extensions(
                            &value.extensions,
                            self.source,
                            &["type", "id", "role", "content", "status"],
                        )?;
                        if let Content::Parts(parts) = &value.content {
                            validate_parts(parts, self.source)?;
                        }
                        &value.id
                    }
                    Item::Reasoning(value) => {
                        validate_extensions(
                            &value.extensions,
                            self.source,
                            &["type", "id", "summary", "encrypted_content"],
                        )?;
                        if let Some(parts) = &value.summary {
                            validate_parts(parts, self.source)?;
                        }
                        if let Some(state) = &value.opaque
                            && state.binding().route.api != self.source
                        {
                            return Err(IrError::WrongProtocol);
                        }
                        &value.id
                    }
                    Item::ToolCall(value) => {
                        value.tool.validate()?;
                        value.input.validate()?;
                        let argument_field = match value.input {
                            ToolInput::Json(_) => "arguments",
                            ToolInput::Freeform(_) => "input",
                        };
                        validate_extensions(
                            &value.extensions,
                            self.source,
                            &[
                                "type",
                                "id",
                                "call_id",
                                "name",
                                "namespace",
                                "status",
                                argument_field,
                            ],
                        )?;
                        if calls
                            .insert(value.call_id.clone(), value.input.kind())
                            .is_some()
                        {
                            return Err(IrError::DuplicateId);
                        }
                        &value.item_id
                    }
                    Item::ToolResult(value) => {
                        validate_extensions(
                            &value.extensions,
                            self.source,
                            &["type", "id", "call_id", "output"],
                        )?;
                        if calls.get(&value.call_id) != Some(&value.kind)
                            || !results.insert(value.call_id.clone())
                        {
                            return Err(IrError::InvalidToolMapping);
                        }
                        &value.item_id
                    }
                    Item::Extension { value, protocol } => {
                        if *protocol != self.source {
                            return Err(IrError::WrongProtocol);
                        }
                        if matches!(
                            value.get("type").and_then(Value::as_str),
                            Some(
                                "message"
                                    | "function_call"
                                    | "custom_tool_call"
                                    | "function_call_output"
                                    | "custom_tool_call_output"
                                    | "reasoning"
                                    | "item_reference"
                                    | "compaction"
                                    | "compaction_trigger"
                            )
                        ) {
                            return Err(IrError::ExtensionConflict);
                        }
                        continue;
                    }
                };
                if let Some(id) = id
                    && !ids.insert(id.clone())
                {
                    return Err(IrError::DuplicateId);
                }
            }
        }
        if let Some(ToolChoice::Named { tool, kind, .. }) = &self.generation.tool_choice
            && !self
                .tool_definitions()
                .any(|d| &d.identity == tool && d.kind() == Some(*kind))
        {
            return Err(IrError::InvalidToolMapping);
        }
        Ok(())
    }
}
