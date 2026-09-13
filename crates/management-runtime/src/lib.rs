//! Optional owned runtime/configuration adapter. It creates no global runtime or listener.
mod files;
pub mod protocol;

use agent_response_gateway::{Config, extensions::ExtensionPlan};
use gateway_management::{
    Action, Backend, Digest, Effect, Error, FailureCode, Id, Operation, PreparedOperation, Request,
    Result, Snapshot, filesystem,
};
use protocol::{Inspection, Launch, Ready};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, Read, Write},
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct PreparedLaunch {
    pub config: Config,
    pub extensions: Option<ExtensionPlan>,
}
pub fn prepare_launch(launch: &Launch) -> Result<PreparedLaunch> {
    if launch.schema != protocol::LAUNCH_SCHEMA {
        return Err(Error::InvalidInput);
    }
    let inspected = protocol::inspect(
        &launch.configuration,
        launch.extensions_lock.as_deref(),
        launch.profile_packs_lock.as_deref(),
    )?;
    if inspected.configuration_sha256 != launch.configuration_sha256
        || inspected.execution_sha256 != launch.execution_sha256
    {
        return Err(Error::Conflict);
    }
    Ok(PreparedLaunch {
        config: inspected.config,
        extensions: inspected.extensions,
    })
}

/// Only trusted host setup supplies paths. HTTP commands refer to registered IDs.
#[derive(Clone, Serialize)]
pub struct Source {
    pub configuration: PathBuf,
    pub extensions_lock: Option<PathBuf>,
    pub profile_packs_lock: Option<PathBuf>,
}
pub struct Registration {
    pub target: Id,
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub executable_sha256: Digest,
    pub credential_generation: Id,
    pub sources: BTreeMap<Id, Source>,
    /// Explicit host bindings; only Config references and Windows SYSTEMROOT are passed.
    pub environment: BTreeMap<String, String>,
    pub startup_timeout: Duration,
    pub stop_timeout: Duration,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Stage {
        source: Id,
        candidate: Id,
        source_sha256: Digest,
    },
    Select {
        candidate: Id,
    },
    Start,
    Stop,
    Restart,
}
impl Command {
    pub fn action(&self) -> Action {
        match self {
            Self::Stage { .. } => Action::ConfigurationStage,
            Self::Select { .. } => Action::ConfigurationSelect,
            Self::Start => Action::RuntimeStart,
            Self::Stop => Action::RuntimeStop,
            Self::Restart => Action::RuntimeRestart,
        }
    }
    pub fn digest(&self) -> Result<Digest> {
        encode(self).map(|v| Digest::of(&v))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    source: Id,
    raw_sha256: Digest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema: String,
    revision: u64,
    selected: Option<Id>,
    candidates: BTreeMap<Id, Candidate>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub schema: &'static str,
    pub target: Id,
    pub revision: u64,
    pub selected: Option<Id>,
    pub candidates: Vec<Id>,
    pub external_change: bool,
    pub ownership: &'static str,
    pub running: Option<Ready>,
    pub running_manifest: Option<Value>,
    pub desired: Option<Value>,
    pub desired_valid: bool,
    pub restart_required: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    operation: Id,
    request_sha256: Digest,
    after: Snapshot,
    observation: Value,
}

struct Owned {
    child: Child,
    ready: Option<Ready>,
    manifest: Value,
}
pub struct Runtime {
    registration: Registration,
    _owner: File,
    state: State,
    state_digest: Digest,
    controller_id: Id,
    epoch: Id,
    owned: Option<Owned>,
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Error::InvalidInput)
}
fn generated_id() -> Id {
    Id::new(uuid::Uuid::new_v4().to_string()).expect("UUID identifier")
}
fn receipt_name(id: &Id) -> String {
    Digest::of(id.as_str().as_bytes()).as_str().to_owned()
}

impl Runtime {
    fn validate_registration(reg: &Registration) -> Result<()> {
        filesystem::directory(&reg.directory)?;
        if !reg.executable.is_absolute()
            || reg.sources.len() > 128
            || !(Duration::from_millis(100)..=Duration::from_secs(60))
                .contains(&reg.startup_timeout)
            || !(Duration::from_millis(100)..=Duration::from_secs(60)).contains(&reg.stop_timeout)
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
    pub fn initialize(registration: Registration) -> Result<Self> {
        Self::validate_registration(&registration)?;
        let owner = filesystem::lease(&registration.directory.join("manager.owner"))?;
        for name in ["candidates", "evidence", "manifests"] {
            files::private_directory(&registration.directory.join(name))?;
        }
        drop(filesystem::private_new(
            &registration.directory.join("runtime.lease"),
        )?);
        let state = State {
            schema: "gateway-runtime-state/v1".into(),
            revision: 0,
            selected: None,
            candidates: BTreeMap::new(),
        };
        let bytes = encode(&state)?;
        files::create(&registration.directory.join("state.json"), &bytes)?;
        Ok(Self {
            registration,
            _owner: owner,
            state,
            state_digest: Digest::of(&bytes),
            controller_id: generated_id(),
            epoch: generated_id(),
            owned: None,
        })
    }
    pub fn open(registration: Registration) -> Result<Self> {
        Self::validate_registration(&registration)?;
        let owner = filesystem::lease(&registration.directory.join("manager.owner"))?;
        filesystem::regular(&registration.directory.join("runtime.lease"))?;
        let bytes = files::read(
            &registration.directory.join("state.json"),
            files::MAX_CONFIG,
            true,
        )?;
        let state: State = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidStore)?;
        if encode(&state)? != bytes
            || state.schema != "gateway-runtime-state/v1"
            || state.candidates.len() > 128
            || state
                .selected
                .as_ref()
                .is_some_and(|id| !state.candidates.contains_key(id))
        {
            return Err(Error::InvalidStore);
        }
        Ok(Self {
            registration,
            _owner: owner,
            state,
            state_digest: Digest::of(&bytes),
            controller_id: generated_id(),
            epoch: generated_id(),
            owned: None,
        })
    }
    fn candidate_path(&self, id: &Id) -> PathBuf {
        self.registration
            .directory
            .join("candidates")
            .join(format!("{}.toml", receipt_name(id)))
    }
    fn candidate(&self, id: &Id) -> Result<Inspection> {
        let candidate = self.state.candidates.get(id).ok_or(Error::NotFound)?;
        let source = self
            .registration
            .sources
            .get(&candidate.source)
            .ok_or(Error::InvalidStore)?;
        let path = self.candidate_path(id);
        let bytes = files::read(&path, files::MAX_CONFIG, true)?;
        if Digest::of(&bytes) != candidate.raw_sha256 {
            return Err(Error::Conflict);
        }
        protocol::inspect_bytes(
            &bytes,
            source.extensions_lock.as_deref(),
            source.profile_packs_lock.as_deref(),
        )
    }
    fn desired(&self) -> Result<Inspection> {
        self.candidate(self.state.selected.as_ref().ok_or(Error::NotFound)?)
    }
    /// Inspect a registered immutable candidate against the current activation locks.
    /// This reports the next launch, not the manifest of an already running child.
    pub fn candidate_manifest(&self, candidate: &Id) -> Result<Value> {
        self.candidate(candidate)
            .map(|inspection| inspection.manifest)
    }
    fn external_change(&self) -> bool {
        files::read(
            &self.registration.directory.join("state.json"),
            files::MAX_CONFIG,
            true,
        )
        .map(|b| Digest::of(&b) != self.state_digest)
        .unwrap_or(true)
    }
    fn refresh_child(&mut self) -> Result<()> {
        if let Some(owned) = &mut self.owned
            && owned
                .child
                .try_wait()
                .map_err(|_| Error::Storage)?
                .is_some()
        {
            self.owned.take();
            self.epoch = generated_id();
        }
        Ok(())
    }
    fn lease_free(&self) -> Result<bool> {
        let path = self.registration.directory.join("runtime.lease");
        filesystem::regular(&path)?;
        match filesystem::lease(&path) {
            Ok(lease) => {
                drop(lease);
                Ok(true)
            }
            Err(Error::AlreadyOwned) => Ok(false),
            Err(error) => Err(error),
        }
    }
    fn view(&mut self) -> Result<(Status, Option<Inspection>)> {
        self.refresh_child()?;
        let desired = self.desired();
        let running = self.owned.as_ref().and_then(|o| o.ready.clone());
        let restart_required = match (&running, &desired) {
            (Some(r), Ok(d)) => {
                r.gateway.configuration_sha256 != d.configuration_sha256
                    || r.gateway.execution_sha256 != d.execution_sha256
            }
            (Some(_), Err(_)) => true,
            _ => false,
        };
        let ownership = if self.owned.is_some() {
            "owned"
        } else if self.lease_free()? {
            "stopped"
        } else {
            "unowned"
        };
        let status = Status {
            schema: "gateway-runtime-status/v1",
            target: self.registration.target.clone(),
            revision: self.state.revision,
            selected: self.state.selected.clone(),
            candidates: self.state.candidates.keys().cloned().collect(),
            external_change: self.external_change(),
            ownership,
            running,
            running_manifest: self
                .owned
                .as_ref()
                .filter(|o| o.ready.is_some())
                .map(|o| o.manifest.clone()),
            desired_valid: desired.is_ok(),
            desired: desired.as_ref().ok().map(|d| d.manifest.clone()),
            restart_required,
        };
        Ok((status, desired.ok()))
    }
    pub fn status(&mut self) -> Result<Status> {
        self.view().map(|(status, _)| status)
    }
    fn observation(&mut self) -> Result<Value> {
        let (status, desired) = self.view()?;
        let running = status.running.as_ref().map(|r| &r.gateway);
        let configuration = running.map(|r| &r.configuration_sha256);
        let execution = running.and_then(|r| r.execution_sha256.as_ref());
        let registration = Digest::of(&encode(&(
            &self.registration.target,
            &self.registration.executable,
            &self.registration.executable_sha256,
            &self.registration.credential_generation,
            &self.registration.sources,
        ))?);
        Ok(
            json!({"controller":self.controller_id,"epoch":self.epoch,"registration":registration,
            "saved_state":self.state_digest,"revision":self.state.revision,"selected":status.selected,
            "candidates":status.candidates,"ownership":status.ownership,"external_change":status.external_change,
            "instance":status.running.as_ref().map(|r|&r.instance_id),"running_configuration":configuration,
            "running_execution":execution,"desired_configuration":desired.as_ref().map(|d|&d.configuration_sha256),
            "desired_execution":desired.as_ref().and_then(|d|d.execution_sha256.as_ref()),
            "desired_manifest":desired.as_ref().map(|d|encode(&d.manifest).map(|v|Digest::of(&v))).transpose()?,
            "desired_valid":status.desired_valid,"restart_required":status.restart_required}),
        )
    }
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let observation = self.observation()?;
        Ok(Snapshot {
            revision: self.state.revision,
            digest: Digest::of(&encode(&observation)?),
        })
    }
    fn retain_manifest(&self, inspection: &Inspection) -> Result<Digest> {
        let bytes = encode(&inspection.manifest)?;
        let digest = Digest::of(&bytes);
        let path = self
            .registration
            .directory
            .join("manifests")
            .join(format!("{}.json", digest.as_str()));
        if path.exists() {
            if files::read(&path, files::MAX_CONFIG, true)? != bytes {
                return Err(Error::Conflict);
            }
        } else {
            files::create(&path, &bytes)?;
        }
        Ok(digest)
    }
    pub fn bind(&mut self, command: Command) -> Bound<'_> {
        Bound {
            runtime: self,
            command,
        }
    }
    fn save(&mut self, state: State) -> Result<()> {
        if self.external_change() {
            return Err(Error::Conflict);
        }
        let bytes = encode(&state)?;
        files::replace(&self.registration.directory.join("state.json"), &bytes)?;
        self.state = state;
        self.state_digest = Digest::of(&bytes);
        Ok(())
    }
    fn stage(&mut self, source: &Id, candidate: &Id, expected: &Digest) -> Result<()> {
        let binding = self
            .registration
            .sources
            .get(source)
            .ok_or(Error::NotFound)?;
        let bytes = files::read(&binding.configuration, files::MAX_CONFIG, false)?;
        if Digest::of(&bytes) != *expected {
            return Err(Error::Conflict);
        }
        let inspection = protocol::inspect_bytes(
            &bytes,
            binding.extensions_lock.as_deref(),
            binding.profile_packs_lock.as_deref(),
        )?;
        self.retain_manifest(&inspection)?;
        if self.state.candidates.contains_key(candidate) || self.state.candidates.len() >= 128 {
            return Err(Error::Conflict);
        }
        let mut next = self.state.clone();
        next.candidates.insert(
            candidate.clone(),
            Candidate {
                source: source.clone(),
                raw_sha256: expected.clone(),
            },
        );
        next.revision = next.revision.checked_add(1).ok_or(Error::InvalidStore)?;
        files::create(&self.candidate_path(candidate), &bytes)?;
        self.save(next)
    }
    fn select(&mut self, candidate: &Id) -> Result<()> {
        let _ = self.candidate(candidate)?;
        let mut next = self.state.clone();
        next.selected = Some(candidate.clone());
        next.revision = next.revision.checked_add(1).ok_or(Error::InvalidStore)?;
        self.save(next)
    }
    fn environment(&self, config: &Config) -> Result<BTreeMap<String, String>> {
        let mut names = BTreeSet::from([config.local_token_env.as_str()]);
        // The Windows socket provider needs the operating-system environment binding.
        // Require an explicit host value; never fall back to inheriting the manager environment.
        #[cfg(windows)]
        names.insert("SYSTEMROOT");
        for provider in config.providers.values() {
            names.insert(&provider.api_key_env);
        }
        if let Some(c) = &config.continuation {
            names.insert(&c.key_env);
            names.insert(&c.control_token_env);
        }
        names
            .into_iter()
            .map(|name| {
                Ok((
                    name.to_owned(),
                    self.registration
                        .environment
                        .get(name)
                        .ok_or(Error::InvalidInput)?
                        .clone(),
                ))
            })
            .collect()
    }
    fn start(&mut self) -> Result<()> {
        self.refresh_child()?;
        if self.owned.is_some() || !self.lease_free()? {
            return Err(Error::Conflict);
        }
        if files::hash_file(&self.registration.executable)? != self.registration.executable_sha256 {
            return Err(Error::Conflict);
        }
        let inspected = self.desired()?;
        self.retain_manifest(&inspected)?;
        let candidate = self.state.selected.clone().ok_or(Error::NotFound)?;
        let source = &self.registration.sources[&self.state.candidates[&candidate].source];
        let launch = Launch {
            schema: protocol::LAUNCH_SCHEMA.into(),
            instance_id: generated_id(),
            directory: self.registration.directory.clone(),
            configuration: self.candidate_path(&candidate),
            extensions_lock: source.extensions_lock.clone(),
            profile_packs_lock: source.profile_packs_lock.clone(),
            configuration_sha256: inspected.configuration_sha256.clone(),
            execution_sha256: inspected.execution_sha256.clone(),
        };
        let mut command = ProcessCommand::new(&self.registration.executable);
        command
            .env_clear()
            .envs(self.environment(&inspected.config)?)
            .current_dir(&self.registration.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| Error::Storage)?;
        let stdout = child.stdout.take().ok_or(Error::Storage)?;
        self.owned = Some(Owned {
            child,
            ready: None,
            manifest: inspected.manifest.clone(),
        });
        let result = (|| {
            let deadline = Instant::now() + self.registration.startup_timeout;
            let mut input = self
                .owned
                .as_mut()
                .and_then(|o| o.child.stdin.take())
                .ok_or(Error::Storage)?;
            let mut frame = encode(&launch)?;
            frame.push(b'\n');
            if frame.len() > 65536 {
                return Err(Error::InvalidInput);
            }
            let (written_tx, written_rx) = mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("managed-launch".into())
                .spawn(move || {
                    let result = input.write_all(&frame).and_then(|_| input.flush());
                    let _ = written_tx.send((input, result));
                })
                .map_err(|_| Error::Storage)?;
            let (input, result) = written_rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| Error::Storage)?;
            result.map_err(|_| Error::Storage)?;
            self.owned.as_mut().ok_or(Error::Storage)?.child.stdin = Some(input);
            let (tx, rx) = mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("managed-readiness".into())
                .spawn(move || {
                    let mut data = vec![];
                    let result = std::io::BufReader::new(stdout)
                        .take(65537)
                        .read_until(b'\n', &mut data)
                        .map(|_| data);
                    let _ = tx.send(result);
                })
                .map_err(|_| Error::Storage)?;
            let bytes = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| Error::Storage)?
                .map_err(|_| Error::Storage)?;
            if bytes.len() > 65536 || bytes.last() != Some(&b'\n') {
                return Err(Error::InvalidInput);
            }
            let ready: Ready = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidInput)?;
            let bound = ready.gateway.address;
            let expected = inspected
                .config
                .manifest()
                .map_err(|_| Error::InvalidInput)?
                .readiness(bound, inspected.extensions.as_ref())
                .map_err(|_| Error::InvalidInput)?;
            if ready.schema != protocol::READY_SCHEMA
                || ready.instance_id != launch.instance_id
                || serde_json::to_value(&ready.gateway).map_err(|_| Error::InvalidInput)?
                    != expected
            {
                return Err(Error::Conflict);
            }
            let owned = self.owned.as_mut().ok_or(Error::Storage)?;
            if owned
                .child
                .try_wait()
                .map_err(|_| Error::Storage)?
                .is_some()
            {
                return Err(Error::Storage);
            }
            owned.ready = Some(ready);
            Ok(())
        })();
        if result.is_err() {
            let _ = self.stop_owned();
        }
        result
    }
    /// Best-effort cleanup of this object's actual child handle, independent of audit availability.
    pub fn stop_owned(&mut self) -> Result<()> {
        let Some(mut owned) = self.owned.take() else {
            return if self.lease_free()? {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        };
        drop(owned.child.stdin.take());
        let deadline = Instant::now() + self.registration.stop_timeout;
        loop {
            match owned.child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(Error::Storage)
                    };
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = owned.child.kill();
                    let force_deadline = Instant::now() + Duration::from_secs(1);
                    loop {
                        if owned.child.try_wait().ok().flatten().is_some() {
                            return Err(Error::Storage);
                        }
                        if Instant::now() >= force_deadline {
                            self.owned = Some(owned);
                            return Err(Error::Storage);
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }
    }
    fn perform(&mut self, command: &Command) -> Result<()> {
        match command {
            Command::Stage {
                source,
                candidate,
                source_sha256,
            } => self.stage(source, candidate, source_sha256),
            Command::Select { candidate } => self.select(candidate),
            Command::Start => self.start(),
            Command::Stop => self.stop_owned(),
            Command::Restart => {
                let _ = self.desired()?;
                self.stop_owned()?;
                self.start()
            }
        }
    }
    fn receipt_path(&self, id: &Id) -> PathBuf {
        self.registration
            .directory
            .join("evidence")
            .join(format!("{}.json", receipt_name(id)))
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if self.owned.is_some() {
            let _ = self.stop_owned();
        }
    }
}

pub struct Bound<'a> {
    runtime: &'a mut Runtime,
    command: Command,
}
struct Prepared<'a> {
    runtime: &'a mut Runtime,
    command: Command,
    request: Request,
    before: Snapshot,
    operation: Option<Id>,
}
impl Backend for Bound<'_> {
    fn prepare<'a>(&'a mut self, request: &'a Request) -> Result<Box<dyn PreparedOperation + 'a>> {
        if request.target != self.runtime.registration.target
            || request.action != self.command.action()
            || request.parameters_sha256 != self.command.digest()?
        {
            return Err(Error::InvalidInput);
        }
        if self.runtime.external_change() && !matches!(self.command, Command::Stop) {
            return Err(Error::Conflict);
        }
        match &self.command {
            Command::Stage {
                source,
                candidate,
                source_sha256,
            } => {
                if self.runtime.state.candidates.contains_key(candidate)
                    || self.runtime.state.candidates.len() >= 128
                {
                    return Err(Error::Conflict);
                }
                let registered = self
                    .runtime
                    .registration
                    .sources
                    .get(source)
                    .ok_or(Error::NotFound)?;
                if files::hash_file(&registered.configuration)? != *source_sha256 {
                    return Err(Error::Conflict);
                }
                let _ = protocol::inspect(
                    &registered.configuration,
                    registered.extensions_lock.as_deref(),
                    registered.profile_packs_lock.as_deref(),
                )?;
            }
            Command::Select { candidate } => {
                let _ = self.runtime.candidate(candidate)?;
            }
            Command::Start | Command::Restart => {
                self.runtime.refresh_child()?;
                if (matches!(self.command, Command::Start) && self.runtime.owned.is_some())
                    || (self.runtime.owned.is_none() && !self.runtime.lease_free()?)
                {
                    return Err(Error::Conflict);
                }
                let inspected = self.runtime.desired()?;
                let _ = self.runtime.environment(&inspected.config)?;
                if files::hash_file(&self.runtime.registration.executable)?
                    != self.runtime.registration.executable_sha256
                {
                    return Err(Error::Conflict);
                }
            }
            Command::Stop => {
                self.runtime.refresh_child()?;
                if self.runtime.owned.is_none() && !self.runtime.lease_free()? {
                    return Err(Error::Conflict);
                }
            }
        }
        let before = self.runtime.snapshot()?;
        Ok(Box::new(Prepared {
            runtime: self.runtime,
            command: self.command.clone(),
            request: request.clone(),
            before,
            operation: None,
        }))
    }
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect> {
        let path = self.runtime.receipt_path(&operation.id);
        let bytes = match files::read(&path, 65536, true) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(Effect::Uncertain {
                    code: FailureCode::Unverified,
                });
            }
        };
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidStore)?;
        if encode(&receipt)? != bytes
            || receipt.schema != "gateway-runtime-evidence/v1"
            || receipt.operation != operation.id
            || receipt.request_sha256 != operation.request.fingerprint()?
            || receipt.after.digest != Digest::of(&encode(&receipt.observation)?)
            || receipt.observation["epoch"] != operation.id.as_str()
            || receipt.observation["revision"].as_u64() != Some(receipt.after.revision)
        {
            return Err(Error::InvalidStore);
        }
        Ok(Effect::Applied {
            after: receipt.after,
            evidence_sha256: Digest::of(&bytes),
        })
    }
}
impl PreparedOperation for Prepared<'_> {
    fn before(&self) -> &Snapshot {
        &self.before
    }
    fn accepted(&mut self, id: &Id) {
        self.operation = Some(id.clone());
    }
    fn apply(&mut self) -> Effect {
        let Some(operation) = self.operation.clone() else {
            return Effect::NotApplied {
                code: FailureCode::Rejected,
            };
        };
        if self.runtime.snapshot().as_ref() != Ok(&self.before) {
            return Effect::NotApplied {
                code: FailureCode::Stale,
            };
        }
        self.runtime.epoch = operation.clone();
        let result = (|| {
            self.runtime.perform(&self.command)?;
            let observation = self.runtime.observation()?;
            let after = Snapshot {
                revision: self.runtime.state.revision,
                digest: Digest::of(&encode(&observation)?),
            };
            let receipt = Receipt {
                schema: "gateway-runtime-evidence/v1".into(),
                operation: operation.clone(),
                request_sha256: self.request.fingerprint()?,
                after: after.clone(),
                observation,
            };
            let bytes = encode(&receipt)?;
            files::complete_evidence(&self.runtime.receipt_path(&operation), &bytes)?;
            Ok::<_, Error>(Effect::Applied {
                after,
                evidence_sha256: Digest::of(&bytes),
            })
        })();
        result.unwrap_or(Effect::Uncertain {
            code: FailureCode::EffectFailed,
        })
    }
}
