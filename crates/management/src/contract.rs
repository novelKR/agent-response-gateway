use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};

pub const CONTRACT: &str = "gateway-management/v1";
pub const STORE_SCHEMA: &str = "gateway-management-store/v1";

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(String);

impl Id {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 96
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        {
            return Err(Error::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Id {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}
impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.0
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest(String);
impl Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(
            ring::digest::digest(&ring::digest::SHA256, bytes)
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for Digest {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidInput);
        }
        Ok(Self(value))
    }
}
impl From<Digest> for String {
    fn from(value: Digest) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    ReadState,
    ReadUsage,
    ReadOperations,
    Reconcile,
    ConfigurationStage,
    ConfigurationSelect,
    RuntimeStart,
    RuntimeStop,
    RuntimeRestart,
    PackageInstall,
    PackageEnable,
    PackageDisable,
    PackageSelect,
    ContinuationTransition,
    TeamSubjectRegister,
    TeamCredentialIssue,
    TeamCredentialRevoke,
    TeamCredentialRotate,
    TeamPermissionsChange,
}
impl Action {
    pub fn is_effect(self) -> bool {
        !matches!(
            self,
            Self::ReadState | Self::ReadUsage | Self::ReadOperations | Self::Reconcile
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub action: Action,
    pub target: Id,
}

/// Constructed only by a trusted authentication adapter, never deserialized from HTTP input.
#[derive(Clone, Debug)]
pub struct Actor {
    identity: Identity,
    grants: BTreeSet<Grant>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub subject: Id,
    pub credential: Id,
}
impl Actor {
    pub fn new(identity: Identity, grants: impl IntoIterator<Item = Grant>) -> Result<Self> {
        let grants: BTreeSet<_> = grants.into_iter().collect();
        if grants.len() > 256 {
            return Err(Error::InvalidInput);
        }
        Ok(Self { identity, grants })
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn authorize(&self, action: Action, target: &Id) -> Result<Grant> {
        let grant = Grant {
            action,
            target: target.clone(),
        };
        if self.grants.contains(&grant) {
            Ok(grant)
        } else {
            Err(Error::Forbidden)
        }
    }
    /// Report the intersection of implemented operations and this actor's grants.
    pub fn capabilities(&self, target: &Id, supported: &[Action]) -> Vec<Action> {
        supported
            .iter()
            .copied()
            .filter(|a| self.authorize(*a, target).is_ok())
            .collect()
    }
}

/// An adapter's canonical target snapshot, including every state relevant to its effect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub revision: u64,
    pub digest: Digest,
}

/// Audit-safe intent. The trusted adapter binds parameters to their exact private bytes.
/// This is not a wire command accepting arbitrary paths, credentials or executable input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub target: Id,
    pub action: Action,
    pub expected: Snapshot,
    pub idempotency_key: Id,
    pub parameters_sha256: Digest,
}
impl Request {
    pub fn fingerprint(&self) -> Result<Digest> {
        serde_json::to_vec(self)
            .map(|v| Digest::of(&v))
            .map_err(|_| Error::InvalidInput)
    }
    pub fn validate(&self) -> Result<()> {
        if self.action.is_effect() {
            Ok(())
        } else {
            Err(Error::InvalidInput)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    Rejected,
    Stale,
    Unavailable,
    Interrupted,
    Unverified,
    EffectFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    Applied {
        after: Snapshot,
        evidence_sha256: Digest,
    },
    NotApplied {
        code: FailureCode,
    },
    Uncertain {
        code: FailureCode,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Running,
    Succeeded,
    Failed,
    Uncertain,
}
impl Effect {
    pub fn state(&self) -> State {
        match self {
            Self::Applied { .. } => State::Succeeded,
            Self::NotApplied { .. } => State::Failed,
            Self::Uncertain { .. } => State::Uncertain,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Accepted,
    Started,
    Finished,
    Recovery,
    Reconciled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub sequence: u64,
    pub at_ms: u64,
    pub phase: Phase,
    pub state: State,
    pub actor: Option<Identity>,
    pub authorization: Option<Grant>,
    pub effect: Option<Effect>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub schema: String,
    pub id: Id,
    pub actor: Identity,
    pub authorization: Grant,
    pub request: Request,
    pub state: State,
    pub events: Vec<Event>,
}

/// Holds the adapter's target lock/lease through inspection, journaling and application.
/// Preparation must have no intended target effects. apply() rechecks any condition
/// that cannot be kept stable by the held lease. Return NotApplied only with proof.
pub trait PreparedOperation {
    fn before(&self) -> &Snapshot;
    /// Observe a committed operation identity without performing target effects.
    /// Useful for private evidence files or a response receipt before completion.
    fn accepted(&mut self, _operation_id: &Id) {}
    fn apply(&mut self) -> Effect;
}

pub trait Backend {
    fn prepare<'a>(&'a mut self, request: &'a Request) -> Result<Box<dyn PreparedOperation + 'a>>;
    /// Read-only reconciliation under the adapter's target synchronization.
    /// Never repeats the original operation. Evidence remains the adapter's responsibility.
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect>;
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum Error {
    #[error("Invalid management input")]
    InvalidInput,
    #[error("Management operation is not permitted")]
    Forbidden,
    #[error("Management request conflicts with recorded or current state")]
    Conflict,
    #[error("Management record was not found")]
    NotFound,
    #[error("Management store is already owned")]
    AlreadyOwned,
    #[error("Invalid management store")]
    InvalidStore,
    #[error("Unsupported management store schema")]
    UnsupportedSchema,
    #[error("Management storage failed")]
    Storage,
    #[error("Invalid management state transition")]
    InvalidTransition,
    #[error("Management outcome is uncertain; inspect operation {0}")]
    Uncertain(Id),
}
pub type Result<T> = std::result::Result<T, Error>;
