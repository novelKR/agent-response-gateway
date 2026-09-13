use gateway_management::{Digest, Error, Id, Identity, Result};
use gateway_team_access::{Principal, Purpose};
use gateway_team_http::ModelAuthority;
use std::{collections::BTreeSet, sync::Arc};
pub const MODEL_IDENTITY_SCHEMA: &str = "gateway-host-model-identity/v1";
/// Host-produced evidence, never an HTTP body DTO. The host verifies its own identity system.
pub struct VerifiedModelIdentity {
    pub schema: String,
    pub principal: Principal,
}
pub trait ModelIdentityVerifier: Send + Sync {
    fn authenticate(&self, credential: &str) -> Option<VerifiedModelIdentity>;
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<VerifiedModelIdentity>;
}
/// Optional bridge with no local Team credential store, listener or authentication session.
pub struct HostModelAuthority {
    target: Id,
    routes: BTreeSet<String>,
    all_usage: bool,
    verifier: Arc<dyn ModelIdentityVerifier>,
}
impl HostModelAuthority {
    pub fn new(
        target: Id,
        routes: BTreeSet<String>,
        all_usage: bool,
        verifier: Arc<dyn ModelIdentityVerifier>,
    ) -> Result<Self> {
        if routes.is_empty()
            || routes.len() > 128
            || routes.iter().any(|r| {
                r.is_empty() || r.len() > 256 || r.trim() != r || r.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            target,
            routes,
            all_usage,
            verifier,
        })
    }
    fn checked(&self, evidence: VerifiedModelIdentity) -> Option<Principal> {
        if evidence.schema != MODEL_IDENTITY_SCHEMA
            || evidence.principal.purpose != Purpose::Model
            || !evidence.principal.permissions.enabled
            || evidence.principal.permissions.validate().is_err()
        {
            return None;
        }
        let mut principal = evidence.principal;
        principal
            .permissions
            .routes
            .retain(|r| self.routes.contains(r));
        principal.permissions.management.clear();
        principal.permissions.read_all_usage &= self.all_usage;
        Some(principal)
    }
}
impl ModelAuthority for HostModelAuthority {
    fn target(&self) -> &Id {
        &self.target
    }
    fn authenticate_model(&self, credential: &str) -> Option<Principal> {
        self.checked(self.verifier.authenticate(credential)?)
    }
    fn refresh_model(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        let principal = self.checked(self.verifier.refresh(identity, version)?)?;
        (principal.identity == *identity && principal.authorization_version == *version)
            .then_some(principal)
    }
}
