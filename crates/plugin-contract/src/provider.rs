//! Provider semantics are delegated explicitly; transport and stored-state authority are not.
use crate::Capabilities;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    pub sequence: u64,
    pub operation: Operation,
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
pub struct Prepare {
    pub request: Value,
    pub route: Route,
    pub continuation: Continuation,
    pub max_request_bytes: u32,
    pub max_output_bytes: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub provider_protocol: String,
    pub model: String,
    pub profile_id: String,
    pub profile_version: String,
    pub support: BTreeMap<String, String>,
    pub editing: EditingSelection,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditingSelection {
    None,
    Enabled { policy: Value },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Stateless,
    Managed {
        pending_tools: bool,
        history: Vec<ReplaySpan>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaySpan {
    pub start: u32,
    pub end: u32,
    pub state: OpaqueState,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueState {
    pub format: String,
    pub version: u32,
    pub data_base64: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub protocol: String,
    pub sequence: u64,
    pub value: ResultValue,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultValue {
    Ready {
        provider_protocol: String,
        capabilities: Capabilities,
    },
    Prepared {
        payload: Value,
    },
    Progress {
        events: Vec<Value>,
        complete: bool,
        usage: UsageSnapshot,
    },
    Completed {
        value: Box<Completed>,
    },
    Rejected {
        code: RejectionCode,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completed {
    pub response: Value,
    pub outcome: Outcome,
    pub usage: UsageSnapshot,
    pub state: StateResult,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    AwaitingTools,
    Incomplete,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StateResult {
    None,
    Opaque { value: OpaqueState },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    UnsupportedRequest,
    InvalidUpstream,
    UnsupportedState,
    ResourceLimit,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UsageSnapshot {
    Unobserved,
    Observed { counters: Counters },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    pub input_tokens: Counter,
    pub output_tokens: Counter,
    pub total_tokens: Counter,
    pub input_regular_tokens: Counter,
    pub cache_read_input_tokens: Counter,
    pub cache_write_input_tokens: Counter,
    pub reasoning_output_tokens: Counter,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum Counter {
    Reported { value: u64 },
    NotReported,
    NotApplicable,
    Invalid,
}
