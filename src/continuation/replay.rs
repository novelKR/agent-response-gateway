//! Version dispatch is an authentication boundary, not a database migration.
use super::{Error, Origin, Replay, Result, SCHEMA, label};
use crate::ir::continuity::{NativeState, ProviderNativeState, ProviderStateBinding};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const REPLAY_V2: &str = "gateway-continuation/v2";
pub const ENVELOPE_V2: &str = "arg-continuation-v2.";

pub use crate::ir::continuity::{NativeReplay, Outcome};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayV2 {
    pub schema: String,
    pub session: String,
    pub epoch: i64,
    pub origin: Origin,
    pub response: String,
    pub parent: Option<String>,
    pub input_len: usize,
    pub input_sha256: String,
    pub outcome: Outcome,
    pub native: NativeReplay,
    pub output: Vec<Value>,
}
impl ReplayV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema != REPLAY_V2
            || self.epoch <= 0
            || self.input_sha256.len() != 64
            || !self.input_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error("invalid replay"));
        }
        label(&self.session)?;
        label(&self.response)?;
        if let Some(parent) = &self.parent {
            label(parent)?;
        }
        self.origin.validate()?;
        self.native
            .validate()
            .map_err(|_| Error("native replay format"))
    }
}

pub const REPLAY_V3: &str = "gateway-continuation/v3";
pub const ENVELOPE_V3: &str = "arg-continuation-v3.";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBindingWire {
    pub protocol: String,
    pub provider_protocol: String,
    pub id: String,
    pub version: String,
    pub package_sha256: String,
    pub executable_sha256: String,
}
impl ProviderBindingWire {
    fn internal(&self) -> ProviderStateBinding {
        ProviderStateBinding {
            protocol: self.protocol.clone(),
            provider_protocol: self.provider_protocol.clone(),
            id: self.id.clone(),
            version: self.version.clone(),
            package_sha256: self.package_sha256.clone(),
            executable_sha256: self.executable_sha256.clone(),
        }
    }
    fn from_internal(binding: &ProviderStateBinding) -> Self {
        Self {
            protocol: binding.protocol.clone(),
            provider_protocol: binding.provider_protocol.clone(),
            id: binding.id.clone(),
            version: binding.version.clone(),
            package_sha256: binding.package_sha256.clone(),
            executable_sha256: binding.executable_sha256.clone(),
        }
    }
    fn matches_origin(&self, origin: &Origin) -> bool {
        let projection = &origin.route["provider_plugin"];
        origin.route["api"] == "plugin"
            && projection["protocol"] == self.protocol
            && projection["provider_protocol"] == self.provider_protocol
            && projection["id"] == self.id
            && projection["version"] == self.version
            && projection["package_sha256"] == self.package_sha256
            && projection["executable_sha256"] == self.executable_sha256
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderReplayWire {
    pub binding: ProviderBindingWire,
    pub format: String,
    pub version: u32,
    pub data_base64: String,
}
pub(crate) fn decode_provider_state(
    binding: ProviderStateBinding,
    format: String,
    version: u32,
    data: &str,
) -> Result<ProviderNativeState> {
    if data.len() > ProviderNativeState::MAX_BYTES.div_ceil(3) * 4 {
        return Err(Error("provider state limit"));
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| Error("provider state encoding"))?;
    if STANDARD.encode(&bytes) != data {
        return Err(Error("provider state encoding"));
    }
    ProviderNativeState::new(binding, format, version, bytes)
        .map_err(|_| Error("provider state format"))
}
impl ProviderReplayWire {
    fn internal(&self) -> Result<ProviderNativeState> {
        decode_provider_state(
            self.binding.internal(),
            self.format.clone(),
            self.version,
            &self.data_base64,
        )
    }
    fn from_internal(state: &ProviderNativeState) -> Self {
        Self {
            binding: ProviderBindingWire::from_internal(&state.binding),
            format: state.format.clone(),
            version: state.version,
            data_base64: STANDARD.encode(state.bytes()),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayV3 {
    pub schema: String,
    pub session: String,
    pub epoch: i64,
    pub origin: Origin,
    pub response: String,
    pub parent: Option<String>,
    pub input_len: usize,
    pub input_sha256: String,
    pub outcome: Outcome,
    pub native: ProviderReplayWire,
    pub output: Vec<Value>,
}
impl ReplayV3 {
    pub fn validate(&self) -> Result<()> {
        if self.schema != REPLAY_V3
            || self.epoch <= 0
            || self.input_len > u32::MAX as usize
            || self.input_sha256.len() != 64
            || !self
                .input_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !self.native.binding.matches_origin(&self.origin)
        {
            return Err(Error("invalid provider replay"));
        }
        label(&self.session)?;
        label(&self.response)?;
        if let Some(parent) = &self.parent {
            label(parent)?;
        }
        self.origin.validate()?;
        self.native.internal()?;
        Ok(())
    }
}

/// Original version layouts alone are serialized and used for finalized digests.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReplayRecord {
    V1(Replay),
    V2(ReplayV2),
    V3(ReplayV3),
}
impl From<Replay> for ReplayRecord {
    fn from(v: Replay) -> Self {
        Self::V1(v)
    }
}
impl From<ReplayV2> for ReplayRecord {
    fn from(v: ReplayV2) -> Self {
        Self::V2(v)
    }
}
impl From<ReplayV3> for ReplayRecord {
    fn from(v: ReplayV3) -> Self {
        Self::V3(v)
    }
}

/// Not Serialize: internal normalization cannot rewrite a historic digest or envelope.
pub(crate) struct NormalizedReplay {
    pub response: String,
    pub parent: Option<String>,
    pub input_len: usize,
    pub input_sha256: String,
    pub native: NativeState,
    pub output: Vec<Value>,
}
pub(crate) struct ReplayMetadata<'a> {
    pub session: &'a str,
    pub epoch: i64,
    pub origin: &'a Origin,
    pub response: &'a str,
    pub outcome: Outcome,
}
pub(crate) struct ReplayMetadataOwned {
    pub session: String,
    pub epoch: i64,
    pub origin: Origin,
    pub response: String,
    pub parent: Option<String>,
    pub input_len: usize,
    pub input_sha256: String,
    pub outcome: Outcome,
}
impl ReplayRecord {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::V1(v) => v.validate(),
            Self::V2(v) => v.validate(),
            Self::V3(v) => v.validate(),
        }
    }
    pub fn schema(&self) -> &str {
        match self {
            Self::V1(_) => SCHEMA,
            Self::V2(_) => REPLAY_V2,
            Self::V3(_) => REPLAY_V3,
        }
    }
    pub(crate) fn metadata(&self) -> ReplayMetadata<'_> {
        macro_rules! metadata {
            ($v:expr,$outcome:expr) => {
                ReplayMetadata {
                    session: &$v.session,
                    epoch: $v.epoch,
                    origin: &$v.origin,
                    response: &$v.response,
                    outcome: $outcome,
                }
            };
        }
        match self {
            Self::V1(v) => metadata!(
                v,
                if v.provider_status == "requires_action" {
                    Outcome::AwaitingTools
                } else {
                    Outcome::Completed
                }
            ),
            Self::V2(v) => metadata!(v, v.outcome),
            Self::V3(v) => metadata!(v, v.outcome),
        }
    }
    pub(crate) fn into_normalized(self) -> Result<NormalizedReplay> {
        self.validate()?;
        macro_rules! normalized {
            ($v:expr,$native:expr) => {
                NormalizedReplay {
                    response: $v.response,
                    parent: $v.parent,
                    input_len: $v.input_len,
                    input_sha256: $v.input_sha256,
                    native: $native,
                    output: $v.output,
                }
            };
        }
        Ok(match self {
            Self::V1(v) => {
                let native = NativeState::Builtin(NativeReplay::Gemini {
                    version: 1,
                    steps: v.steps,
                });
                normalized!(v, native)
            }
            Self::V2(v) => {
                let native = NativeState::Builtin(v.native);
                normalized!(v, native)
            }
            Self::V3(v) => {
                let native = NativeState::Provider(v.native.internal()?);
                normalized!(v, native)
            }
        })
    }
    pub(crate) fn from_native(
        metadata: ReplayMetadataOwned,
        native: NativeState,
        output: Vec<Value>,
    ) -> Result<Self> {
        macro_rules! record {
            ($ty:ident,$schema:expr,$native:expr) => {
                $ty {
                    schema: $schema.into(),
                    session: metadata.session,
                    epoch: metadata.epoch,
                    origin: metadata.origin,
                    response: metadata.response,
                    parent: metadata.parent,
                    input_len: metadata.input_len,
                    input_sha256: metadata.input_sha256,
                    outcome: metadata.outcome,
                    native: $native,
                    output,
                }
            };
        }
        if matches!(&native, NativeState::Builtin(_)) && metadata.origin.route["api"] == "plugin" {
            return Err(Error("provider replay required"));
        }
        let record = match native {
            NativeState::Builtin(native) => Self::V2(record!(ReplayV2, REPLAY_V2, native)),
            NativeState::Provider(native) => Self::V3(record!(
                ReplayV3,
                REPLAY_V3,
                ProviderReplayWire::from_internal(&native)
            )),
        };
        record.validate()?;
        Ok(record)
    }
}

/// Optional public summaries have their own Responses representation and digest.
/// This helper does not accept native signatures or encrypted provider data.
pub fn public_reasoning(id: &str, text: &str) -> Value {
    json!({"type":"reasoning", "id":id, "summary":[{"type":"summary_text","text":text}]})
}
