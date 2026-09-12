use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::{IrError, ToolIdentity, ToolKind, grammar::Grammar, request::*};

struct Binding {
    normalize: bool,
    structured: bool,
    original: ToolIdentity,
    grammar: Option<Grammar>,
    wrapped: bool,
}

/// One deterministic request-scoped registry for definitions, choices, history and output.
/// Custom tools use a JSON string envelope; namespace members use collision-free flat names.
/// Grammar checks validate syntax only and never execute a tool.
pub struct CustomToolBridge {
    code_mode: bool,
    forward: BTreeMap<ToolIdentity, ToolIdentity>,
    reverse: BTreeMap<ToolIdentity, Binding>,
    definitions: Vec<ToolDefinition>,
    normalized: std::sync::Mutex<BTreeSet<super::CallId>>,
}

impl CustomToolBridge {
    pub fn new(tools: &[ToolDefinition]) -> Result<Self, IrError> {
        Self::build(tools, true, true, false, false)
    }

    /// Responses may independently retain custom input and namespace representations.
    pub fn for_responses(
        tools: &[ToolDefinition],
        profile: &super::capability::CapabilityProfile,
    ) -> Result<Self, IrError> {
        use super::capability::{BridgeRule, Feature, Support};
        Self::build(
            tools,
            profile.support(Feature::CustomTools) == Support::Bridged(BridgeRule::CustomToolJson),
            profile.support(Feature::NamespacedTools)
                == Support::Bridged(BridgeRule::ToolNamespace),
            profile.support(Feature::CustomGrammar)
                == Support::Bridged(BridgeRule::RegisteredGrammarValidation),
            false,
        )
    }

    fn build(
        tools: &[ToolDefinition],
        wrap_custom: bool,
        flatten_namespaces: bool,
        strip_grammar: bool,
        code_mode: bool,
    ) -> Result<Self, IrError> {
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
                    if flatten_namespaces && let Some(description) = &tool.description {
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
                ToolDefinitionKind::Custom { format } => Some(Grammar::from_format_for_contract(
                    format.as_ref(),
                    code_mode,
                )?),
                ToolDefinitionKind::Function { .. } => None,
                ToolDefinitionKind::Namespace { .. } => return Err(IrError::InvalidToolMapping),
            };
            let wrapped = grammar.is_some() && wrap_custom;
            let alias = if wrapped || (flatten_namespaces && tool.identity.namespace.is_some()) {
                loop {
                    let prefix = if grammar.is_some() {
                        "arg_custom"
                    } else {
                        "arg_namespaced"
                    };
                    let candidate = format!("{prefix}_{sequence}");
                    sequence += 1;
                    if occupied.insert(candidate.clone()) {
                        break ToolIdentity::new(
                            if flatten_namespaces {
                                None
                            } else {
                                tool.identity.namespace.clone()
                            },
                            candidate,
                        )?;
                    }
                }
            } else {
                tool.identity.clone()
            };
            let mut definition = tool.clone();
            definition.identity = alias.clone();
            if let ToolDefinitionKind::Custom { format } = &tool.kind
                && wrapped
            {
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
            } else if strip_grammar && grammar.is_some_and(|g| g != Grammar::Text) {
                definition.description = Some(
                    json!({
                        "tool_description": tool.description,
                        "registered_grammar": match &tool.kind {
                            ToolDefinitionKind::Custom { format } => format,
                            _ => unreachable!(),
                        },
                    })
                    .to_string(),
                );
                definition.kind = ToolDefinitionKind::Custom {
                    format: Some(json!({"type":"text"})),
                };
            }
            forward.insert(tool.identity.clone(), alias.clone());
            reverse.insert(
                alias,
                Binding {
                    normalize: false,
                    structured: false,
                    original: tool.identity,
                    grammar,
                    wrapped,
                },
            );
            definitions.push(definition);
        }
        if !flatten_namespaces {
            let mut flat = definitions.into_iter();
            definitions = tools
                .iter()
                .map(|tool| {
                    if let ToolDefinitionKind::Namespace { tools: children } = &tool.kind {
                        let mut group = tool.clone();
                        group.kind = ToolDefinitionKind::Namespace {
                            tools: flat.by_ref().take(children.len()).collect(),
                        };
                        group
                    } else {
                        flat.next()
                            .expect("one lowered definition per original leaf")
                    }
                })
                .collect();
        }
        Ok(Self {
            code_mode,
            forward,
            reverse,
            definitions,
            normalized: Default::default(),
        })
    }

    pub(crate) fn for_plan(
        request: &RequestIR,
        profile: &super::capability::CapabilityProfile,
        editing: Option<&crate::editing::Policy>,
        add_alternatives: bool,
    ) -> Result<Self, IrError> {
        use super::capability::{BridgeRule, Feature, Support};
        let responses = profile.protocol == super::ApiProtocol::Responses;
        Self::build(
            request.tools.as_deref().unwrap_or(&[]),
            !responses
                || profile.support(Feature::CustomTools)
                    == Support::Bridged(BridgeRule::CustomToolJson),
            !responses
                || profile.support(Feature::NamespacedTools)
                    == Support::Bridged(BridgeRule::ToolNamespace),
            responses
                && profile.support(Feature::CustomGrammar)
                    == Support::Bridged(BridgeRule::RegisteredGrammarValidation),
            editing.is_some_and(|p| p.client_contract == crate::editing::ClientContract::CodeMode),
        )?
        .with_editing(editing, request, add_alternatives)
    }

    pub(crate) fn with_editing(
        mut self,
        policy: Option<&crate::editing::Policy>,
        request: &RequestIR,
        add_alternatives: bool,
    ) -> Result<Self, IrError> {
        let Some(policy) = policy else {
            return Ok(self);
        };
        policy.validate()?;
        let mut originals = Vec::new();
        for (original, alias) in &self.forward {
            let code_mode = policy.client_contract == crate::editing::ClientContract::CodeMode;
            let selected = if code_mode {
                original.name == "exec" && original.namespace.is_none()
            } else {
                original.name == "apply_patch"
                    && original
                        .namespace
                        .as_deref()
                        .is_none_or(|n| n == "functions")
            };
            if !selected {
                continue;
            }
            let binding = &self.reverse[alias];
            let expected = if code_mode {
                Grammar::CodeModeSourceV1
            } else {
                Grammar::CodexPatchV1
            };
            if binding.grammar != Some(expected) {
                return Err(IrError::InvalidToolMapping);
            }
            if code_mode {
                let declaration = request
                    .tool_definitions()
                    .find(|t| t.identity == *original)
                    .ok_or(IrError::InvalidToolMapping)?;
                if crate::continuation::hex(&crate::digest::sha256(
                    declaration.description.as_deref().unwrap_or("").as_bytes(),
                )) != *policy
                    .client_descriptor_sha256
                    .as_ref()
                    .ok_or(IrError::InvalidToolMapping)?
                {
                    return Err(IrError::InvalidToolMapping);
                }
            }
            originals.push(original.clone());
        }
        if policy.client_contract == crate::editing::ClientContract::CodeMode
            && originals.is_empty()
            && request.tool_definitions().any(|t| {
                t.identity.name == "apply_patch"
                    && t.identity
                        .namespace
                        .as_deref()
                        .is_none_or(|n| n == "functions")
            })
        {
            return Err(IrError::InvalidToolMapping);
        }
        if policy.normalization == crate::editing::Normalization::PatchEnvelope {
            for original in &originals {
                let alias = self
                    .forward
                    .get(original)
                    .ok_or(IrError::InvalidToolMapping)?;
                self.reverse
                    .get_mut(alias)
                    .ok_or(IrError::InvalidToolMapping)?
                    .normalize = true;
            }
        }
        if policy.representation == crate::editing::Representation::PatchText
            || !add_alternatives
            || matches!(
                request.generation.tool_choice,
                Some(ToolChoice::Named { .. } | ToolChoice::None)
            )
        {
            return Ok(self);
        }
        for original in originals {
            let encoded =
                serde_json::to_vec(&(original.namespace.as_deref(), &original.name, policy))
                    .map_err(|_| IrError::InvalidToolMapping)?;
            let digest = crate::continuation::hex(&crate::digest::sha256(&encoded));
            let name = format!("arg_edit_{}", &digest[..32]);
            // A stable synthetic identity is required by native replay. Refuse a collision
            // instead of silently assigning a different historical tool name.
            if self
                .reverse
                .keys()
                .chain(self.forward.keys())
                .any(|t| t.name == name)
            {
                return Err(IrError::InvalidToolMapping);
            }
            let alias = ToolIdentity::new(None, name)?;
            self.definitions.push(ToolDefinition {
                identity: alias.clone(), description: Some("Propose one context-based line edit. The host checks the supplied context and applies the patch; this is not replace-all. Use the original patch tool for other operations.".into()),
                kind: ToolDefinitionKind::Function { parameters: Some(crate::editing::ContextEdit::schema()), strict: None },
                extensions: Extensions::responses(),
            });
            self.reverse.insert(
                alias,
                Binding {
                    normalize: false,
                    structured: true,
                    original,
                    grammar: Some(
                        if policy.client_contract == crate::editing::ClientContract::CodeMode {
                            Grammar::CodeModeSourceV1
                        } else {
                            Grammar::CodexPatchV1
                        },
                    ),
                    wrapped: true,
                },
            );
        }
        Ok(self)
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
        if matches!(
            original.status,
            Some(ToolCallStatus::InProgress | ToolCallStatus::Incomplete)
        ) {
            return Err(IrError::InvalidToolMapping);
        }
        let original_patch = match &original.input {
            ToolInput::Freeform(text)
                if self.binding(&original.tool)?.grammar == Some(Grammar::CodeModeSourceV1) =>
            {
                crate::editing::helper_patch(text).ok()
            }
            ToolInput::Freeform(text) => Some(text.clone()),
            _ => None,
        };
        if let Some(text) = original_patch
            && let Ok(edit) = crate::editing::ContextEdit::from_patch(&text)
            && let Some((alias, _)) = self
                .reverse
                .iter()
                .find(|(_, b)| b.structured && b.original == original.tool)
        {
            return Ok(ToolCall {
                status: original.status,
                item_id: original.item_id.clone(),
                call_id: original.call_id.clone(),
                tool: alias.clone(),
                input: ToolInput::Json(
                    serde_json::to_string(&edit).map_err(|_| IrError::InvalidToolMapping)?,
                ),
                extensions: Extensions::responses(),
            });
        }
        let binding = self.binding(&original.tool)?;
        let input = match (&original.input, binding.grammar) {
            (ToolInput::Freeform(text), Some(grammar)) => {
                grammar.validate(text)?;
                if binding.wrapped {
                    ToolInput::Json(json!({"input":text}).to_string())
                } else {
                    ToolInput::Freeform(text.clone())
                }
            }
            (ToolInput::Json(raw), None) => ToolInput::Json(raw.clone()),
            _ => return Err(IrError::InvalidToolMapping),
        };
        Ok(ToolCall {
            status: original.status,
            item_id: original.item_id.clone(),
            call_id: original.call_id.clone(),
            tool: self.alias(&original.tool)?.clone(),
            input,
            extensions: Extensions::responses(),
        })
    }

    fn normalized_input(
        &self,
        binding: &Binding,
        call: &super::CallId,
        text: &str,
    ) -> Result<String, IrError> {
        if !binding.normalize {
            return Ok(text.to_owned());
        }
        let result = crate::editing::normalize_patch_envelope(text)?;
        if result.applied_rule.is_some() {
            self.normalized
                .lock()
                .map_err(|_| IrError::InvalidToolMapping)?
                .insert(call.clone());
        }
        Ok(result.text.into_owned())
    }
    pub fn normalization_evidence(&self) -> Result<crate::editing::NormalizationEvidence, IrError> {
        let count = self
            .normalized
            .lock()
            .map_err(|_| IrError::InvalidToolMapping)?
            .len();
        Ok(crate::editing::NormalizationEvidence {
            rule: if count == 0 {
                None
            } else {
                Some("patch-envelope/v1")
            },
            distinct_calls: count,
        })
    }

    pub fn restore_call(&self, lowered: &ToolCall) -> Result<ToolCall, IrError> {
        if !lowered.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        let binding = self
            .reverse
            .get(&lowered.tool)
            .ok_or(IrError::InvalidToolMapping)?;
        let input = if binding.structured {
            let ToolInput::Json(raw) = &lowered.input else {
                return Err(IrError::InvalidToolMapping);
            };
            let patch = crate::editing::ContextEdit::from_json(raw)?.compile()?;
            ToolInput::Freeform(if binding.grammar == Some(Grammar::CodeModeSourceV1) {
                crate::editing::helper_program(&patch)?
            } else {
                patch
            })
        } else if let Some(grammar) = binding.grammar {
            if !binding.wrapped {
                let ToolInput::Freeform(text) = &lowered.input else {
                    return Err(IrError::InvalidToolMapping);
                };
                let text = self.normalized_input(binding, &lowered.call_id, text)?;
                grammar.validate(&text)?;
                ToolInput::Freeform(text)
            } else {
                let ToolInput::Json(raw) = &lowered.input else {
                    return Err(IrError::InvalidToolMapping);
                };
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Envelope {
                    input: String,
                }
                // Parse directly from raw JSON so duplicate wrapper fields cannot be erased.
                let envelope: Envelope =
                    serde_json::from_str(raw).map_err(|_| IrError::InvalidToolMapping)?;
                let text = self.normalized_input(binding, &lowered.call_id, &envelope.input)?;
                grammar.validate(&text)?;
                ToolInput::Freeform(text)
            }
        } else {
            let ToolInput::Json(raw) = &lowered.input else {
                return Err(IrError::InvalidToolMapping);
            };
            if !serde_json::from_str::<serde_json::Value>(raw)
                .map_err(|_| IrError::InvalidJsonArguments)?
                .is_object()
            {
                return Err(IrError::InvalidJsonArguments);
            }
            ToolInput::Json(raw.clone())
        };
        Ok(ToolCall {
            status: lowered.status,
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
                    kind: if self.binding(tool)?.wrapped {
                        ToolKind::Function
                    } else {
                        *kind
                    },
                    extensions: Extensions::responses(),
                })
            }
            ToolChoice::Extension { .. } => Err(IrError::UnsupportedExtension),
            _ => Ok(choice.clone()),
        }
    }
    pub(crate) fn result_text<'a>(
        &self,
        result: &'a ToolResult,
        call: &ToolCall,
    ) -> Result<std::borrow::Cow<'a, str>, IrError> {
        // Pending native calls are authenticated independently of current declarations.
        // Compaction can omit every tool definition while retaining their results.
        if self.code_mode
            && call.tool.namespace.is_none()
            && call.tool.name == "exec"
            && matches!(call.input, ToolInput::Freeform(_))
        {
            Ok(std::borrow::Cow::Owned(crate::editing::code_mode_result(
                &result.output,
            )?))
        } else {
            result
                .output
                .as_str()
                .map(std::borrow::Cow::Borrowed)
                .ok_or(IrError::UnsupportedFeature)
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
        let lowered = self.lower_call(call)?;
        let kind = if self.binding(&call.tool)?.grammar.is_some() {
            ToolKind::Custom
        } else {
            ToolKind::Function
        };
        let lowered_kind = if matches!(lowered.input, ToolInput::Json(_)) {
            ToolKind::Function
        } else {
            kind
        };
        if result.call_id != call.call_id
            || result.kind != if restore { lowered_kind } else { kind }
        {
            return Err(IrError::InvalidToolMapping);
        }
        if !result.extensions.fields.is_empty() {
            return Err(IrError::UnsupportedExtension);
        }
        let mut converted = result.clone();
        if self.binding(&call.tool)?.grammar == Some(Grammar::CodeModeSourceV1) {
            converted.output = if restore {
                crate::editing::code_mode_result_parts(
                    result.output.as_str().ok_or(IrError::InvalidToolMapping)?,
                )?
            } else {
                serde_json::Value::String(crate::editing::code_mode_result(&result.output)?)
            };
        }
        converted.kind = if restore { kind } else { lowered_kind };
        Ok(converted)
    }
}

#[cfg(test)]
mod replay_result_tests {
    use super::*;
    #[test]
    fn authenticated_results_do_not_require_current_tool_declarations() {
        let registry = CustomToolBridge::new(&[]).unwrap();
        let call = ToolCall {
            call_id: super::super::CallId::new("synthetic").unwrap(),
            tool: ToolIdentity::new(Some("fixture".into()), "echo").unwrap(),
            input: ToolInput::Json("{}".into()),
            item_id: None,
            status: Some(ToolCallStatus::Completed),
            extensions: Extensions::responses(),
        };
        let mut result = ToolResult {
            call_id: call.call_id.clone(),
            kind: ToolKind::Function,
            output: json!("synthetic-result"),
            item_id: None,
            extensions: Extensions::responses(),
        };
        assert_eq!(
            registry.result_text(&result, &call).unwrap(),
            "synthetic-result"
        );
        result.output = json!([{"type":"input_text","text":"synthetic"}]);
        assert!(registry.result_text(&result, &call).is_err());
        let helper = CustomToolBridge::build(&[], true, true, false, true).unwrap();
        let call = ToolCall {
            tool: ToolIdentity::new(None, "exec").unwrap(),
            input: ToolInput::Freeform("synthetic".into()),
            ..call
        };
        assert!(helper.result_text(&result, &call).is_ok());
        assert!(registry.result_text(&result, &call).is_err());
    }
}

#[cfg(test)]
mod normalization_tests {
    use super::*;

    #[test]
    fn normalization_evidence_deduplicates_validation_of_the_same_call() {
        let original = ToolIdentity::new(None, "apply_patch").unwrap();
        let alias = ToolIdentity::new(None, "synthetic_patch").unwrap();
        let registry = CustomToolBridge {
            code_mode: false,
            forward: BTreeMap::from([(original.clone(), alias.clone())]),
            reverse: BTreeMap::from([(
                alias.clone(),
                Binding {
                    original,
                    grammar: Some(Grammar::CodexPatchV1),
                    structured: false,
                    wrapped: true,
                    normalize: true,
                },
            )]),
            definitions: vec![],
            normalized: Default::default(),
        };
        let call = ToolCall { status: Some(ToolCallStatus::Completed), item_id: None,
            call_id: super::super::CallId::new("call_synthetic").unwrap(), tool: alias,
            input: ToolInput::Json(json!({"input":"*** Begin Patch ***\n*** Add File: synthetic.txt\n+data\n*** End Patch ***"}).to_string()), extensions: Extensions::responses() };
        for _ in 0..2 {
            let restored = registry.restore_call(&call).unwrap();
            assert!(
                restored.input
                    == ToolInput::Freeform(
                        "*** Begin Patch\n*** Add File: synthetic.txt\n+data\n*** End Patch".into()
                    )
            );
        }
        assert_eq!(
            registry.normalization_evidence().unwrap(),
            crate::editing::NormalizationEvidence {
                rule: Some("patch-envelope/v1"),
                distinct_calls: 1
            }
        );
    }
}
