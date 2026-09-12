//! Provider-reported accounting only. No transport, database, body, or credential types.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "gateway-usage-event/v1";
pub const PROTOCOL: &str = "gateway-usage-recorder/v1";
pub const MAX_EVENT_BYTES: usize = 65_536;
pub const FIELDS: [&str; 7] = [
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "input_regular_tokens",
    "cache_read_input_tokens",
    "cache_write_input_tokens",
    "reasoning_output_tokens",
];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Reported,
    Derived,
    #[default]
    NotReported,
    NotApplicable,
    Invalid,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Counter {
    pub value: Option<u64>,
    pub source: Source,
}
impl Counter {
    fn read(value: Option<&Value>) -> Self {
        match value {
            None | Some(Value::Null) => Self::default(),
            Some(v) => match v.as_u64() {
                Some(n) => Self {
                    value: Some(n),
                    source: Source::Reported,
                },
                None => Self {
                    value: None,
                    source: Source::Invalid,
                },
            },
        }
    }
    fn derived(value: Option<u64>) -> Self {
        Self {
            value,
            source: if value.is_some() {
                Source::Derived
            } else {
                Source::Invalid
            },
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CacheWriteDetail {
    pub ttl_seconds: u64,
    pub input_tokens: Counter,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonicalUsage {
    pub counters: BTreeMap<String, Counter>,
    pub cache_write_details: Vec<CacheWriteDetail>,
    /// Only documented numeric usage paths; never arbitrary provider JSON.
    pub reported: BTreeMap<String, Counter>,
    pub violations: Vec<String>,
}
impl Default for CanonicalUsage {
    fn default() -> Self {
        Self {
            counters: FIELDS
                .into_iter()
                .map(|f| (f.into(), Counter::default()))
                .collect(),
            cache_write_details: vec![],
            reported: BTreeMap::new(),
            violations: vec![],
        }
    }
}
impl CanonicalUsage {
    pub fn value(&self, field: &str) -> Option<u64> {
        self.counters.get(field).and_then(|v| v.value)
    }
    fn invalid(&mut self, field: &str, reason: &str) {
        self.counters.insert(
            field.into(),
            Counter {
                value: None,
                source: Source::Invalid,
            },
        );
        if !self.violations.iter().any(|v| v == reason) {
            self.violations.push(reason.into());
        }
    }
    pub fn non_read_input_tokens(&self) -> Option<u64> {
        self.value("input_tokens")?
            .checked_sub(self.value("cache_read_input_tokens")?)
    }
    pub fn observed(&self) -> bool {
        self.counters.values().any(|v| v.value.is_some())
    }
    pub fn responses(&self) -> Value {
        let mut v = json!({"input_tokens":self.value("input_tokens"),"output_tokens":self.value("output_tokens"),"total_tokens":self.value("total_tokens")});
        for (field, group, key) in [
            (
                "cache_read_input_tokens",
                "input_tokens_details",
                "cached_tokens",
            ),
            (
                "cache_write_input_tokens",
                "input_tokens_details",
                "cache_write_tokens",
            ),
            (
                "reasoning_output_tokens",
                "output_tokens_details",
                "reasoning_tokens",
            ),
        ] {
            if let Some(n) = self.value(field) {
                v[group][key] = json!(n);
            }
        }
        v
    }
    pub fn validate(&self) -> bool {
        self.counters.len() == FIELDS.len()
            && FIELDS.iter().all(|f| self.counters.contains_key(*f))
            && self
                .counters
                .values()
                .chain(self.reported.values())
                .chain(self.cache_write_details.iter().map(|d| &d.input_tokens))
                .all(|c| {
                    matches!(
                        (c.value, c.source),
                        (Some(_), Source::Reported | Source::Derived)
                            | (
                                None,
                                Source::NotReported | Source::NotApplicable | Source::Invalid
                            )
                    )
                })
            && self.cache_write_details.len() <= 16
            && self.reported.len() <= 16
            && self.violations.len() <= 32
            && self.reported.keys().all(|k| allowed_path(k))
            && self.violations.iter().all(|v| {
                matches!(
                    v.as_str(),
                    "invalid_counter"
                        | "input_partition"
                        | "total_mismatch"
                        | "subset_exceeds_total"
                        | "ttl_mismatch"
                        | "counter_decreased"
                )
            })
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    ResponsesV1,
    ChatV1,
    MessagesV1,
}
impl Profile {
    pub fn id(self) -> &'static str {
        match self {
            Self::ResponsesV1 => "responses/v1",
            Self::ChatV1 => "chat/v1",
            Self::MessagesV1 => "messages/v1",
        }
    }
}
fn paths(profile: Profile) -> Vec<(&'static str, &'static str)> {
    match profile {
        Profile::ResponsesV1 => vec![
            ("input_tokens", "input_tokens"),
            ("output_tokens", "output_tokens"),
            ("total_tokens", "total_tokens"),
            (
                "cache_read_input_tokens",
                "input_tokens_details.cached_tokens",
            ),
            (
                "cache_write_input_tokens",
                "input_tokens_details.cache_write_tokens",
            ),
            (
                "reasoning_output_tokens",
                "output_tokens_details.reasoning_tokens",
            ),
        ],
        Profile::ChatV1 => vec![
            ("input_tokens", "prompt_tokens"),
            ("output_tokens", "completion_tokens"),
            ("total_tokens", "total_tokens"),
            (
                "cache_read_input_tokens",
                "prompt_tokens_details.cached_tokens",
            ),
            (
                "reasoning_output_tokens",
                "completion_tokens_details.reasoning_tokens",
            ),
        ],
        Profile::MessagesV1 => vec![
            ("input_regular_tokens", "input_tokens"),
            ("output_tokens", "output_tokens"),
            ("cache_read_input_tokens", "cache_read_input_tokens"),
            ("cache_write_input_tokens", "cache_creation_input_tokens"),
        ],
    }
}
fn allowed_path(path: &str) -> bool {
    [Profile::ResponsesV1, Profile::ChatV1, Profile::MessagesV1]
        .into_iter()
        .any(|p| paths(p).iter().any(|(_, v)| *v == path))
        || matches!(
            path,
            "cache_creation.ephemeral_5m_input_tokens" | "cache_creation.ephemeral_1h_input_tokens"
        )
}
fn at<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut v = value;
    for key in path.split('.') {
        v = v.get(key)?;
    }
    Some(v)
}
/// Retains only allowlisted counters, so accumulation cannot retain model content.
pub fn extract(profile: Profile, value: &Value) -> BTreeMap<String, Counter> {
    let mut result = BTreeMap::new();
    if !value.is_object() {
        return paths(profile)
            .into_iter()
            .map(|(_, path)| {
                (
                    path.into(),
                    Counter {
                        value: None,
                        source: Source::Invalid,
                    },
                )
            })
            .collect();
    }
    let mut list = paths(profile);
    if profile == Profile::MessagesV1 {
        list.extend([
            ("", "cache_creation.ephemeral_5m_input_tokens"),
            ("", "cache_creation.ephemeral_1h_input_tokens"),
        ]);
    }
    for (_, path) in list {
        if let Some(v) = at(value, path) {
            result.insert(path.into(), Counter::read(Some(v)));
        }
    }
    result
}
pub fn normalize(profile: Profile, reported: BTreeMap<String, Counter>) -> CanonicalUsage {
    let mut u = CanonicalUsage {
        reported,
        ..Default::default()
    };
    for (field, path) in paths(profile) {
        if let Some(c) = u.reported.get(path) {
            u.counters.insert(field.into(), c.clone());
        }
    }
    if u.counters.values().any(|c| c.source == Source::Invalid) {
        u.violations.push("invalid_counter".into());
    }
    if profile == Profile::MessagesV1 {
        if let (Some(a), Some(b), Some(c)) = (
            u.value("input_regular_tokens"),
            u.value("cache_read_input_tokens"),
            u.value("cache_write_input_tokens"),
        ) {
            u.counters.insert(
                "input_tokens".into(),
                Counter::derived(a.checked_add(b).and_then(|n| n.checked_add(c))),
            );
        }
        for (ttl, path) in [
            (300, "cache_creation.ephemeral_5m_input_tokens"),
            (3600, "cache_creation.ephemeral_1h_input_tokens"),
        ] {
            if let Some(c) = u.reported.get(path) {
                u.cache_write_details.push(CacheWriteDetail {
                    ttl_seconds: ttl,
                    input_tokens: c.clone(),
                });
            }
        }
        if u.cache_write_details.len() == 2 {
            let sum = u
                .cache_write_details
                .iter()
                .try_fold(0u64, |sum, d| sum.checked_add(d.input_tokens.value?));
            if let (Some(sum), Some(total)) = (sum, u.value("cache_write_input_tokens"))
                && sum != total
            {
                u.invalid("cache_write_input_tokens", "ttl_mismatch");
            }
        }
    }
    if let (Some(input), Some(read), Some(write)) = (
        u.value("input_tokens"),
        u.value("cache_read_input_tokens"),
        u.value("cache_write_input_tokens"),
    ) {
        if let Some(regular) = input.checked_sub(read).and_then(|n| n.checked_sub(write)) {
            if profile != Profile::MessagesV1 {
                u.counters.insert(
                    "input_regular_tokens".into(),
                    Counter::derived(Some(regular)),
                );
            }
        } else {
            u.invalid("cache_read_input_tokens", "input_partition");
            u.invalid("cache_write_input_tokens", "input_partition");
        }
    }
    for (field, parent) in [
        ("cache_read_input_tokens", "input_tokens"),
        ("cache_write_input_tokens", "input_tokens"),
        ("reasoning_output_tokens", "output_tokens"),
    ] {
        if let (Some(n), Some(max)) = (u.value(field), u.value(parent))
            && n > max
        {
            u.invalid(field, "subset_exceeds_total");
        }
    }
    if let (Some(a), Some(b)) = (u.value("input_tokens"), u.value("output_tokens")) {
        let sum = a.checked_add(b);
        match u.value("total_tokens") {
            Some(total) if sum != Some(total) => u.invalid("total_tokens", "total_mismatch"),
            None if u.counters["total_tokens"].source == Source::NotReported => {
                u.counters
                    .insert("total_tokens".into(), Counter::derived(sum));
            }
            _ => {}
        }
    }
    u
}
#[derive(Clone, Debug)]
pub struct Accumulator {
    profile: Profile,
    reported: BTreeMap<String, Counter>,
    pub usage: CanonicalUsage,
    pub incomplete: bool,
}
impl Accumulator {
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            reported: BTreeMap::new(),
            usage: CanonicalUsage::default(),
            incomplete: false,
        }
    }
    pub fn observe(&mut self, value: &Value) -> bool {
        self.incomplete |= !value.is_object();
        let mut decreased = false;
        for (path, mut c) in extract(self.profile, value) {
            if let (Some(old), Some(new)) =
                (self.reported.get(&path).and_then(|v| v.value), c.value)
                && new < old
            {
                c = Counter {
                    value: None,
                    source: Source::Invalid,
                };
                decreased = true;
            }
            self.reported.insert(path, c);
        }
        let mut next = normalize(self.profile, self.reported.clone());
        if decreased {
            self.incomplete = true;
            next.violations.push("counter_decreased".into());
        }
        let changed = next != self.usage;
        self.usage = next;
        changed
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AttemptStarted,
    UsageUpdated,
    AttemptFinished,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Finality {
    #[default]
    Unobserved,
    Partial,
    Final,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    #[default]
    Unknown,
    InProgress,
    Completed,
    Incomplete,
    Failed,
    Cancelled,
    TransportLost,
    ConversionFailed,
    ObservationIncomplete,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsageEvent {
    pub schema: String,
    pub producer_id: String,
    pub request_id: String,
    pub attempt_id: String,
    pub event_id: String,
    pub revision: u64,
    pub kind: EventKind,
    pub started_at_ms: u64,
    pub observed_at_ms: u64,
    pub provider: String,
    pub model_alias: String,
    pub upstream_model: String,
    pub reported_model: Option<String>,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub profile: Profile,
    pub configuration_sha256: String,
    pub upstream: Outcome,
    pub gateway: Outcome,
    pub finality: Finality,
    pub observation_incomplete: bool,
    pub usage: CanonicalUsage,
}
pub fn safe_label(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 200
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_/.:".contains(&b))
}
pub fn digest(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl UsageEvent {
    pub fn validate(&self) -> bool {
        let normalized = normalize(self.profile, self.usage.reported.clone());
        let consistent = self.usage.counters == normalized.counters
            && self.usage.cache_write_details == normalized.cache_write_details
            && self
                .usage
                .violations
                .iter()
                .filter(|v| v.as_str() != "counter_decreased")
                .eq(normalized.violations.iter())
            && self
                .usage
                .reported
                .values()
                .all(|v| v.source != Source::Derived && v.source != Source::NotApplicable);
        self.schema == SCHEMA
            && consistent
            && [
                &self.producer_id,
                &self.request_id,
                &self.attempt_id,
                &self.event_id,
                &self.provider,
                &self.model_alias,
                &self.upstream_model,
            ]
            .into_iter()
            .all(|v| safe_label(v))
            && [
                &self.reported_model,
                &self.provider_request_id,
                &self.provider_response_id,
            ]
            .into_iter()
            .all(|v| v.as_ref().is_none_or(|v| safe_label(v)))
            && self.configuration_sha256.len() == 64
            && self
                .configuration_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && self.observed_at_ms >= self.started_at_ms
            && self.usage.validate()
            && (self.kind != EventKind::AttemptStarted || self.revision == 0)
            && (self.kind == EventKind::AttemptStarted || self.revision > 0)
    }
    pub fn bytes(&self) -> Result<Vec<u8>, &'static str> {
        if !self.validate() {
            return Err("invalid_usage_event");
        }
        // Value sorts map keys for stable IPC/storage identity across both executables.
        let bytes =
            serde_json::to_vec(&serde_json::to_value(self).map_err(|_| "invalid_usage_event")?)
                .map_err(|_| "invalid_usage_event")?;
        if bytes.len() > MAX_EVENT_BYTES {
            return Err("usage_event_too_large");
        };
        Ok(bytes)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    BestEffort,
    DurableLocal,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecorderBinding {
    pub store_id: String,
    pub mode: Mode,
    pub queue_capacity: usize,
    pub ack_timeout_ms: u64,
    pub config_sha256: String,
}
impl RecorderBinding {
    pub fn validate(&self) -> bool {
        self.store_id.len() <= 64
            && self
                .store_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !self.store_id.is_empty()
            && (2..=4096).contains(&self.queue_capacity)
            && (1..=60_000).contains(&self.ack_timeout_ms)
            && self.config_sha256.len() == 64
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecorderConfig {
    pub schema: String,
    pub destinations: Vec<Destination>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Destination {
    Http {
        id: String,
        url: String,
        bearer_file: String,
    },
    Postgres {
        id: String,
        connection_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tls_ca_file: Option<String>,
    },
}
impl Destination {
    pub fn id(&self) -> &str {
        match self {
            Self::Http { id, .. } | Self::Postgres { id, .. } => id,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema: String,
    pub events: Vec<UsageEvent>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub producer_id: String,
    pub event_id: String,
    pub sha256: String,
    pub status: ReceiptStatus,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Committed,
    Duplicate,
    Conflict,
    Rejected,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchReceipt {
    pub schema: String,
    pub receipts: Vec<Receipt>,
}
