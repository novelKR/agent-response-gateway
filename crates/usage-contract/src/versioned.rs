use super::*;
pub const SCHEMA_V2: &str = "gateway-usage-event/v2";
pub const PROTOCOL_V2: &str = "gateway-usage-recorder/v2";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Interpretation {
    pub kind: String,
    pub protocol: String,
    pub provider_protocol: String,
    pub package_id: String,
    pub package_version: String,
    pub package_sha256: String,
    pub executable_sha256: String,
}
fn hex(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Interpretation {
    pub fn validate(&self) -> bool {
        let Some((name, v)) = self.provider_protocol.split_once("/v") else {
            return false;
        };
        self.kind == "trusted_provider_plugin"
            && self.protocol == "gateway-provider/v1"
            && !name.is_empty()
            && name.len() <= 64
            && name.as_bytes()[0].is_ascii_lowercase()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            && (1..=6).contains(&v.len())
            && matches!(v.as_bytes()[0], b'1'..=b'9')
            && v.bytes().all(|b| b.is_ascii_digit())
            && !self.package_id.is_empty()
            && self.package_id.len() <= 64
            && self.package_id.as_bytes()[0].is_ascii_lowercase()
            && self
                .package_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && self.package_version.split('.').count() == 3
            && self.package_version.split('.').all(|p| {
                !p.is_empty()
                    && p.len() <= 6
                    && (p.len() == 1 || !p.starts_with('0'))
                    && p.bytes().all(|b| b.is_ascii_digit())
            })
            && hex(&self.package_sha256)
            && hex(&self.executable_sha256)
    }
}
pub fn recorder_v2_capabilities() -> Value {
    json!({"schema":"gateway-plugin-capabilities/v1","apis":[],"features":["usage_event_v1","usage_event_v2"],"requires":["usage_recorder_ipc_v2"]})
}
pub fn provider_usage_valid(u: &CanonicalUsage) -> bool {
    if !u.validate() || !u.reported.is_empty() || !u.cache_write_details.is_empty() {
        return false;
    }
    if let (Some(i), Some(o), Some(t)) = (
        u.value("input_tokens"),
        u.value("output_tokens"),
        u.value("total_tokens"),
    ) && i.checked_add(o) != Some(t)
    {
        return false;
    }
    if let Some(i) = u.value("input_tokens") {
        let mut sum = 0u64;
        for f in [
            "input_regular_tokens",
            "cache_read_input_tokens",
            "cache_write_input_tokens",
        ] {
            if let Some(n) = u.value(f) {
                let Some(s) = sum.checked_add(n) else {
                    return false;
                };
                sum = s;
            }
        }
        if sum > i {
            return false;
        }
        if [
            "input_regular_tokens",
            "cache_read_input_tokens",
            "cache_write_input_tokens",
        ]
        .iter()
        .all(|f| u.value(f).is_some())
            && sum != i
        {
            return false;
        }
    }
    if let (Some(r), Some(o)) = (u.value("reasoning_output_tokens"), u.value("output_tokens"))
        && r > o
    {
        return false;
    }
    true
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsageEventV2 {
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
    pub interpretation: Interpretation,
    pub configuration_sha256: String,
    pub upstream: Outcome,
    pub gateway: Outcome,
    pub finality: Finality,
    pub observation_incomplete: bool,
    pub usage: CanonicalUsage,
}
impl UsageEventV2 {
    pub fn validate(&self) -> bool {
        self.schema == SCHEMA_V2
            && self.interpretation.validate()
            && provider_usage_valid(&self.usage)
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
        let b = serde_json::to_vec(&serde_json::to_value(self).map_err(|_| "invalid_usage_event")?)
            .map_err(|_| "invalid_usage_event")?;
        if b.len() > MAX_EVENT_BYTES {
            return Err("usage_event_too_large");
        }
        Ok(b)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedEvent {
    V1(UsageEvent),
    V2(UsageEventV2),
}
impl From<UsageEvent> for RecordedEvent {
    fn from(v: UsageEvent) -> Self {
        Self::V1(v)
    }
}
impl From<UsageEventV2> for RecordedEvent {
    fn from(v: UsageEventV2) -> Self {
        Self::V2(v)
    }
}
impl Serialize for RecordedEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::V1(v) => v.serialize(s),
            Self::V2(v) => v.serialize(s),
        }
    }
}
impl<'de> Deserialize<'de> for RecordedEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match v.get("schema").and_then(Value::as_str) {
            Some(SCHEMA) => serde_json::from_value(v).map(Self::V1),
            Some(SCHEMA_V2) => serde_json::from_value(v).map(Self::V2),
            _ => return Err(serde::de::Error::custom("unsupported_usage_event")),
        }
        .map_err(serde::de::Error::custom)
    }
}
pub struct EventView<'a> {
    pub producer_id: &'a String,
    pub request_id: &'a String,
    pub attempt_id: &'a String,
    pub event_id: &'a String,
    pub revision: u64,
    pub kind: EventKind,
    pub started_at_ms: u64,
    pub observed_at_ms: u64,
    pub provider: &'a String,
    pub model_alias: &'a String,
    pub upstream_model: &'a String,
    pub reported_model: &'a Option<String>,
    pub provider_request_id: &'a Option<String>,
    pub provider_response_id: &'a Option<String>,
    pub configuration_sha256: &'a String,
    pub upstream: Outcome,
    pub gateway: Outcome,
    pub finality: Finality,
    pub observation_incomplete: bool,
    pub usage: &'a CanonicalUsage,
}
impl RecordedEvent {
    /// Read the persisted event without repairing its byte identity or interpretation.
    pub fn from_stored_bytes(bytes: &[u8], sha256: &str) -> Result<Self, &'static str> {
        if bytes.len() > MAX_EVENT_BYTES || digest(bytes) != sha256 {
            return Err("stored_usage_integrity");
        }
        let event: Self = serde_json::from_slice(bytes).map_err(|_| "stored_usage_integrity")?;
        if event
            .bytes()
            .map_err(|_| "stored_usage_integrity")?
            .as_slice()
            != bytes
        {
            return Err("stored_usage_integrity");
        }
        Ok(event)
    }
    pub fn validate(&self) -> bool {
        match self {
            Self::V1(e) => e.validate(),
            Self::V2(e) => e.validate(),
        }
    }
    pub fn bytes(&self) -> Result<Vec<u8>, &'static str> {
        match self {
            Self::V1(e) => e.bytes(),
            Self::V2(e) => e.bytes(),
        }
    }
    pub fn view(&self) -> EventView<'_> {
        match self {
            Self::V1(e) => EventView {
                producer_id: &e.producer_id,
                request_id: &e.request_id,
                attempt_id: &e.attempt_id,
                event_id: &e.event_id,
                revision: e.revision,
                kind: e.kind,
                started_at_ms: e.started_at_ms,
                observed_at_ms: e.observed_at_ms,
                provider: &e.provider,
                model_alias: &e.model_alias,
                upstream_model: &e.upstream_model,
                reported_model: &e.reported_model,
                provider_request_id: &e.provider_request_id,
                provider_response_id: &e.provider_response_id,
                configuration_sha256: &e.configuration_sha256,
                upstream: e.upstream,
                gateway: e.gateway,
                finality: e.finality,
                observation_incomplete: e.observation_incomplete,
                usage: &e.usage,
            },
            Self::V2(e) => EventView {
                producer_id: &e.producer_id,
                request_id: &e.request_id,
                attempt_id: &e.attempt_id,
                event_id: &e.event_id,
                revision: e.revision,
                kind: e.kind,
                started_at_ms: e.started_at_ms,
                observed_at_ms: e.observed_at_ms,
                provider: &e.provider,
                model_alias: &e.model_alias,
                upstream_model: &e.upstream_model,
                reported_model: &e.reported_model,
                provider_request_id: &e.provider_request_id,
                provider_response_id: &e.provider_response_id,
                configuration_sha256: &e.configuration_sha256,
                upstream: e.upstream,
                gateway: e.gateway,
                finality: e.finality,
                observation_incomplete: e.observation_incomplete,
                usage: &e.usage,
            },
        }
    }
    pub fn usage(&self) -> &CanonicalUsage {
        match self {
            Self::V1(e) => &e.usage,
            Self::V2(e) => &e.usage,
        }
    }
    pub fn is_v2(&self) -> bool {
        matches!(self, Self::V2(_))
    }
    pub fn as_v1(&self) -> Option<&UsageEvent> {
        match self {
            Self::V1(e) => Some(e),
            _ => None,
        }
    }
    pub fn interpretation(&self) -> Value {
        match self {
            Self::V1(e) => json!({"kind":"builtin_parser","profile":e.profile}),
            Self::V2(e) => json!(e.interpretation),
        }
    }
    pub fn identity_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        match self {
            Self::V1(e) => serde_json::to_vec(&(
                &e.request_id,
                e.started_at_ms,
                &e.provider,
                &e.model_alias,
                &e.upstream_model,
                e.profile,
                &e.configuration_sha256,
            )),
            Self::V2(e) => serde_json::to_vec(&(
                SCHEMA_V2,
                &e.request_id,
                e.started_at_ms,
                &e.provider,
                &e.model_alias,
                &e.upstream_model,
                &e.interpretation,
                &e.configuration_sha256,
            )),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchV2 {
    pub schema: String,
    pub events: Vec<RecordedEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_only_provider_usage_preserves_unknown_and_invalid_sources() {
        let mut u = CanonicalUsage::default();
        assert!(provider_usage_valid(&u));
        u.counters.insert(
            "input_tokens".into(),
            Counter {
                value: Some(10),
                source: Source::Reported,
            },
        );
        u.counters.insert(
            "input_regular_tokens".into(),
            Counter {
                value: Some(8),
                source: Source::Reported,
            },
        );
        u.counters.insert(
            "cache_read_input_tokens".into(),
            Counter {
                value: Some(8),
                source: Source::Reported,
            },
        );
        assert!(!provider_usage_valid(&u));
        u.counters.insert(
            "input_regular_tokens".into(),
            Counter {
                value: None,
                source: Source::Invalid,
            },
        );
        u.violations.push("input_partition".into());
        assert!(provider_usage_valid(&u));
        u.reported.insert("input_tokens".into(), Counter::default());
        assert!(!provider_usage_valid(&u));
    }
    #[test]
    fn recorded_event_rejects_unknown_schema_and_does_not_wrap_wire() {
        assert!(
            serde_json::from_value::<RecordedEvent>(json!({"schema":"gateway-usage-event/v3"}))
                .is_err()
        );
    }
}
