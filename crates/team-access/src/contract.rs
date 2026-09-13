use gateway_management::{Action, Digest, Error, Grant, Id, Identity, Operation, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

pub const SCHEMA: &str = "gateway-team-access/v1";
pub const STORE_SCHEMA: &str = "gateway-team-store/v1";
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Model,
    Management,
    ReadOnly,
}
impl Purpose {
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            Self::Model => "gwt1_model_",
            Self::Management => "gwt1_manage_",
            Self::ReadOnly => "gwt1_read_",
        }
    }
}
/// Explicit host permissions. Model routes are exact aliases, not patterns or role names.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    pub enabled: bool,
    pub routes: BTreeSet<String>,
    pub management: BTreeSet<Grant>,
    pub read_all_usage: bool,
}
impl Permissions {
    pub fn validate(&self) -> Result<()> {
        if self.routes.len() > 128
            || self.management.len() > 256
            || self.routes.iter().any(|route| {
                route.is_empty()
                    || route.len() > 256
                    || route.trim() != route
                    || route.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidInput);
        }
        if serde_json::to_vec(self)
            .map_err(|_| Error::InvalidInput)?
            .len()
            > 48 * 1024
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
    pub fn permits_route(&self, route: &str) -> bool {
        self.enabled && self.routes.contains(route)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Register {
        subject: Id,
        permissions: Permissions,
    },
    PermissionsChange {
        subject: Id,
        permissions: Permissions,
    },
    Issue {
        subject: Id,
        credential: Id,
        purpose: Purpose,
    },
    Revoke {
        credential: Id,
    },
    Rotate {
        credential: Id,
        replacement: Id,
    },
}
impl Command {
    pub fn action(&self) -> Action {
        match self {
            Self::Register { .. } => Action::TeamSubjectRegister,
            Self::PermissionsChange { .. } => Action::TeamPermissionsChange,
            Self::Issue { .. } => Action::TeamCredentialIssue,
            Self::Revoke { .. } => Action::TeamCredentialRevoke,
            Self::Rotate { .. } => Action::TeamCredentialRotate,
        }
    }
    pub fn digest(&self) -> Result<Digest> {
        if let Self::Register { permissions, .. } | Self::PermissionsChange { permissions, .. } =
            self
        {
            permissions.validate()?;
        }
        if matches!(self,Self::Rotate{credential,replacement} if credential==replacement) {
            return Err(Error::InvalidInput);
        }
        serde_json::to_vec(self)
            .map(|v| Digest::of(&v))
            .map_err(|_| Error::InvalidInput)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub id: Id,
    pub revision: u64,
    pub permissions: Permissions,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub id: Id,
    pub subject: Id,
    pub purpose: Purpose,
    pub revoked: bool,
    /// Proves insertion, not delivery to the holder. Query never exposes a verifier or raw key.
    pub issued_operation: Id,
}
#[derive(Clone, Debug, Serialize)]
pub struct Inventory {
    pub schema: &'static str,
    pub target: Id,
    pub snapshot: gateway_management::Snapshot,
    pub subjects: Vec<Subject>,
    pub credentials: Vec<Credential>,
}
#[derive(Clone, Debug)]
pub struct Principal {
    pub identity: Identity,
    pub purpose: Purpose,
    pub permissions: Permissions,
    pub authorization_version: Digest,
}
/// One-time delivery only after a successful durable operation result. No Debug/Serialize.
///
/// ```compile_fail
/// fn log_secret(secret: &gateway_team_access::Secret) { println!("{secret:?}"); }
/// ```
/// ```compile_fail
/// fn record_secret(secret: &gateway_team_access::Secret) { serde_json::to_string(secret).unwrap(); }
/// ```
pub struct Secret {
    pub(crate) value: Zeroizing<String>,
    pub(crate) credential: Id,
}
impl Secret {
    pub fn credential_id(&self) -> &Id {
        &self.credential
    }
    /// Use only an explicitly protected delivery path; never log or place in operation metadata.
    pub fn expose(&self) -> &str {
        &self.value
    }
}
/// Retried operations contain no secret. A missing secret never requests automatic reissuance.
pub struct Completion {
    pub operation: Operation,
    pub secret: Option<Secret>,
}
