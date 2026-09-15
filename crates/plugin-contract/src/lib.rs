//! Portable codec wire types. Nested role values follow the published schemas.
//! This crate has no gateway, transport, storage, or credential dependencies.
//! Deserialization alone is not semantic validation; hosts validate role values.
pub mod provider;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PROTOCOL: &str = "gateway-api-codec/v1";
pub const EDITING_PROTOCOL: &str = "gateway-api-codec/v2";
pub const CAPABILITIES_PROTOCOL: &str = "gateway-api-codec/v3";
pub const PROVIDER_PROTOCOL: &str = "gateway-provider/v1";
pub const CAPABILITIES_SCHEMA: &str = "gateway-plugin-capabilities/v1";
pub const CODEC_APIS: [&str; 4] = [
    "chat_completions",
    "gemini_interactions",
    "messages",
    "responses",
];
pub const ROLE_FEATURES: [&str; 4] = ["editing", "json", "managed_continuation", "streaming"];

/// Exact declarations, separate from package grants and from runtime observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub schema: String,
    pub apis: Vec<String>,
    pub features: Vec<String>,
    pub requires: Vec<String>,
}
impl Capabilities {
    pub fn validate_for(&self, protocol: &str) -> bool {
        let sorted = |values: &[String]| values.windows(2).all(|v| v[0] < v[1]);
        self.schema == CAPABILITIES_SCHEMA
            && sorted(&self.apis)
            && sorted(&self.features)
            && self
                .features
                .iter()
                .all(|v| ROLE_FEATURES.contains(&v.as_str()))
            && self.supports("json")
            && match protocol {
                CAPABILITIES_PROTOCOL => {
                    !self.apis.is_empty()
                        && self.apis.iter().all(|v| CODEC_APIS.contains(&v.as_str()))
                        && self
                            .requires
                            .iter()
                            .map(String::as_str)
                            .eq(["codec_ipc_v3", "responses_output_validation"])
                }
                PROVIDER_PROTOCOL => {
                    self.apis.is_empty()
                        && self
                            .requires
                            .iter()
                            .map(String::as_str)
                            .eq(["provider_ipc_v1", "responses_output_validation"])
                }
                _ => false,
            }
    }
    pub fn supports(&self, feature: &str) -> bool {
        self.features.iter().any(|value| value == feature)
    }
    pub fn supports_api(&self, api: &str) -> bool {
        self.apis.iter().any(|value| value == api)
    }
}

/// Separate startup shape prevents changing legacy ready parsing or serialization.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitiesReply {
    pub protocol: String,
    pub sequence: u64,
    pub value: CapabilitiesResult,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapabilitiesResult {
    Ready { capabilities: Capabilities },
}

pub fn valid_provider_protocol(value: &str) -> bool {
    let Some((name, version)) = value.split_once("/v") else {
        return false;
    };
    (1..=64).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        && (1..=6).contains(&version.len())
        && matches!(version.as_bytes()[0], b'1'..=b'9')
        && version.bytes().all(|b| b.is_ascii_digit())
}
pub fn supported_protocol(value: &str) -> bool {
    matches!(value, PROTOCOL | EDITING_PROTOCOL | CAPABILITIES_PROTOCOL)
}
pub const MAX_FRAME: usize = 128 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub api: String,
    pub model: String,
    pub profile_id: String,
    pub profile_version: String,
    pub reasoning_contract: Option<Value>,
    pub support: BTreeMap<String, String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySpan {
    pub start: usize,
    pub end: usize,
    pub native: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing: Option<Value>,
    pub request: Value,
    pub route: Route,
    pub managed: bool,
    pub pending_tools: bool,
    pub history: Vec<ReplaySpan>,
    pub max_output_bytes: usize,
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
    pub native: Value,
    pub outcome: String,
    pub accounting: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultValue {
    Ready {
        apis: Vec<String>,
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
        accounting: Option<Value>,
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
