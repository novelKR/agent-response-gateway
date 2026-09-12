//! Version dispatch is an authentication boundary, not a database migration.
use super::{Error, Origin, Replay, Result, SCHEMA, label};
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

/// Serialize each version in its original field layout. In particular, a v1
/// finalized digest must be checked before converting to the internal v2 model.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReplayRecord {
    V1(Replay),
    V2(ReplayV2),
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
impl ReplayRecord {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::V1(v) => v.validate(),
            Self::V2(v) => v.validate(),
        }
    }
    pub fn schema(&self) -> &str {
        match self {
            Self::V1(_) => SCHEMA,
            Self::V2(_) => REPLAY_V2,
        }
    }
    pub fn normalize(self) -> ReplayV2 {
        match self {
            Self::V2(v) => v,
            Self::V1(v) => ReplayV2 {
                schema: REPLAY_V2.into(),
                session: v.session,
                epoch: v.epoch,
                origin: v.origin,
                response: v.response,
                parent: v.parent,
                input_len: v.input_len,
                input_sha256: v.input_sha256,
                outcome: if v.provider_status == "requires_action" {
                    Outcome::AwaitingTools
                } else {
                    Outcome::Completed
                },
                native: NativeReplay::Gemini {
                    version: 1,
                    steps: v.steps,
                },
                output: v.output,
            },
        }
    }
}

/// Optional public summaries have their own Responses representation and digest.
/// This helper does not accept native signatures or encrypted provider data.
pub fn public_reasoning(id: &str, text: &str) -> Value {
    json!({"type":"reasoning", "id":id, "summary":[{"type":"summary_text","text":text}]})
}
