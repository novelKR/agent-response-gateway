//! Portable codec wire types. Nested role values follow the published schemas.
//! This crate has no gateway, transport, storage, or credential dependencies.
//! Deserialization alone is not semantic validation; hosts validate role values.
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
