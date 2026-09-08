use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::{IrError, ToolIdentity, ToolKind, request::*};

/// Deterministic per-request name mapping. Does not execute tools or enforce grammars.
pub struct CustomToolBridge {
    forward: BTreeMap<ToolIdentity, ToolIdentity>,
    reverse: BTreeMap<ToolIdentity, ToolIdentity>,
    definitions: Vec<ToolDefinition>,
}

impl CustomToolBridge {
    pub fn new(tools: &[ToolDefinition]) -> Result<Self, IrError> {
        let mut identities = BTreeSet::new();
        let mut occupied: BTreeSet<String> =
            tools.iter().map(|t| t.identity.name.clone()).collect();
        let mut forward = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        let mut definitions = Vec::new();
        let mut sequence = 0;
        for tool in tools {
            tool.identity.validate()?;
            if !identities.insert(tool.identity.clone()) {
                return Err(IrError::DuplicateId);
            }
            if let ToolDefinitionKind::Custom { format } = &tool.kind {
                if !tool.extensions.fields.is_empty()
                    || format
                        .as_ref()
                        .is_some_and(|v| v != &json!({"type":"text"}))
                {
                    return Err(IrError::UnsupportedFeature);
                }
                let alias = loop {
                    let candidate = format!("arg_custom_{sequence}");
                    sequence += 1;
                    if occupied.insert(candidate.clone()) {
                        break ToolIdentity::new(None, candidate)?;
                    }
                };
                forward.insert(tool.identity.clone(), alias.clone());
                reverse.insert(alias.clone(), tool.identity.clone());
                definitions.push(ToolDefinition {
                    identity: alias,
                    description: tool.description.clone(),
                    kind: ToolDefinitionKind::Function {
                        parameters: Some(json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false})),
                        strict: None,
                    },
                    extensions: Extensions::responses(),
                });
            } else {
                definitions.push(tool.clone());
            }
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

    pub fn lower_call(&self, original: &ToolCall) -> Result<ToolCall, IrError> {
        let ToolInput::Freeform(text) = &original.input else {
            return Err(IrError::InvalidToolMapping);
        };
        if !original.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        Ok(ToolCall {
            item_id: original.item_id.clone(),
            call_id: original.call_id.clone(),
            tool: self.alias(&original.tool)?.clone(),
            input: ToolInput::Json(json!({"input":text}).to_string()),
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
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            input: String,
        }
        let envelope: Envelope =
            serde_json::from_str(raw).map_err(|_| IrError::InvalidToolMapping)?;
        let text = envelope.input;
        let original = self
            .reverse
            .get(&lowered.tool)
            .ok_or(IrError::InvalidToolMapping)?;
        Ok(ToolCall {
            item_id: lowered.item_id.clone(),
            call_id: lowered.call_id.clone(),
            tool: original.clone(),
            input: ToolInput::Freeform(text),
            extensions: Extensions::responses(),
        })
    }

    pub fn lower_choice(&self, choice: &ToolChoice) -> Result<ToolChoice, IrError> {
        match choice {
            ToolChoice::Named {
                tool,
                kind: ToolKind::Custom,
                extensions,
            } => {
                if !extensions.fields.is_empty() {
                    return Err(IrError::UnsupportedExtension);
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
        self.result(result, call, ToolKind::Custom, ToolKind::Function)
    }
    pub fn restore_result(
        &self,
        result: &ToolResult,
        call: &ToolCall,
    ) -> Result<ToolResult, IrError> {
        self.result(result, call, ToolKind::Function, ToolKind::Custom)
    }
    fn result(
        &self,
        result: &ToolResult,
        call: &ToolCall,
        from: ToolKind,
        to: ToolKind,
    ) -> Result<ToolResult, IrError> {
        self.alias(&call.tool)?;
        if result.call_id != call.call_id
            || result.kind != from
            || !matches!(call.input, ToolInput::Freeform(_))
        {
            return Err(IrError::InvalidToolMapping);
        }
        if !result.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        let mut converted = result.clone();
        converted.kind = to;
        Ok(converted)
    }
}
