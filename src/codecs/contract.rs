//! Versioned IPC DTOs. No URL, credential, host path, session key or storage handle.
use crate::ir::{
    ApiProtocol, IrError,
    capability::{BridgeRule, CapabilityProfile, Feature, Support},
    continuity::{NativeReplay, RouteSnapshot, VerifiedProviderHistory},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PROTOCOL: &str = "gateway-api-codec/v1";
pub const EDITING_PROTOCOL: &str = "gateway-api-codec/v2";
pub fn supported_protocol(value: &str) -> bool {
    matches!(value, PROTOCOL | EDITING_PROTOCOL)
}
pub const MAX_FRAME: usize = 128 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub api: ApiProtocol,
    pub model: String,
    pub profile_id: String,
    pub profile_version: String,
    pub reasoning_contract: Option<crate::ir::reasoning::ReasoningContract>,
    pub support: BTreeMap<Feature, String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}
fn support_name(s: Support) -> &'static str {
    match s {
        Support::Native => "native",
        Support::Unsupported => "unsupported",
        Support::Bridged(b) => match b {
            BridgeRule::CustomToolJson => "custom_tool_json",
            BridgeRule::ToolNamespace => "tool_namespace",
            BridgeRule::CodexPatchGrammar => "codex_patch_grammar",
            BridgeRule::RegisteredGrammarValidation => "registered_grammar_validation",
            BridgeRule::MessagesInstructionEnvelope => "messages_instruction_envelope",
            BridgeRule::GeminiInstructionEnvelope => "gemini_instruction_envelope",
            BridgeRule::ChatInstructionEnvelope => "chat_instruction_envelope",
            BridgeRule::ProviderParallelPermission => "provider_parallel_permission",
        },
    }
}
fn support(value: &str) -> Result<Support, IrError> {
    Ok(match value {
        "native" => Support::Native,
        "unsupported" => Support::Unsupported,
        value => Support::Bridged(match value {
            "custom_tool_json" => BridgeRule::CustomToolJson,
            "tool_namespace" => BridgeRule::ToolNamespace,
            "codex_patch_grammar" => BridgeRule::CodexPatchGrammar,
            "registered_grammar_validation" => BridgeRule::RegisteredGrammarValidation,
            "messages_instruction_envelope" => BridgeRule::MessagesInstructionEnvelope,
            "gemini_instruction_envelope" => BridgeRule::GeminiInstructionEnvelope,
            "chat_instruction_envelope" => BridgeRule::ChatInstructionEnvelope,
            "provider_parallel_permission" => BridgeRule::ProviderParallelPermission,
            _ => return Err(IrError::UnsupportedVersion),
        }),
    })
}
impl Route {
    pub(crate) fn from_snapshot(s: &RouteSnapshot) -> Self {
        Self {
            api: s.api,
            model: s.model.clone(),
            profile_id: s.capabilities.id.clone(),
            profile_version: s.capabilities.version.clone(),
            reasoning_contract: s.capabilities.reasoning_contract.clone(),
            support: s
                .capabilities
                .support
                .iter()
                .map(|(f, s)| (*f, support_name(*s).into()))
                .collect(),
            context_window: s.context_window,
            max_output_tokens: s.max_output_tokens,
        }
    }
    pub(crate) fn snapshot(&self) -> Result<RouteSnapshot, IrError> {
        let value = RouteSnapshot {
            provider_id: "codec-host".into(),
            model: self.model.clone(),
            api: self.api,
            credential_binding: "withheld".into(),
            adapter_version: PROTOCOL.into(),
            capabilities: CapabilityProfile {
                id: self.profile_id.clone(),
                version: self.profile_version.clone(),
                protocol: self.api,
                reasoning_contract: self.reasoning_contract.clone(),
                support: self
                    .support
                    .iter()
                    .map(|(f, s)| Ok((*f, support(s)?)))
                    .collect::<Result<_, IrError>>()?,
            },
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySpan {
    pub start: usize,
    pub end: usize,
    pub native: NativeReplay,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing: Option<crate::editing::Policy>,
    pub request: Value,
    pub route: Route,
    pub managed: bool,
    pub pending_tools: bool,
    pub history: Vec<ReplaySpan>,
    pub max_output_bytes: usize,
}
impl Prepare {
    pub(crate) fn history(&self) -> Result<VerifiedProviderHistory, IrError> {
        let mut result = VerifiedProviderHistory::default();
        let mut end = 0;
        let length = self.request["input"].as_array().map_or(0, Vec::len);
        for span in &self.history {
            span.native.validate()?;
            if !self.managed || span.start < end || span.start >= span.end || span.end > length {
                return Err(IrError::ContinuityMismatch);
            }
            result
                .segments
                .insert(span.start, (span.end, span.native.clone()));
            end = span.end;
        }
        Ok(result)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Prepare { value: Box<Prepare> },
    Json { body: String, response_id: String },
    Stream { response_id: String },
    Event { event: String, data: String },
    Finish,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    pub sequence: u64,
    pub operation: Operation,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedResult {
    pub response: Value,
    pub native: NativeReplay,
    pub outcome: crate::ir::continuity::Outcome,
    pub accounting: crate::adapters::managed::Accounting,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultValue {
    Ready {
        apis: Vec<ApiProtocol>,
        replay_versions: Vec<u32>,
    },
    Prepared {
        payload: Value,
    },
    Json {
        response: Value,
    },
    Managed {
        value: Box<ManagedResult>,
    },
    Progress {
        events: Vec<Value>,
        complete: bool,
        accounting: Option<crate::adapters::managed::Accounting>,
    },
    Finished,
    Rejected,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub protocol: String,
    pub sequence: u64,
    pub value: ResultValue,
}
