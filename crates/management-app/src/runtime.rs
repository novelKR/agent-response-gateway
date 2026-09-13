use crate::settings::now;
use gateway_management::{Digest, Effect, Error, Id, PreparedOperation, Request, Result, Snapshot};
use gateway_management_runtime::{Command, Runtime, Status};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};

pub type OwnedRuntime = Arc<Mutex<Runtime>>;
pub fn base_manifest(manifest: &Value) -> &Value {
    if manifest.get("execution_sha256").is_some() {
        &manifest["configuration"]["gateway"]
    } else {
        manifest
    }
}
/// The lock pins process ownership through durable intent and execution. Canonical adapter
/// validation is reused before admission and again immediately before the effect.
pub struct LockedRuntime<'a> {
    runtime: MutexGuard<'a, Runtime>,
    command: Command,
    request: &'a Request,
    intent: Digest,
    before: Snapshot,
    operation: Option<Id>,
}
impl<'a> LockedRuntime<'a> {
    pub fn new(
        owner: &'a OwnedRuntime,
        request: &'a Request,
        command: Command,
        intent: Digest,
    ) -> Result<Self> {
        let mut runtime = owner.lock().map_err(|_| Error::Storage)?;
        let before = runtime
            .prepare_mapped(request, &command, &intent)?
            .before()
            .clone();
        Ok(Self {
            runtime,
            command,
            request,
            intent,
            before,
            operation: None,
        })
    }
}
impl PreparedOperation for LockedRuntime<'_> {
    fn before(&self) -> &Snapshot {
        &self.before
    }
    fn accepted(&mut self, id: &Id) {
        self.operation = Some(id.clone());
    }
    fn apply(&mut self) -> Effect {
        let Some(operation) = &self.operation else {
            return Effect::NotApplied {
                code: gateway_management::FailureCode::Rejected,
            };
        };
        match self
            .runtime
            .prepare_mapped(self.request, &self.command, &self.intent)
        {
            Ok(mut prepared) if prepared.before() == &self.before => {
                prepared.accepted(operation);
                prepared.apply()
            }
            _ => Effect::NotApplied {
                code: gateway_management::FailureCode::Stale,
            },
        }
    }
}
pub fn effective(
    status: &Status,
    native: bool,
) -> Result<Option<gateway_management_extensions::EffectiveSelection>> {
    let (Some(ready), Some(manifest)) = (&status.running, &status.running_manifest) else {
        return Ok(None);
    };
    let activation = if native {
        &manifest["configuration"]["extensions"]["activation"]["extensions"]
    } else {
        &base_manifest(manifest)["configuration"]["profile_packs"]["activation"]["packs"]
    };
    let packages = if activation.is_null() {
        Vec::new()
    } else {
        activation
            .as_array()
            .ok_or(Error::InvalidStore)?
            .iter()
            .map(|p| {
                Ok(gateway_management_extensions::Selection {
                    id: Id::new(p["id"].as_str().ok_or(Error::InvalidStore)?)?,
                    version: p["version"].as_str().ok_or(Error::InvalidStore)?.into(),
                    package_sha256: Digest::try_from(
                        p["package_sha256"]
                            .as_str()
                            .ok_or(Error::InvalidStore)?
                            .to_owned(),
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?
    };
    Ok(Some(gateway_management_extensions::EffectiveSelection {
        instance: ready.instance_id.clone(),
        observed_at_ms: now()?,
        configuration_sha256: ready.gateway.configuration_sha256.clone(),
        execution_sha256: ready.gateway.execution_sha256.clone(),
        packages,
    }))
}
