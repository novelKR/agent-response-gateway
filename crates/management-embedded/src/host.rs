use gateway_management::{
    Action, Actor, Digest, Effect, Error, Id, Identity, Journal, Operation, PreparedOperation,
    Reader, Request, Result, Snapshot,
};
use gateway_management_api::{
    Authenticator, Command, CredentialKind, Dispatcher, Feature, Principal, Query, Service,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};
pub const HOST_SCHEMA: &str = "gateway-embedded-management/v1";
pub const IDENTITY_SCHEMA: &str = "gateway-host-identity/v1";
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    HostOwned,
    Delegated,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostContract {
    pub schema: String,
    pub target: Id,
    pub lifecycle: Lifecycle,
    pub operations: BTreeSet<Action>,
}
fn controls(action: Action) -> bool {
    matches!(
        action,
        Action::RuntimeStart | Action::RuntimeStop | Action::RuntimeRestart
    )
}
impl HostContract {
    pub fn validate(&self) -> Result<()> {
        if self.schema != HOST_SCHEMA {
            return Err(Error::UnsupportedSchema);
        }
        if self.lifecycle == Lifecycle::HostOwned && self.operations.iter().any(|a| controls(*a)) {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
    fn require(&self, action: Action) -> Result<()> {
        if self.operations.contains(&action) {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }
    fn principal(&self, principal: Principal) -> Option<Principal> {
        let supported: Vec<_> = self
            .operations
            .iter()
            .copied()
            .filter(|a| {
                principal.kind == CredentialKind::Management
                    || matches!(
                        a,
                        Action::ReadState | Action::ReadUsage | Action::ReadOperations
                    )
            })
            .collect();
        let grants = principal
            .actor
            .capabilities(&self.target, &supported)
            .into_iter()
            .map(|action| gateway_management::Grant {
                action,
                target: self.target.clone(),
            });
        Some(Principal {
            actor: Actor::new(principal.actor.identity().clone(), grants).ok()?,
            ..principal
        })
    }
}
/// Evidence produced after the host's authentication boundary. No Deserialize implementation:
/// an API body or identity header cannot become trusted evidence by deserializing this type.
pub struct VerifiedIdentity {
    pub schema: String,
    pub principal: Principal,
}
pub trait IdentityVerifier: Send + Sync {
    fn authenticate(&self, credential: &str) -> Option<VerifiedIdentity>;
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<VerifiedIdentity>;
}
pub struct HostAuthenticator {
    contract: HostContract,
    verifier: Arc<dyn IdentityVerifier>,
}
impl HostAuthenticator {
    pub fn new(contract: HostContract, verifier: Arc<dyn IdentityVerifier>) -> Result<Self> {
        contract.validate()?;
        Ok(Self { contract, verifier })
    }
    fn checked(&self, evidence: VerifiedIdentity) -> Option<Principal> {
        if evidence.schema != IDENTITY_SCHEMA {
            return None;
        }
        self.contract.principal(evidence.principal)
    }
}
impl Authenticator for HostAuthenticator {
    fn authenticate(&self, credential: &str) -> Option<Principal> {
        self.checked(self.verifier.authenticate(credential)?)
    }
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        let principal = self.checked(self.verifier.refresh(identity, version)?)?;
        (principal.actor.identity() == identity && principal.authorization_version == *version)
            .then_some(principal)
    }
}
/// Host limits apply to capability reports and every direct/HTTP invocation.
pub struct HostDispatcher {
    contract: HostContract,
    backend: Box<dyn Dispatcher>,
}
impl HostDispatcher {
    pub fn new(contract: HostContract, backend: Box<dyn Dispatcher>) -> Result<Self> {
        contract.validate()?;
        let supported = backend.supported();
        if contract.operations.iter().any(|a| !supported.contains(a)) {
            return Err(Error::Unsupported);
        }
        Ok(Self { contract, backend })
    }
    pub fn target(&self) -> &Id {
        &self.contract.target
    }
    /// Explicit factory binds the router to the same target. Hosts supply existing stores;
    /// this neither initializes persistence nor starts a listener or owned execution.
    pub fn service(
        self,
        bound: SocketAddr,
        verifier: Arc<dyn IdentityVerifier>,
        journal: Journal,
        reader: Reader,
        read_sessions: bool,
    ) -> Result<Arc<Service>> {
        let auth = HostAuthenticator::new(self.contract.clone(), verifier)?;
        Service::new(
            self.contract.target.clone(),
            bound,
            Arc::new(auth),
            journal,
            reader,
            Box::new(self),
            read_sessions,
        )
        .map_err(|_| Error::InvalidInput)
    }
}
impl Dispatcher for HostDispatcher {
    fn features(&self) -> Vec<Feature> {
        self.backend
            .features()
            .into_iter()
            .map(|mut feature| {
                feature
                    .operations
                    .retain(|a| self.contract.operations.contains(a));
                feature
            })
            .collect()
    }
    fn supported(&self) -> Vec<Action> {
        self.contract.operations.iter().copied().collect()
    }
    fn snapshot(&mut self, command: &Command) -> Result<Snapshot> {
        self.contract.require(command.action())?;
        self.backend.snapshot(command)
    }
    fn read(&mut self, actor: &Actor, query: &Query) -> Result<Value> {
        self.contract.require(query.action())?;
        actor.authorize(query.action(), &self.contract.target)?;
        self.backend.read(actor, query)
    }
    fn prepare<'a>(
        &'a mut self,
        request: &'a Request,
        command: &'a Command,
    ) -> Result<Box<dyn PreparedOperation + 'a>> {
        if request.target != self.contract.target
            || request.action != command.action()
            || request.parameters_sha256 != command.digest()?
        {
            return Err(Error::InvalidInput);
        }
        self.contract.require(command.action())?;
        self.backend.prepare(request, command)
    }
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect> {
        if operation.request.target != self.contract.target {
            return Err(Error::InvalidInput);
        }
        self.contract.require(Action::Reconcile)?;
        self.backend.reconcile(operation)
    }
}
