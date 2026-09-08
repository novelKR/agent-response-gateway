use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::{IrError, ToolIdentity, ToolKind, grammar::Grammar, request::*};

struct Binding {
    original: ToolIdentity,
    grammar: Option<Grammar>,
}

/// One deterministic request-scoped registry for definitions, choices, history and output.
/// Custom tools use a JSON string envelope; namespace members use collision-free flat names.
/// Grammar checks validate syntax only and never execute a tool.
pub struct CustomToolBridge {
    forward: BTreeMap<ToolIdentity, ToolIdentity>,
    reverse: BTreeMap<ToolIdentity, Binding>,
    definitions: Vec<ToolDefinition>,
}

impl CustomToolBridge {
    pub fn new(tools: &[ToolDefinition]) -> Result<Self, IrError> {
        let mut leaves = Vec::new();
        let mut groups = BTreeSet::new();
        for tool in tools {
            tool.identity.validate()?;
            if let ToolDefinitionKind::Namespace { tools: children } = &tool.kind {
                if tool.identity.namespace.is_some()
                    || children.is_empty()
                    || !groups.insert(&tool.identity.name)
                {
                    return Err(IrError::InvalidToolMapping);
                }
                if !tool.extensions.fields.is_empty() {
                    return Err(IrError::UnsupportedExtension);
                }
                for child in children {
                    if child.identity.namespace.as_ref() != Some(&tool.identity.name)
                        || matches!(child.kind, ToolDefinitionKind::Namespace { .. })
                    {
                        return Err(IrError::InvalidToolMapping);
                    }
                    let mut child = child.clone();
                    if let Some(description) = &tool.description {
                        child.description = Some(json!({"namespace":tool.identity.name,
                            "namespace_description":description,"tool_description":child.description}).to_string());
                    }
                    leaves.push(child);
                }
            } else {
                leaves.push(tool.clone());
            }
        }
        let mut occupied: BTreeSet<String> =
            leaves.iter().map(|t| t.identity.name.clone()).collect();
        let mut forward = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        let mut definitions = Vec::new();
        let mut sequence = 0;
        for tool in leaves {
            tool.identity.validate()?;
            if forward.contains_key(&tool.identity) {
                return Err(IrError::DuplicateId);
            }
            if !tool.extensions.fields.is_empty() {
                return Err(IrError::UnsupportedExtension);
            }
            let grammar = match &tool.kind {
                ToolDefinitionKind::Custom { format } => {
                    Some(Grammar::from_format(format.as_ref())?)
                }
                ToolDefinitionKind::Function { .. } => None,
                ToolDefinitionKind::Namespace { .. } => return Err(IrError::InvalidToolMapping),
            };
            let alias = if grammar.is_some() || tool.identity.namespace.is_some() {
                loop {
                    let prefix = if grammar.is_some() {
                        "arg_custom"
                    } else {
                        "arg_namespaced"
                    };
                    let candidate = format!("{prefix}_{sequence}");
                    sequence += 1;
                    if occupied.insert(candidate.clone()) {
                        break ToolIdentity::new(None, candidate)?;
                    }
                }
            } else {
                tool.identity.clone()
            };
            let mut definition = tool.clone();
            definition.identity = alias.clone();
            if let ToolDefinitionKind::Custom { format } = &tool.kind {
                let mut input = json!({"type":"string"});
                if grammar == Some(Grammar::CodexPatchV1) {
                    input["description"] = json!(format!(
                        "The exact text must match this grammar: {}",
                        format.as_ref().expect("registered grammar")
                    ));
                }
                definition.kind = ToolDefinitionKind::Function {
                    parameters: Some(
                        json!({"type":"object","properties":{"input":input},"required":["input"],"additionalProperties":false}),
                    ),
                    strict: None,
                };
            }
            forward.insert(tool.identity.clone(), alias.clone());
            reverse.insert(
                alias,
                Binding {
                    original: tool.identity,
                    grammar,
                },
            );
            definitions.push(definition);
        }
        Ok(Self {
            forward,
            reverse,
            definitions,
        })
    }

    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }
    pub fn alias(&self, original: &ToolIdentity) -> Result<&ToolIdentity, IrError> {
        self.forward
            .get(original)
            .ok_or(IrError::InvalidToolMapping)
    }
    pub fn original(&self, alias: &ToolIdentity) -> Result<(&ToolIdentity, ToolKind), IrError> {
        let binding = self.reverse.get(alias).ok_or(IrError::InvalidToolMapping)?;
        Ok((
            &binding.original,
            if binding.grammar.is_some() {
                ToolKind::Custom
            } else {
                ToolKind::Function
            },
        ))
    }
    fn binding(&self, original: &ToolIdentity) -> Result<&Binding, IrError> {
        self.reverse
            .get(self.alias(original)?)
            .ok_or(IrError::InvalidToolMapping)
    }

    pub fn lower_call(&self, original: &ToolCall) -> Result<ToolCall, IrError> {
        if !original.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        let binding = self.binding(&original.tool)?;
        let input = match (&original.input, binding.grammar) {
            (ToolInput::Freeform(text), Some(grammar)) => {
                grammar.validate(text)?;
                ToolInput::Json(json!({"input":text}).to_string())
            }
            (ToolInput::Json(raw), None) => ToolInput::Json(raw.clone()),
            _ => return Err(IrError::InvalidToolMapping),
        };
        Ok(ToolCall {
            item_id: original.item_id.clone(),
            call_id: original.call_id.clone(),
            tool: self.alias(&original.tool)?.clone(),
            input,
            extensions: Extensions::responses(),
        })
    }

    pub fn restore_call(&self, lowered: &ToolCall) -> Result<ToolCall, IrError> {
        let ToolInput::Json(raw) = &lowered.input else {
            return Err(IrError::InvalidToolMapping);
        };
        if !lowered.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        let binding = self
            .reverse
            .get(&lowered.tool)
            .ok_or(IrError::InvalidToolMapping)?;
        let input = if let Some(grammar) = binding.grammar {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Envelope {
                input: String,
            }
            // Parse directly from raw JSON so duplicate wrapper fields cannot be erased.
            let envelope: Envelope =
                serde_json::from_str(raw).map_err(|_| IrError::InvalidToolMapping)?;
            grammar.validate(&envelope.input)?;
            ToolInput::Freeform(envelope.input)
        } else {
            if !serde_json::from_str::<serde_json::Value>(raw)
                .map_err(|_| IrError::InvalidJsonArguments)?
                .is_object()
            {
                return Err(IrError::InvalidJsonArguments);
            }
            ToolInput::Json(raw.clone())
        };
        Ok(ToolCall {
            item_id: lowered.item_id.clone(),
            call_id: lowered.call_id.clone(),
            tool: binding.original.clone(),
            input,
            extensions: Extensions::responses(),
        })
    }

    pub fn lower_choice(&self, choice: &ToolChoice) -> Result<ToolChoice, IrError> {
        match choice {
            ToolChoice::Named {
                tool,
                kind,
                extensions,
            } => {
                if !extensions.fields.is_empty() {
                    return Err(IrError::UnsupportedExtension);
                }
                let expected = if self.binding(tool)?.grammar.is_some() {
                    ToolKind::Custom
                } else {
                    ToolKind::Function
                };
                if *kind != expected {
                    return Err(IrError::InvalidToolMapping);
                }
                Ok(ToolChoice::Named {
                    tool: self.alias(tool)?.clone(),
                    kind: ToolKind::Function,
                    extensions: Extensions::responses(),
                })
            }
            ToolChoice::Extension { .. } => Err(IrError::UnsupportedExtension),
            _ => Ok(choice.clone()),
        }
    }
    pub fn lower_result(
        &self,
        result: &ToolResult,
        call: &ToolCall,
    ) -> Result<ToolResult, IrError> {
        self.result(result, call, false)
    }
    pub fn restore_result(
        &self,
        result: &ToolResult,
        original_call: &ToolCall,
    ) -> Result<ToolResult, IrError> {
        self.result(result, original_call, true)
    }
    fn result(
        &self,
        result: &ToolResult,
        call: &ToolCall,
        restore: bool,
    ) -> Result<ToolResult, IrError> {
        self.lower_call(call)?;
        let kind = if self.binding(&call.tool)?.grammar.is_some() {
            ToolKind::Custom
        } else {
            ToolKind::Function
        };
        if result.call_id != call.call_id
            || result.kind != if restore { ToolKind::Function } else { kind }
        {
            return Err(IrError::InvalidToolMapping);
        }
        if !result.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        let mut converted = result.clone();
        converted.kind = if restore { kind } else { ToolKind::Function };
        Ok(converted)
    }
}
