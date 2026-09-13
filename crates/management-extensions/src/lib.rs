//! Optional audited local package adapters. No package code or gateway lifecycle is run here.
mod driver;
pub use driver::{Driver, Inventory, NativeDriver};
use gateway_management::{
    Action, Backend, Digest, Effect, Error, FailureCode, Id, Operation, PreparedOperation, Request,
    Result, Snapshot, filesystem,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Error::InvalidInput)
}
pub(crate) fn bytes(path: &Path, limit: u64, private: bool) -> Result<Vec<u8>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidInput);
    }
    for ancestor in path.ancestors() {
        let meta = ancestor
            .symlink_metadata()
            .map_err(|_| Error::InvalidStore)?;
        if meta.file_type().is_symlink() {
            return Err(Error::InvalidStore);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(Error::InvalidStore);
            }
        }
    }
    if private {
        filesystem::regular(path)?;
    }
    let file = File::open(path).map_err(|_| Error::Storage)?;
    let meta = file.metadata().map_err(|_| Error::Storage)?;
    if !meta.is_file() || meta.len() > limit {
        return Err(Error::InvalidStore);
    }
    let mut value = vec![];
    file.take(limit + 1)
        .read_to_end(&mut value)
        .map_err(|_| Error::Storage)?;
    if value.len() as u64 > limit {
        return Err(Error::InvalidStore);
    }
    Ok(value)
}
fn fresh() -> Id {
    Id::new(uuid::Uuid::new_v4().to_string()).expect("UUID identifier")
}

#[derive(Clone, Serialize)]
pub struct LocalSource {
    pub path: PathBuf,
    pub package_sha256: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: Id,
    pub version: String,
    pub package_sha256: Digest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecorderBinding {
    pub store_id: Id,
    pub mode: String,
    pub queue_capacity: u64,
    pub ack_timeout_ms: u64,
    pub config_sha256: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Install {
        source: Id,
    },
    Enable {
        package: Selection,
        grants: Vec<String>,
        recorder: Option<Id>,
    },
    Disable {
        package: Id,
    },
    Select {
        package: Selection,
        grants: Vec<String>,
        recorder: Option<Id>,
    },
}
impl Command {
    pub fn action(&self) -> Action {
        match self {
            Self::Install { .. } => Action::PackageInstall,
            Self::Enable { .. } => Action::PackageEnable,
            Self::Disable { .. } => Action::PackageDisable,
            Self::Select { .. } => Action::PackageSelect,
        }
    }
    pub fn digest(&self) -> Result<Digest> {
        Ok(Digest::of(&encode(self)?))
    }
}
pub struct Registration {
    pub target: Id,
    pub directory: PathBuf,
    pub store: PathBuf,
    pub driver: Driver,
    pub sources: BTreeMap<Id, LocalSource>,
    pub recorder_bindings: BTreeMap<Id, RecorderBinding>,
}
/// Supplied only by the trusted runtime owner. These entries mean effective configuration,
/// not a resident codec process. Lack of an observation is distinct from an empty selection.
#[derive(Clone, Debug, Serialize)]
pub struct EffectiveSelection {
    pub instance: Id,
    pub observed_at_ms: u64,
    pub configuration_sha256: Digest,
    pub execution_sha256: Option<Digest>,
    pub packages: Vec<Selection>,
}
#[derive(Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub target: Id,
    pub snapshot: Snapshot,
    pub store: Inventory,
    pub effective: Option<EffectiveSelection>,
    pub external_change: bool,
    pub removal_supported: bool,
    pub version_change_requires_disable: bool,
}
pub struct Manager {
    registration: Registration,
    registration_sha256: Digest,
    _owner: File,
    epoch: Id,
    last_inventory: Digest,
}
impl Manager {
    /// Initialize adapter evidence explicitly. The package store must already exist.
    pub fn initialize(registration: Registration) -> Result<Self> {
        let manager = Self::acquire(registration)?;
        let options = std::fs::DirBuilder::new();
        #[cfg(unix)]
        let options = {
            use std::os::unix::fs::DirBuilderExt;
            let mut options = options;
            options.mode(0o700);
            options
        };
        options
            .create(manager.registration.directory.join("snapshots"))
            .map_err(|_| Error::Storage)?;
        let marker = manager.registration.directory.join("schema");
        write_new(&marker, b"gateway-extension-manager/v1\n")?;
        Ok(manager)
    }
    pub fn open(registration: Registration) -> Result<Self> {
        let manager = Self::acquire(registration)?;
        filesystem::directory(&manager.registration.directory.join("snapshots"))?;
        if bytes(&manager.registration.directory.join("schema"), 128, true)?
            != b"gateway-extension-manager/v1\n"
        {
            return Err(Error::InvalidStore);
        }
        Ok(manager)
    }
    fn acquire(registration: Registration) -> Result<Self> {
        if !registration.driver.supported() {
            return Err(Error::Unsupported);
        }
        filesystem::directory(&registration.directory)?;
        if registration.sources.len() > 128 || registration.recorder_bindings.len() > 128 {
            return Err(Error::InvalidInput);
        }
        for binding in registration.recorder_bindings.values() {
            if !matches!(
                binding.mode.as_str(),
                "off" | "best_effort" | "durable_local"
            ) || !(2..=4096).contains(&binding.queue_capacity)
                || !(1..=60000).contains(&binding.ack_timeout_ms)
            {
                return Err(Error::InvalidInput);
            }
        }
        let owner = filesystem::lease(&registration.directory.join("manager.owner"))?;
        let inventory = registration.driver.inventory(&registration.store)?;
        let registration_sha256 = Digest::of(&encode(&(
            &registration.target,
            &registration.store,
            &registration.driver,
            &registration.sources,
            &registration.recorder_bindings,
        ))?);
        Ok(Self {
            registration,
            registration_sha256,
            _owner: owner,
            epoch: fresh(),
            last_inventory: inventory.inventory_sha256,
        })
    }
    pub fn supported_operations(&self) -> Vec<Action> {
        let mut actions = vec![
            Action::PackageInstall,
            Action::PackageEnable,
            Action::PackageDisable,
        ];
        if matches!(self.registration.driver, Driver::Native(_)) {
            actions.push(Action::PackageSelect);
        }
        actions
    }
    fn snapshot_for(&self, inventory: &Inventory) -> Result<Snapshot> {
        let digest = Digest::of(&encode(
            &json!({"registration":self.registration_sha256,"epoch":self.epoch,"inventory":inventory.inventory_sha256,"generation":inventory.generation}),
        )?);
        Ok(Snapshot {
            revision: inventory.generation,
            digest,
        })
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.snapshot_for(
            &self
                .registration
                .driver
                .inventory(&self.registration.store)?,
        )
    }
    pub fn status(&self, effective: Option<EffectiveSelection>) -> Result<Status> {
        if effective.as_ref().is_some_and(|e| e.packages.len() > 128) {
            return Err(Error::InvalidInput);
        }
        let inventory = self
            .registration
            .driver
            .inventory(&self.registration.store)?;
        Ok(Status {
            schema: "gateway-extension-status/v1",
            target: self.registration.target.clone(),
            snapshot: self.snapshot_for(&inventory)?,
            external_change: inventory.inventory_sha256 != self.last_inventory,
            store: inventory,
            effective,
            removal_supported: false,
            version_change_requires_disable: matches!(
                self.registration.driver,
                Driver::ProfilePack
            ),
        })
    }
    pub fn inspect_source(&self, id: &Id) -> Result<Value> {
        self.registration
            .driver
            .inspect(self.registration.sources.get(id).ok_or(Error::NotFound)?)
    }
    pub fn bind(&mut self, command: Command) -> Bound<'_> {
        Bound {
            manager: self,
            command,
        }
    }
    fn validate(&self, command: &Command, inventory: &Inventory) -> Result<()> {
        match command {
            Command::Install { source } => {
                self.inspect_source(source)?;
            }
            Command::Enable {
                package,
                grants,
                recorder,
            }
            | Command::Select {
                package,
                grants,
                recorder,
            } => {
                if grants.len() > 8 || grants.windows(2).any(|pair| pair[0] >= pair[1]) {
                    return Err(Error::InvalidInput);
                }
                let installed = inventory.inventory["installed"]
                    .as_array()
                    .ok_or(Error::InvalidStore)?
                    .iter()
                    .find(|item| {
                        item["id"] == package.id.as_str()
                            && item["version"] == package.version
                            && item["package_sha256"] == package.package_sha256.as_str()
                    })
                    .ok_or(Error::NotFound)?;
                if installed["verified"] != true {
                    return Err(Error::Conflict);
                }
                match &self.registration.driver {
                    Driver::Native(_) => {
                        let active = inventory.inventory["activation"]["extensions"]
                            .as_array()
                            .ok_or(Error::InvalidStore)?
                            .iter()
                            .find(|entry| entry["id"] == package.id.as_str());
                        match (command, active) {
                            (Command::Select { .. }, None) => return Err(Error::NotFound),
                            (Command::Enable { .. }, Some(entry))
                                if entry["version"] != package.version
                                    || entry["package_sha256"]
                                        != package.package_sha256.as_str() =>
                            {
                                return Err(Error::Conflict);
                            }
                            _ => {}
                        }
                        if installed["package"]["permissions"] != json!(grants) {
                            return Err(Error::Forbidden);
                        }
                        let is_recorder =
                            installed["package"]["protocol"] == "gateway-usage-recorder/v1";
                        if is_recorder != recorder.is_some() {
                            return Err(Error::InvalidInput);
                        }
                        if recorder
                            .as_ref()
                            .is_some_and(|id| !self.registration.recorder_bindings.contains_key(id))
                        {
                            return Err(Error::NotFound);
                        }
                    }
                    Driver::ProfilePack => {
                        if matches!(command, Command::Select { .. }) {
                            return Err(Error::Unsupported);
                        }
                        if !grants.is_empty() || recorder.is_some() {
                            return Err(Error::InvalidInput);
                        }
                        if inventory.inventory["activation"]["packs"]
                            .as_array()
                            .ok_or(Error::InvalidStore)?
                            .iter()
                            .any(|p| {
                                p["id"] == package.id.as_str()
                                    && (p["version"] != package.version
                                        || p["package_sha256"] != package.package_sha256.as_str())
                            })
                        {
                            return Err(Error::Conflict);
                        }
                    }
                }
            }
            Command::Disable { package } => {
                let key = if matches!(self.registration.driver, Driver::Native(_)) {
                    "extensions"
                } else {
                    "packs"
                };
                if !inventory.inventory["activation"][key]
                    .as_array()
                    .ok_or(Error::InvalidStore)?
                    .iter()
                    .any(|p| p["id"] == package.as_str())
                {
                    return Err(Error::NotFound);
                }
            }
        }
        Ok(())
    }
    fn receipt_path(&self, id: &Id) -> PathBuf {
        self.registration.directory.join(format!(
            "{}.json",
            Digest::of(id.as_str().as_bytes()).as_str()
        ))
    }
    fn retain_inventory(&self, inventory: &Inventory) -> Result<()> {
        let mut raw = encode(&inventory.inventory)?;
        raw.push(b'\n');
        if Digest::of(&raw) != inventory.inventory_sha256 {
            return Err(Error::InvalidStore);
        }
        let path = self
            .registration
            .directory
            .join("snapshots")
            .join(format!("{}.json", inventory.inventory_sha256.as_str()));
        if path.symlink_metadata().is_ok() {
            if bytes(&path, 2 * 1024 * 1024, true)? != raw {
                return Err(Error::InvalidStore);
            }
            Ok(())
        } else {
            write_new(&path, &raw)
        }
    }
}
fn write_new(path: &Path, value: &[u8]) -> Result<()> {
    filesystem::directory(path.parent().ok_or(Error::InvalidStore)?)?;
    let mut file = filesystem::private_new(path)?;
    file.write_all(value)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::Storage)?;
    #[cfg(unix)]
    File::open(path.parent().ok_or(Error::InvalidStore)?)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::Storage)?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    operation: Id,
    request_sha256: Digest,
    after: Snapshot,
    registration_sha256: Digest,
    epoch: Id,
    inventory_sha256: Digest,
    result_sha256: Digest,
    before_inventory_sha256: Digest,
    before_epoch: Id,
}
pub struct Bound<'a> {
    manager: &'a mut Manager,
    command: Command,
}
struct Prepared<'a> {
    manager: &'a mut Manager,
    command: Command,
    request: Request,
    before: Snapshot,
    inventory: Inventory,
    operation: Option<Id>,
}
impl Backend for Bound<'_> {
    fn prepare<'a>(&'a mut self, request: &'a Request) -> Result<Box<dyn PreparedOperation + 'a>> {
        if request.target != self.manager.registration.target
            || request.action != self.command.action()
            || request.parameters_sha256 != self.command.digest()?
        {
            return Err(Error::InvalidInput);
        }
        let inventory = self
            .manager
            .registration
            .driver
            .inventory(&self.manager.registration.store)?;
        self.manager.validate(&self.command, &inventory)?;
        let before = self.manager.snapshot_for(&inventory)?;
        Ok(Box::new(Prepared {
            manager: self.manager,
            command: self.command.clone(),
            request: request.clone(),
            before,
            inventory,
            operation: None,
        }))
    }
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect> {
        if operation.request.target != self.manager.registration.target {
            return Err(Error::NotFound);
        }
        let raw = match bytes(&self.manager.receipt_path(&operation.id), 4096, true) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Ok(Effect::Uncertain {
                    code: FailureCode::Unverified,
                });
            }
        };
        let receipt: Receipt = serde_json::from_slice(&raw).map_err(|_| Error::InvalidStore)?;
        let expected = Digest::of(&encode(
            &json!({"registration":receipt.registration_sha256,"epoch":receipt.epoch,"inventory":receipt.inventory_sha256,"generation":receipt.after.revision}),
        )?);
        let expected_before = Digest::of(&encode(
            &json!({"registration":receipt.registration_sha256,"epoch":receipt.before_epoch,"inventory":receipt.before_inventory_sha256,"generation":operation.request.expected.revision}),
        )?);
        if encode(&receipt)? != raw
            || receipt.schema != "gateway-package-evidence/v1"
            || receipt.operation != operation.id
            || receipt.epoch != operation.id
            || receipt.request_sha256 != operation.request.fingerprint()?
            || receipt.after.digest != expected
            || operation.request.expected.digest != expected_before
        {
            return Err(Error::InvalidStore);
        }
        Ok(Effect::Applied {
            after: receipt.after,
            evidence_sha256: Digest::of(&raw),
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
        if self.manager.snapshot().as_ref() != Ok(&self.before) {
            return Effect::NotApplied {
                code: FailureCode::Stale,
            };
        }
        if self.manager.retain_inventory(&self.inventory).is_err() {
            return Effect::NotApplied {
                code: FailureCode::Unavailable,
            };
        }
        let source = match &self.command {
            Command::Install { source } => self.manager.registration.sources.get(source),
            _ => None,
        };
        let binding = match &self.command {
            Command::Enable { recorder, .. } | Command::Select { recorder, .. } => recorder
                .as_ref()
                .and_then(|id| self.manager.registration.recorder_bindings.get(id)),
            _ => None,
        };
        let result = self.manager.registration.driver.mutate(
            &self.manager.registration.store,
            &self.command,
            source,
            binding,
            &self.inventory,
        );
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(Error::Conflict) => {
                return Effect::NotApplied {
                    code: FailureCode::Stale,
                };
            }
            Err(_) => {
                return Effect::Uncertain {
                    code: FailureCode::EffectFailed,
                };
            }
        };
        let before_epoch = self.manager.epoch.clone();
        self.manager.epoch = operation.clone();
        let result = (|| {
            self.manager.retain_inventory(&outcome.after)?;
            let after = self.manager.snapshot_for(&outcome.after)?;
            let receipt = Receipt {
                schema: "gateway-package-evidence/v1".into(),
                operation: operation.clone(),
                request_sha256: self.request.fingerprint()?,
                after: after.clone(),
                registration_sha256: self.manager.registration_sha256.clone(),
                epoch: operation.clone(),
                inventory_sha256: outcome.after.inventory_sha256.clone(),
                result_sha256: Digest::of(&encode(&outcome.result)?),
                before_inventory_sha256: self.inventory.inventory_sha256.clone(),
                before_epoch,
            };
            let raw = encode(&receipt)?;
            write_new(&self.manager.receipt_path(&operation), &raw)?;
            self.manager.last_inventory = outcome.after.inventory_sha256;
            Ok::<_, Error>(Effect::Applied {
                after,
                evidence_sha256: Digest::of(&raw),
            })
        })();
        result.unwrap_or(Effect::Uncertain {
            code: FailureCode::EffectFailed,
        })
    }
}
