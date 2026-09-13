use crate::{
    runtime::{OwnedRuntime, base_manifest},
    settings::Usage,
};
use gateway_management::{Action, Digest, Error, Id, Identity, Result};
use gateway_management_api::{
    Authenticator, Command, LocalAuthenticator, Principal, TeamPermissions, TeamPurpose,
};
use gateway_team_access::{Command as TeamCommand, Permissions, Purpose};
use gateway_team_http::{ManagedOrigin, Peer, PeerIdentity, PeerSource, Route};
use std::{collections::BTreeMap, sync::Arc};

pub struct Authority {
    pub local: LocalAuthenticator,
    pub team: Option<Arc<gateway_team_access::Authenticator>>,
}
impl Authenticator for Authority {
    fn authenticate(&self, token: &str) -> Option<Principal> {
        if token.starts_with("gwt1_") {
            self.team.as_ref()?.authenticate(token)
        } else {
            self.local.authenticate(token)
        }
    }
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        if let Some(p) = self.local.refresh(identity, version) {
            return Some(p);
        }
        if identity.subject.as_str().starts_with("local:")
            || identity.credential.as_str().starts_with("local:")
        {
            return None;
        }
        self.team.as_ref()?.refresh(identity, version)
    }
}
fn permissions(p: &TeamPermissions) -> Permissions {
    Permissions {
        enabled: p.enabled,
        routes: p.routes.clone(),
        management: p.management.clone(),
        read_all_usage: p.read_all_usage,
    }
}
pub fn command(command: &Command) -> Result<TeamCommand> {
    let value = match command {
        Command::TeamSubjectRegister {
            subject,
            permissions: p,
        } => TeamCommand::Register {
            subject: subject.clone(),
            permissions: permissions(p),
        },
        Command::TeamPermissionsChange {
            subject,
            permissions: p,
        } => TeamCommand::PermissionsChange {
            subject: subject.clone(),
            permissions: permissions(p),
        },
        Command::TeamCredentialIssue {
            subject,
            credential,
            purpose,
        } => TeamCommand::Issue {
            subject: subject.clone(),
            credential: credential.clone(),
            purpose: match purpose {
                TeamPurpose::Model => Purpose::Model,
                TeamPurpose::Management => Purpose::Management,
                TeamPurpose::ReadOnly => Purpose::ReadOnly,
            },
        },
        Command::TeamCredentialRevoke { credential } => TeamCommand::Revoke {
            credential: credential.clone(),
        },
        Command::TeamCredentialRotate {
            credential,
            replacement,
        } => TeamCommand::Rotate {
            credential: credential.clone(),
            replacement: replacement.clone(),
        },
        _ => return Err(Error::Unsupported),
    };
    let ids = match &value {
        TeamCommand::Register { subject, .. } | TeamCommand::PermissionsChange { subject, .. } => {
            vec![subject]
        }
        TeamCommand::Issue {
            subject,
            credential,
            ..
        } => vec![subject, credential],
        TeamCommand::Revoke { credential } => vec![credential],
        TeamCommand::Rotate {
            credential,
            replacement,
        } => vec![credential, replacement],
    };
    if ids.iter().any(|id| id.as_str().starts_with("local:")) {
        return Err(Error::Forbidden);
    }
    Ok(value)
}
pub fn validate_namespace(manager: &gateway_team_access::Manager) -> Result<()> {
    let inventory = manager.inventory()?;
    if inventory
        .subjects
        .iter()
        .any(|s| s.id.as_str().starts_with("local:"))
        || inventory.credentials.iter().any(|c| {
            c.id.as_str().starts_with("local:") || c.subject.as_str().starts_with("local:")
        })
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
pub fn actions() -> Vec<Action> {
    vec![
        Action::TeamSubjectRegister,
        Action::TeamPermissionsChange,
        Action::TeamCredentialIssue,
        Action::TeamCredentialRevoke,
        Action::TeamCredentialRotate,
    ]
}
pub struct RuntimePeer {
    pub target: Id,
    pub runtime: OwnedRuntime,
    pub environment: Arc<BTreeMap<String, String>>,
    pub usage: Option<Usage>,
}
impl PeerSource for RuntimePeer {
    fn current(&self) -> Result<Peer> {
        let status = self.runtime.lock().map_err(|_| Error::Storage)?.status()?;
        let ready = status.running.ok_or(Error::NotFound)?;
        let manifest = status.running_manifest.ok_or(Error::NotFound)?;
        let config = &base_manifest(&manifest)["configuration"];
        let name = config["local_token_env"]
            .as_str()
            .ok_or(Error::InvalidStore)?;
        let token = self
            .environment
            .get(name)
            .ok_or(Error::InvalidInput)?
            .clone();
        let continuation = &config["continuation"];
        let control = continuation["control_token_env"]
            .as_str()
            .map(|n| self.environment.get(n).cloned().ok_or(Error::InvalidInput))
            .transpose()?;
        let mut routes = BTreeMap::new();
        for route in config["routes"].as_array().ok_or(Error::InvalidStore)? {
            let alias = route["alias"]
                .as_str()
                .ok_or(Error::InvalidStore)?
                .to_owned();
            let managed = if route["continuation_mode"] == "managed"
                || route["api"] == "gemini_interactions"
            {
                let mut route = route.clone();
                route
                    .as_object_mut()
                    .ok_or(Error::InvalidStore)?
                    .remove("api_key_env");
                Some(ManagedOrigin {
                    route,
                    realm: continuation["realm"]
                        .as_str()
                        .ok_or(Error::InvalidStore)?
                        .into(),
                    generation: continuation["generation"]
                        .as_str()
                        .ok_or(Error::InvalidStore)?
                        .into(),
                })
            } else {
                None
            };
            routes.insert(alias, Route { managed });
        }
        let producer = self.producer(&manifest);
        Peer::new(
            PeerIdentity {
                target: self.target.clone(),
                instance: ready.instance_id,
                configuration_sha256: ready.gateway.configuration_sha256,
                producer,
            },
            &format!("http://{}/", ready.gateway.address),
            routes,
            token,
            control,
        )
    }
}
impl RuntimePeer {
    fn producer(&self, manifest: &serde_json::Value) -> Option<Id> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let usage = self.usage.as_ref()?;
            let extensions = &manifest["configuration"]["extensions"];
            let store = std::path::PathBuf::from(extensions["store"].as_str()?);
            let store_id = extensions["activation"]["recorder"]["store_id"].as_str()?;
            if store.join("usage").join(store_id) != usage.directory {
                return None;
            }
            let reader =
                gateway_usage_recorder::Store::open(&usage.directory, false, false).ok()?;
            Id::new(reader.producer().ok()?).ok()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (&self.usage, manifest);
            None
        }
    }
}
