use gateway_management::{
    Action, Actor, Digest, Effect, Id, Operation, PreparedOperation, Request, Result, Snapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: &str = "gateway-management-http/v1";
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageFamily {
    Native,
    ProfilePack,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSelection {
    pub id: Id,
    pub version: String,
    pub package_sha256: Digest,
}
/// Closed wire commands carry registered IDs and review metadata, never host paths or programs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    RuntimeStart {},
    RuntimeStop {},
    RuntimeRestart {},
    ConfigurationStage {
        source: Id,
        candidate: Id,
        source_sha256: Digest,
    },
    ConfigurationSelect {
        candidate: Id,
    },
    PackageInstall {
        family: PackageFamily,
        source: Id,
    },
    PackageEnable {
        family: PackageFamily,
        package: PackageSelection,
        grants: Vec<String>,
        recorder: Option<Id>,
    },
    PackageDisable {
        family: PackageFamily,
        package: Id,
    },
    PackageSelect {
        family: PackageFamily,
        package: PackageSelection,
        grants: Vec<String>,
        recorder: Option<Id>,
    },
    ContinuationTransition {
        session: Id,
        revision: u64,
        transition_kind: String,
        portable_sha256: Option<Digest>,
        decision_reference: Id,
        pending_tools: bool,
        pending_approvals: bool,
    },
}
impl Command {
    pub fn action(&self) -> Action {
        match self {
            Self::RuntimeStart { .. } => Action::RuntimeStart,
            Self::RuntimeStop { .. } => Action::RuntimeStop,
            Self::RuntimeRestart { .. } => Action::RuntimeRestart,
            Self::ConfigurationStage { .. } => Action::ConfigurationStage,
            Self::ConfigurationSelect { .. } => Action::ConfigurationSelect,
            Self::PackageInstall { .. } => Action::PackageInstall,
            Self::PackageEnable { .. } => Action::PackageEnable,
            Self::PackageDisable { .. } => Action::PackageDisable,
            Self::PackageSelect { .. } => Action::PackageSelect,
            Self::ContinuationTransition { .. } => Action::ContinuationTransition,
        }
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::PackageEnable {
                package, grants, ..
            }
            | Self::PackageSelect {
                package, grants, ..
            } => {
                let version = &package.version;
                if version.len() > 32
                    || version.split('.').count() != 3
                    || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                    || grants.len() > 8
                    || grants.iter().any(|g| {
                        g.is_empty()
                            || g.len() > 64
                            || !g.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    })
                    || grants.windows(2).any(|g| g[0] >= g[1])
                {
                    return Err(gateway_management::Error::InvalidInput);
                }
            }
            Self::ContinuationTransition {
                transition_kind: kind,
                pending_tools,
                pending_approvals,
                ..
            } if *pending_tools
                || *pending_approvals
                || kind.is_empty()
                || kind.len() > 64
                || !kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') =>
            {
                return Err(gateway_management::Error::InvalidInput);
            }
            _ => {}
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<Digest> {
        self.validate()?;
        serde_json::to_vec(self)
            .map(|v| Digest::of(&v))
            .map_err(|_| gateway_management::Error::InvalidInput)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub schema: String,
    pub target: Id,
    pub expected: Snapshot,
    pub idempotency_key: Id,
    pub command: Command,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preflight {
    pub schema: String,
    pub target: Id,
    pub idempotency_key: Id,
    pub command: Command,
}
impl Submission {
    pub fn request(&self) -> Result<Request> {
        if self.schema != SCHEMA {
            return Err(gateway_management::Error::InvalidInput);
        }
        Ok(Request {
            target: self.target.clone(),
            action: self.command.action(),
            expected: self.expected.clone(),
            idempotency_key: self.idempotency_key.clone(),
            parameters_sha256: self.command.digest()?,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageRange {
    pub from_ms: u64,
    pub to_ms: u64,
    pub timezone: String,
}
impl UsageRange {
    pub fn validate(&self) -> Result<()> {
        if self.from_ms >= self.to_ms
            || self.to_ms > i64::MAX as u64
            || self.to_ms - self.from_ms > 366 * 24 * 60 * 60 * 1000
            || self.timezone.is_empty()
            || self.timezone.len() > 64
            || !self
                .timezone
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
        {
            return Err(gateway_management::Error::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub enum Query {
    State,
    Usage(UsageRange),
    Continuation(Id),
}
impl Query {
    pub fn action(&self) -> Action {
        match self {
            Self::State | Self::Continuation(_) => Action::ReadState,
            Self::Usage(_) => Action::ReadUsage,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Feature {
    pub id: Id,
    pub version: String,
    pub installed: bool,
    pub enabled: bool,
    pub operations: Vec<Action>,
}
/// Trusted host dispatch. Concrete commands must reuse their manager's canonical validation
/// and lock through PreparedOperation. Read results contain only authorized metadata.
pub trait Dispatcher: Send {
    fn features(&self) -> Vec<Feature>;
    fn supported(&self) -> Vec<Action>;
    fn snapshot(&mut self, command: &Command) -> Result<Snapshot>;
    fn read(&mut self, actor: &Actor, query: &Query) -> Result<Value>;
    fn prepare<'a>(
        &'a mut self,
        request: &'a Request,
        command: &'a Command,
    ) -> Result<Box<dyn PreparedOperation + 'a>>;
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect>;
}

pub const STATE_SCHEMA: &str = "gateway-management-state/v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observation {
    Observed { observed_at_ms: u64, data: Value },
    Unobserved { reason: Id },
    Unsupported {},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleView {
    pub id: Id,
    pub contract: String,
    pub observation: Observation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateView {
    pub schema: String,
    pub modules: Vec<ModuleView>,
}
impl StateView {
    pub fn validate(&self) -> Result<()> {
        let mut ids = std::collections::BTreeSet::new();
        if self.schema != STATE_SCHEMA || self.modules.len() > 16 {
            return Err(gateway_management::Error::InvalidInput);
        }
        for module in &self.modules {
            if !ids.insert(&module.id)
                || module.contract.is_empty()
                || module.contract.len() > 96
                || !module
                    .contract
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_/.:".contains(&b))
            {
                return Err(gateway_management::Error::InvalidInput);
            }
            if let Observation::Observed { data, .. } = &module.observation
                && data["schema"] != module.contract
            {
                return Err(gateway_management::Error::InvalidInput);
            }
        }
        Ok(())
    }
}
