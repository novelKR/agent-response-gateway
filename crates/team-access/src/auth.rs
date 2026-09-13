use crate::{Credential, Permissions, Principal, Purpose, store::*};
use gateway_management::{
    Action, Actor, Digest, Error, Grant, Id, Identity, Reader, Result, State,
};
use gateway_management_api::{
    Authenticator as ManagementAuthenticator, CredentialKind, Principal as ManagementPrincipal,
};
use rusqlite::{Connection, OptionalExtension};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

/// Independently opened read-only authority adapter. No listener or background activity.
/// Positive immutable audit evidence is cached; an established credential does not require
/// a new management write or repeated audit read for each model call.
pub struct Authenticator {
    state: Mutex<Authority>,
    target: Id,
}
struct Authority {
    db: Connection,
    audit: Reader,
    evidence_actor: Actor,
    confirmed: BTreeMap<Id, (Digest, Digest)>,
}
impl Authenticator {
    pub fn open(path: &Path, target: Id, audit: Reader) -> Result<Self> {
        let db = open_db(path, &target, false)?;
        let evidence_actor = Actor::new(
            Identity {
                subject: Id::new("team-authority")?,
                credential: Id::new("internal-evidence-reader")?,
            },
            [Grant {
                action: Action::ReadOperations,
                target: target.clone(),
            }],
        )?;
        Ok(Self {
            target,
            state: Mutex::new(Authority {
                db,
                audit,
                evidence_actor,
                confirmed: BTreeMap::new(),
            }),
        })
    }
    fn principal(&self, selected: Select<'_>) -> Result<Principal> {
        let mut authority = self.state.lock().map_err(|_| Error::Storage)?;
        // A single read transaction binds subject permissions to credential status.
        let tx = authority.db.unchecked_transaction().map_err(storage)?;
        let key = match selected {
            Select::Token(purpose, token) => {
                if token.len() != purpose.prefix().len() + 64
                    || !token.starts_with(purpose.prefix())
                    || !token[purpose.prefix().len()..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(Error::Forbidden);
                }
                let hash = Digest::of(token.as_bytes());
                let id: Option<String> = tx
                    .query_row(
                        "SELECT id FROM credentials WHERE verifier=?1",
                        [hash.as_str()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(storage)?;
                let key = credential(&tx, &Id::new(id.ok_or(Error::Forbidden)?)?)?;
                if key.verifier != hash || key.metadata.purpose != purpose {
                    return Err(Error::Forbidden);
                }
                key
            }
            Select::Identity(identity) => {
                let key = credential(&tx, &identity.credential)?;
                if key.metadata.subject != identity.subject {
                    return Err(Error::Forbidden);
                }
                key
            }
        };
        let owner = subject(&tx, &key.metadata.subject)?;
        if key.metadata.revoked || !owner.permissions.enabled {
            return Err(Error::Forbidden);
        }
        let version =
            Digest::of(json(&(&key.metadata, owner.revision, &owner.permissions))?.as_bytes());
        drop(tx);
        let proof = (key.request_sha256.clone(), credential_digest(&key)?);
        let confirmed = authority.confirmed.get(&key.metadata.issued_operation) == Some(&proof);
        if !confirmed {
            let op = authority.audit.get(
                &authority.evidence_actor,
                &self.target,
                &key.metadata.issued_operation,
            )?;
            if op.state != State::Succeeded
                || op.request.fingerprint() != Ok(key.request_sha256.clone())
                || !matches!(
                    op.request.action,
                    Action::TeamCredentialIssue | Action::TeamCredentialRotate
                )
            {
                return Err(Error::Forbidden);
            }
            verify_issuance(&authority.db, &op, &key)?;
            if authority.confirmed.len() >= 1024 {
                return Err(Error::Conflict);
            }
            authority
                .confirmed
                .insert(key.metadata.issued_operation.clone(), proof);
        }
        Ok(Principal {
            identity: Identity {
                subject: key.metadata.subject,
                credential: key.metadata.id,
            },
            purpose: key.metadata.purpose,
            permissions: owner.permissions,
            authorization_version: version,
        })
    }
    pub fn authenticate_model(&self, token: &str) -> Option<Principal> {
        self.principal(Select::Token(Purpose::Model, token)).ok()
    }
    pub fn refresh_model(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        self.principal(Select::Identity(identity))
            .ok()
            .filter(|p| p.purpose == Purpose::Model && p.authorization_version == *version)
    }
    /// Safe metadata for a caller already authorized by its host; no verifier or secret.
    pub fn credential(&self, id: &Id) -> Result<Credential> {
        let authority = self.state.lock().map_err(|_| Error::Storage)?;
        Ok(credential(&authority.db, id)?.metadata)
    }
}
enum Select<'a> {
    Token(Purpose, &'a str),
    Identity(&'a Identity),
}
fn management(principal: Principal) -> Option<ManagementPrincipal> {
    let kind = match principal.purpose {
        Purpose::Model => return None,
        Purpose::Management => CredentialKind::Management,
        Purpose::ReadOnly => CredentialKind::ReadOnly,
    };
    let Permissions { management, .. } = principal.permissions;
    let grants = management.into_iter().filter(|g| {
        kind == CredentialKind::Management
            || matches!(
                g.action,
                Action::ReadState | Action::ReadUsage | Action::ReadOperations
            )
    });
    Some(ManagementPrincipal {
        actor: Actor::new(principal.identity, grants).ok()?,
        kind,
        authorization_version: principal.authorization_version,
    })
}
impl ManagementAuthenticator for Authenticator {
    fn authenticate(&self, token: &str) -> Option<ManagementPrincipal> {
        let purpose = if token.starts_with(Purpose::Management.prefix()) {
            Purpose::Management
        } else if token.starts_with(Purpose::ReadOnly.prefix()) {
            Purpose::ReadOnly
        } else {
            return None;
        };
        self.principal(Select::Token(purpose, token))
            .ok()
            .and_then(management)
    }
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<ManagementPrincipal> {
        self.principal(Select::Identity(identity))
            .ok()
            .filter(|p| p.authorization_version == *version)
            .and_then(management)
    }
}
