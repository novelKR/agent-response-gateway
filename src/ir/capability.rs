use std::collections::{BTreeMap, BTreeSet};

use super::{
    ApiProtocol, IrError,
    continuity::{ContinuityBinding, RouteSnapshot},
    request::*,
};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Instructions,
    InstructionHierarchy,
    Images,
    FunctionTools,
    StrictToolArguments,
    CustomTools,
    CustomGrammar,
    NamespacedTools,
    StructuredToolOutput,
    ToolChoice,
    ParallelToolControl,
    StructuredOutput,
    StrictStructuredOutput,
    MaxOutputTokens,
    Temperature,
    TopP,
    ReasoningEffort,
    ReasoningSummary,
    ReasoningItems,
    OpaqueContinuation,
    Extensions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeRule {
    CustomToolJson,
    ToolNamespace,
    CodexPatchGrammar,
    MessagesInstructionEnvelope,
    GeminiInstructionEnvelope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    Native,
    Bridged(BridgeRule),
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityProfile {
    pub id: String,
    pub version: String,
    pub protocol: ApiProtocol,
    pub support: BTreeMap<Feature, Support>,
}

impl CapabilityProfile {
    pub fn validate(&self) -> Result<(), IrError> {
        if self.id.is_empty() || self.version.is_empty() {
            return Err(IrError::InvalidField("capability_profile"));
        }
        for (feature, support) in &self.support {
            match support {
                Support::Bridged(BridgeRule::CustomToolJson)
                    if *feature == Feature::CustomTools => {}
                Support::Bridged(BridgeRule::ToolNamespace)
                    if *feature == Feature::NamespacedTools => {}
                Support::Bridged(BridgeRule::CodexPatchGrammar)
                    if *feature == Feature::CustomGrammar => {}
                Support::Bridged(BridgeRule::GeminiInstructionEnvelope)
                    if *feature == Feature::InstructionHierarchy
                        && self.protocol == ApiProtocol::GeminiInteractions => {}
                Support::Bridged(BridgeRule::MessagesInstructionEnvelope)
                    if *feature == Feature::InstructionHierarchy
                        && self.protocol == ApiProtocol::Messages => {}
                Support::Bridged(_) => return Err(IrError::UnsupportedFeature),
                _ => {}
            }
        }
        Ok(())
    }
    pub fn support(&self, feature: Feature) -> Support {
        self.support
            .get(&feature)
            .copied()
            .unwrap_or(Support::Unsupported)
    }
}

/// Derived only from a validated request; callers cannot omit inconvenient requirements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredCapabilities(BTreeSet<Feature>);
impl RequiredCapabilities {
    pub fn contains(&self, feature: Feature) -> bool {
        self.0.contains(&feature)
    }
    pub fn iter(&self) -> impl Iterator<Item = Feature> + '_ {
        self.0.iter().copied()
    }
}

fn extensions(
    source: ApiProtocol,
    ext: &Extensions,
    set: &mut BTreeSet<Feature>,
) -> Result<(), IrError> {
    if ext.protocol != source {
        return Err(IrError::WrongProtocol);
    }
    if !ext.fields.is_empty() {
        set.insert(Feature::Extensions);
    }
    Ok(())
}

fn parts(source: ApiProtocol, items: &[Part], set: &mut BTreeSet<Feature>) -> Result<(), IrError> {
    for part in items {
        extensions(source, &part.extensions, set)?;
        if part.annotations.as_ref().is_some_and(|v| !v.is_empty()) {
            set.insert(Feature::Extensions);
        }
        match &part.kind {
            PartKind::Image { .. } => {
                set.insert(Feature::Images);
            }
            PartKind::Extension(_) => {
                set.insert(Feature::Extensions);
            }
            PartKind::Text { .. } => {}
        }
    }
    Ok(())
}

pub fn requirements(request: &RequestIR) -> Result<RequiredCapabilities, IrError> {
    request.validate()?;
    let mut set = BTreeSet::new();
    let source = request.source;
    extensions(source, &request.extensions, &mut set)?;
    if request.instructions.is_some() {
        set.insert(Feature::Instructions);
    }
    for instruction in request.instructions() {
        if instruction.role != InstructionRole::ProtocolDefault {
            set.insert(Feature::InstructionHierarchy);
        }
    }
    for item in request.input.iter().flat_map(|input| match input {
        Input::Items(items) => items.as_slice(),
        _ => &[],
    }) {
        match item {
            Item::Message(message) => {
                extensions(source, &message.extensions, &mut set)?;
                if message
                    .status
                    .is_some_and(|v| v != super::request::ToolCallStatus::Completed)
                {
                    return Err(IrError::UnsupportedFeature);
                }
                if let Content::Parts(items) = &message.content {
                    parts(source, items, &mut set)?;
                }
            }
            Item::ToolCall(call) => {
                set.insert(match call.input {
                    ToolInput::Json(_) => Feature::FunctionTools,
                    ToolInput::Freeform(_) => Feature::CustomTools,
                });
                if call.tool.namespace.is_some() {
                    set.insert(Feature::NamespacedTools);
                }
                extensions(source, &call.extensions, &mut set)?;
            }
            Item::ToolResult(result) => {
                if !result.output.is_string() {
                    set.insert(Feature::StructuredToolOutput);
                }
                extensions(source, &result.extensions, &mut set)?;
            }
            Item::Reasoning(reasoning) => {
                set.insert(Feature::ReasoningItems);
                if reasoning.opaque.is_some() {
                    set.insert(Feature::OpaqueContinuation);
                }
                if let Some(summary) = &reasoning.summary {
                    parts(source, summary, &mut set)?;
                }
                extensions(source, &reasoning.extensions, &mut set)?;
            }
            Item::Extension { protocol, .. } => {
                if *protocol != source {
                    return Err(IrError::WrongProtocol);
                }
                set.insert(Feature::Extensions);
            }
        }
    }
    for group in request.tools.iter().flatten() {
        if matches!(group.kind, ToolDefinitionKind::Namespace { .. }) {
            extensions(source, &group.extensions, &mut set)?;
            set.insert(Feature::NamespacedTools);
        }
    }
    for tool in request.tool_definitions() {
        extensions(source, &tool.extensions, &mut set)?;
        if tool.identity.namespace.is_some() {
            set.insert(Feature::NamespacedTools);
        }
        match &tool.kind {
            ToolDefinitionKind::Function { strict, .. } => {
                set.insert(Feature::FunctionTools);
                if strict == &Some(true) {
                    set.insert(Feature::StrictToolArguments);
                }
            }
            ToolDefinitionKind::Custom { .. } => {
                set.insert(Feature::CustomTools);
                if tool.needs_grammar() {
                    set.insert(Feature::CustomGrammar);
                }
            }
            ToolDefinitionKind::Namespace { .. } => return Err(IrError::InvalidToolMapping),
        }
    }
    let g = &request.generation;
    for (present, feature) in [
        (g.max_output_tokens.is_some(), Feature::MaxOutputTokens),
        (g.temperature.is_some(), Feature::Temperature),
        (g.top_p.is_some(), Feature::TopP),
        (
            g.parallel_tool_calls.is_some(),
            Feature::ParallelToolControl,
        ),
    ] {
        if present {
            set.insert(feature);
        }
    }
    if let Some(choice) = &g.tool_choice {
        set.insert(Feature::ToolChoice);
        match choice {
            ToolChoice::Named {
                tool,
                extensions: ext,
                ..
            } => {
                if tool.namespace.is_some() {
                    set.insert(Feature::NamespacedTools);
                }
                extensions(source, ext, &mut set)?;
            }
            ToolChoice::Extension { protocol, .. } => {
                if *protocol != source {
                    return Err(IrError::WrongProtocol);
                }
                set.insert(Feature::Extensions);
            }
            _ => {}
        }
    }
    if let Some(output) = &g.output {
        extensions(source, &output.extensions, &mut set)?;
        if let Some(format) = &output.format {
            match format {
                OutputFormat::Text(ext) => extensions(source, ext, &mut set)?,
                OutputFormat::JsonObject(ext) => {
                    set.insert(Feature::StructuredOutput);
                    extensions(source, ext, &mut set)?;
                }
                OutputFormat::JsonSchema {
                    strict,
                    extensions: ext,
                    ..
                } => {
                    set.insert(Feature::StructuredOutput);
                    if strict == &Some(true) {
                        set.insert(Feature::StrictStructuredOutput);
                    }
                    extensions(source, ext, &mut set)?;
                }
                OutputFormat::Extension { protocol, .. } => {
                    if *protocol != source {
                        return Err(IrError::WrongProtocol);
                    }
                    set.insert(Feature::Extensions);
                }
            }
        }
    }
    if let Some(reasoning) = &g.reasoning {
        if reasoning.effort.is_some() {
            set.insert(Feature::ReasoningEffort);
        }
        if reasoning.summary.is_some() {
            set.insert(Feature::ReasoningSummary);
        }
        extensions(source, &reasoning.extensions, &mut set)?;
    }
    Ok(RequiredCapabilities(set))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslationPlan {
    pub route: RouteSnapshot,
    pub required: RequiredCapabilities,
    pub bridges: Vec<BridgeRule>,
    pub retains_source_extensions: bool,
}

/// No network, token counting, provider selection, or model qualification occurs here.
pub fn plan_translation(
    request: &RequestIR,
    target: &ContinuityBinding,
) -> Result<TranslationPlan, IrError> {
    plan_translation_with_history(
        request,
        target,
        &super::continuity::VerifiedProviderHistory::default(),
    )
}

pub(crate) fn plan_translation_with_history(
    request: &super::request::RequestIR,
    target: &super::continuity::ContinuityBinding,
    history: &super::continuity::VerifiedProviderHistory,
) -> Result<TranslationPlan, IrError> {
    target.validate()?;
    let required = requirements(request)?;
    let route = &target.route;
    if let (Some(requested), Some(limit)) = (
        request.generation.max_output_tokens,
        route.max_output_tokens,
    ) && requested > limit
    {
        return Err(IrError::UnsupportedFeature);
    }
    let mut bridges = Vec::new();
    for feature in required.iter() {
        if feature == Feature::Extensions {
            if route.api != request.source {
                return Err(IrError::UnsupportedExtension);
            }
            continue;
        }
        match route.capabilities.support(feature) {
            Support::Native => {}
            Support::Bridged(
                rule @ (BridgeRule::MessagesInstructionEnvelope
                | BridgeRule::GeminiInstructionEnvelope),
            ) => {
                bridges.push(rule);
            }
            Support::Bridged(
                rule @ (BridgeRule::CustomToolJson
                | BridgeRule::ToolNamespace
                | BridgeRule::CodexPatchGrammar),
            ) => {
                if route.capabilities.support(Feature::FunctionTools) != Support::Native
                    || (rule == BridgeRule::CodexPatchGrammar
                        && route.capabilities.support(Feature::CustomTools)
                            != Support::Bridged(BridgeRule::CustomToolJson))
                {
                    return Err(IrError::UnsupportedFeature);
                }
                bridges.push(rule);
            }
            Support::Unsupported => return Err(IrError::UnsupportedFeature),
        }
    }
    if bridges.iter().any(|rule| {
        matches!(
            rule,
            BridgeRule::CustomToolJson | BridgeRule::ToolNamespace | BridgeRule::CodexPatchGrammar
        )
    }) {
        let registry =
            super::bridge::CustomToolBridge::new(request.tools.as_deref().unwrap_or(&[]))?;
        if let Some(Input::Items(items)) = &request.input {
            for (index, item) in items.iter().enumerate() {
                if let Item::ToolCall(call) = item {
                    if history
                        .segments
                        .iter()
                        .any(|(start, (end, _))| *start <= index && index < *end)
                    {
                        continue;
                    }
                    registry.lower_call(call)?;
                }
            }
        }
    }
    if let Some(Input::Items(items)) = &request.input {
        for item in items {
            if let Item::Reasoning(reasoning) = item
                && let Some(state) = &reasoning.opaque
            {
                state.replay(target, state.format())?;
            }
        }
    }
    Ok(TranslationPlan {
        route: route.clone(),
        retains_source_extensions: required.contains(Feature::Extensions),
        required,
        bridges,
    })
}
