//! Explicit host continuation control; no inference, tool execution or automatic recovery.
use crate::{
    runtime::{OwnedRuntime, base_manifest},
    settings::{bytes, now},
};
use agent_response_gateway::continuation::Session;
use gateway_management::{
    Digest, Effect, Error, FailureCode, Id, Operation, PreparedOperation, Request, Result,
    Snapshot, filesystem,
};
use gateway_management_api::Command;
use gateway_management_runtime::Runtime;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, MutexGuard},
    time::Duration,
};
pub struct Control {
    directory: PathBuf,
    environment: Arc<BTreeMap<String, String>>,
    client: reqwest::blocking::Client,
    _owner: File,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    operation: Id,
    request_sha256: Digest,
    after: Snapshot,
}
impl Control {
    pub fn open(directory: PathBuf, environment: Arc<BTreeMap<String, String>>) -> Result<Self> {
        filesystem::directory(&directory)?;
        if bytes(&directory.join("schema"), 128, true)? != b"gateway-management-continuation/v1\n" {
            return Err(Error::InvalidStore);
        }
        let owner = filesystem::lease(&directory.join("owner.lock"))?;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| Error::Storage)?;
        Ok(Self {
            directory,
            environment,
            client,
            _owner: owner,
        })
    }
    fn path(&self, id: &Id) -> PathBuf {
        self.directory.join(format!(
            "{}.json",
            Digest::of(id.as_str().as_bytes()).as_str()
        ))
    }
    fn connection(&self, runtime: &mut Runtime) -> Result<(String, String, Id)> {
        let status = runtime.status()?;
        let running = status.running.ok_or(Error::NotFound)?;
        let manifest = status.running_manifest.ok_or(Error::NotFound)?;
        let name = base_manifest(&manifest)["configuration"]["continuation"]["control_token_env"]
            .as_str()
            .ok_or(Error::Unsupported)?;
        let token = self
            .environment
            .get(name)
            .ok_or(Error::InvalidInput)?
            .clone();
        Ok((
            format!("http://{}", running.gateway.address),
            token,
            running.instance_id,
        ))
    }
    fn call(&self, base: &str, token: &str, id: &Id, body: Option<Value>) -> Result<Session> {
        if uuid::Uuid::parse_str(id.as_str()).is_err() {
            return Err(Error::InvalidInput);
        }
        let path = format!("{base}/__continuation/sessions/{id}", id = id.as_str());
        let request = if let Some(body) = body {
            self.client.post(format!("{path}/transitions")).json(&body)
        } else {
            self.client.get(path)
        };
        let response = request
            .bearer_auth(token)
            .send()
            .map_err(|_| Error::Storage)?;
        if !response.status().is_success() {
            return Err(Error::Conflict);
        }
        let mut raw = Vec::new();
        response
            .take(65537)
            .read_to_end(&mut raw)
            .map_err(|_| Error::Storage)?;
        if raw.len() > 65536 {
            return Err(Error::InvalidStore);
        }
        let value: Session = serde_json::from_slice(&raw).map_err(|_| Error::InvalidStore)?;
        value.origin.validate().map_err(|_| Error::InvalidStore)?;
        if value.id != id.as_str() || value.epoch <= 0 || value.revision < 0 {
            return Err(Error::InvalidStore);
        }
        Ok(value)
    }
    fn snapshot(instance: &Id, session: &Session) -> Result<Snapshot> {
        Ok(Snapshot {
            revision: session
                .revision
                .try_into()
                .map_err(|_| Error::InvalidStore)?,
            digest: Digest::of(
                &serde_json::to_vec(&(instance, session)).map_err(|_| Error::InvalidStore)?,
            ),
        })
    }
    pub fn view(&self, runtime: &OwnedRuntime, id: &Id) -> Result<Value> {
        let mut runtime = runtime.lock().map_err(|_| Error::Storage)?;
        let (base, token, instance) = self.connection(&mut runtime)?;
        let session = self.call(&base, &token, id, None)?;
        Ok(
            json!({"schema":"gateway-management-continuation/v1","observed_at_ms":now()?,"instance":instance,"session":session}),
        )
    }
    pub fn prepare<'a>(
        &'a self,
        runtime: &'a OwnedRuntime,
        request: &'a Request,
        command: &Command,
    ) -> Result<Box<dyn PreparedOperation + 'a>> {
        let Command::ContinuationTransition {
            session,
            revision,
            transition_kind,
            portable_sha256,
            decision_reference,
            pending_tools,
            pending_approvals,
        } = command
        else {
            return Err(Error::Unsupported);
        };
        if *pending_tools
            || *pending_approvals
            || !matches!(
                transition_kind.as_str(),
                "compact_begin" | "compact_commit" | "recover"
            )
            || decision_reference.as_str().len() > 128
            || !decision_reference
                .as_str()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(Error::InvalidInput);
        }
        let mut owner = runtime.lock().map_err(|_| Error::Storage)?;
        let (base, token, instance) = self.connection(&mut owner)?;
        let current = self.call(&base, &token, session, None)?;
        if current.revision as u64 != *revision
            || matches!(current.status.as_str(), "pending" | "compacting_pending")
            || (transition_kind == "compact_begin"
                && (current.status != "ready" || current.pending_tools))
            || (transition_kind == "compact_commit" && current.status != "awaiting_compaction")
            || (transition_kind != "compact_begin" && portable_sha256.is_none())
        {
            return Err(Error::Conflict);
        }
        let before = Self::snapshot(&instance, &current)?;
        Ok(Box::new(Prepared {
            control: self,
            _runtime: owner,
            request,
            instance,
            base,
            token: zeroize::Zeroizing::new(token),
            session: session.clone(),
            before,
            operation: None,
            body: json!({"revision":revision,"kind":transition_kind,"portable_sha256":portable_sha256,"decision_reference":decision_reference,"pending_tools":false,"pending_approvals":false}),
        }))
    }
    pub fn reconcile(&self, operation: &Operation) -> Result<Effect> {
        let raw = match bytes(&self.path(&operation.id), 4096, true) {
            Ok(b) => b,
            Err(_) => {
                return Ok(Effect::Uncertain {
                    code: FailureCode::Unverified,
                });
            }
        };
        let receipt: Receipt = serde_json::from_slice(&raw).map_err(|_| Error::InvalidStore)?;
        if receipt.schema != "gateway-management-continuation-evidence/v1"
            || receipt.operation != operation.id
            || receipt.request_sha256 != operation.request.fingerprint()?
        {
            return Err(Error::InvalidStore);
        }
        Ok(Effect::Applied {
            after: receipt.after,
            evidence_sha256: Digest::of(&raw),
        })
    }
}
struct Prepared<'a> {
    control: &'a Control,
    _runtime: MutexGuard<'a, Runtime>,
    request: &'a Request,
    instance: Id,
    base: String,
    token: zeroize::Zeroizing<String>,
    session: Id,
    before: Snapshot,
    operation: Option<Id>,
    body: Value,
}
impl PreparedOperation for Prepared<'_> {
    fn before(&self) -> &Snapshot {
        &self.before
    }
    fn accepted(&mut self, id: &Id) {
        self.operation = Some(id.clone())
    }
    fn apply(&mut self) -> Effect {
        let Some(operation) = self.operation.clone() else {
            return Effect::NotApplied {
                code: FailureCode::Rejected,
            };
        };
        let check = self
            .control
            .call(&self.base, &self.token, &self.session, None)
            .and_then(|s| Control::snapshot(&self.instance, &s));
        if check.as_ref() != Ok(&self.before) {
            return Effect::NotApplied {
                code: FailureCode::Stale,
            };
        }
        let result = (|| {
            let session = self.control.call(
                &self.base,
                &self.token,
                &self.session,
                Some(self.body.clone()),
            )?;
            let after = Control::snapshot(&self.instance, &session)?;
            if after.revision
                != self
                    .before
                    .revision
                    .checked_add(1)
                    .ok_or(Error::InvalidStore)?
            {
                return Err(Error::InvalidStore);
            }
            let raw = serde_json::to_vec(&Receipt {
                schema: "gateway-management-continuation-evidence/v1".into(),
                operation: operation.clone(),
                request_sha256: self.request.fingerprint()?,
                after: after.clone(),
            })
            .map_err(|_| Error::Storage)?;
            let mut file = filesystem::private_new(&self.control.path(&operation))?;
            file.write_all(&raw)
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::Storage)?;
            #[cfg(unix)]
            File::open(&self.control.directory)
                .and_then(|f| f.sync_all())
                .map_err(|_| Error::Storage)?;
            Ok(Effect::Applied {
                after,
                evidence_sha256: Digest::of(&raw),
            })
        })();
        result.unwrap_or(Effect::Uncertain {
            code: FailureCode::Unverified,
        })
    }
}
