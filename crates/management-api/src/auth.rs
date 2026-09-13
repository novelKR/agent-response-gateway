use gateway_management::{Actor, Digest, Grant, Id, Identity};
use std::collections::BTreeMap;
use subtle::ConstantTimeEq;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialKind {
    Management,
    ReadOnly,
}
#[derive(Clone)]
pub struct Principal {
    pub actor: Actor,
    pub kind: CredentialKind,
    pub authorization_version: Digest,
}
/// Implement only behind a trusted authentication boundary. Refresh is keyed by a server-held
/// identity and version, not arbitrary client role/subject headers or request fields.
pub trait Authenticator: Send + Sync {
    fn authenticate(&self, credential: &str) -> Option<Principal>;
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<Principal>;
}
/// Explicit startup bindings. Neither token values nor this type implement Debug/Serialize.
pub struct LocalCredential {
    pub token: String,
    pub identity: Identity,
    pub kind: CredentialKind,
    pub grants: Vec<Grant>,
}
struct Entry {
    token: Digest,
    principal: Principal,
}
pub struct LocalAuthenticator {
    entries: BTreeMap<Id, Entry>,
}
impl LocalAuthenticator {
    pub fn new(credentials: Vec<LocalCredential>) -> gateway_management::Result<Self> {
        if credentials.is_empty() || credentials.len() > 128 {
            return Err(gateway_management::Error::InvalidInput);
        }
        let mut entries = BTreeMap::new();
        for credential in credentials {
            if !(32..=4096).contains(&credential.token.len())
                || !credential.token.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(gateway_management::Error::InvalidInput);
            }
            if credential.kind == CredentialKind::ReadOnly
                && credential.grants.iter().any(|g| {
                    !matches!(
                        g.action,
                        gateway_management::Action::ReadState
                            | gateway_management::Action::ReadUsage
                            | gateway_management::Action::ReadOperations
                    )
                })
            {
                return Err(gateway_management::Error::InvalidInput);
            }
            let token = Digest::of(credential.token.as_bytes());
            if entries.values().any(|entry: &Entry| entry.token == token)
                || entries.contains_key(&credential.identity.credential)
            {
                return Err(gateway_management::Error::InvalidInput);
            }
            let version = Digest::of(
                &serde_json::to_vec(&(
                    &credential.identity,
                    &credential.grants,
                    credential.kind == CredentialKind::Management,
                    token.as_str(),
                ))
                .map_err(|_| gateway_management::Error::InvalidInput)?,
            );
            let actor = Actor::new(credential.identity.clone(), credential.grants)?;
            entries.insert(
                credential.identity.credential,
                Entry {
                    token,
                    principal: Principal {
                        actor,
                        kind: credential.kind,
                        authorization_version: version,
                    },
                },
            );
        }
        Ok(Self { entries })
    }
}
impl Authenticator for LocalAuthenticator {
    fn authenticate(&self, credential: &str) -> Option<Principal> {
        if credential.len() > 4096 {
            return None;
        }
        let digest = Digest::of(credential.as_bytes());
        self.entries
            .values()
            .find(|e| {
                bool::from(
                    e.token
                        .as_str()
                        .as_bytes()
                        .ct_eq(digest.as_str().as_bytes()),
                )
            })
            .map(|e| e.principal.clone())
    }
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        let principal = &self.entries.get(&identity.credential)?.principal;
        (principal.actor.identity() == identity && principal.authorization_version == *version)
            .then(|| principal.clone())
    }
}
