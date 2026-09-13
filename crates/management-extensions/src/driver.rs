use crate::{Command, LocalSource, RecorderBinding, Selection, bytes, encode};
use agent_response_gateway::profile_packs::manager::{self, Command as PackCommand};
use gateway_management::{Digest, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize)]
pub struct NativeDriver {
    pub python: PathBuf,
    pub python_sha256: Digest,
    pub manager: PathBuf,
    pub manager_sha256: Digest,
    pub timeout_ms: u64,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Driver {
    Native(NativeDriver),
    ProfilePack,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub generation: u64,
    pub inventory_sha256: Digest,
    pub inventory: Value,
}
impl Inventory {
    pub(crate) fn validate(&self, driver: &Driver) -> Result<()> {
        let mut canonical = encode(&self.inventory)?;
        canonical.push(b'\n');
        if Digest::of(&canonical) != self.inventory_sha256
            || self.inventory["activation"]["generation"].as_u64() != Some(self.generation)
            || self.inventory["schema"] != driver.schema()
            || self.inventory["runtime_checked"] != false
            || self.inventory["removal_supported"] != false
            || self.inventory["installed"]
                .as_array()
                .is_none_or(|v| v.len() > 128)
        {
            return Err(Error::InvalidStore);
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Outcome {
    pub result: Value,
    pub after: Inventory,
}
impl NativeDriver {
    fn run(&self, arguments: Vec<OsString>) -> Result<Value> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(Error::Unsupported);
        }
        if !(100..=60000).contains(&self.timeout_ms)
            || Digest::of(&bytes(&self.python, 256 * 1024 * 1024, false)?) != self.python_sha256
            || Digest::of(&bytes(&self.manager, 1024 * 1024, false)?) != self.manager_sha256
        {
            return Err(Error::InvalidInput);
        }
        let mut child = Process::new(&self.python)
            .args(["-I", "-B"])
            .arg(&self.manager)
            .args(arguments)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Storage)?;
        let stdout = child.stdout.take().ok_or(Error::Storage)?;
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = std::thread::Builder::new()
            .name("native-package-result".into())
            .spawn(move || {
                let mut data = vec![];
                let result = stdout
                    .take(2 * 1024 * 1024 + 1)
                    .read_to_end(&mut data)
                    .map(|_| data);
                let _ = tx.send(result);
            });
        if reader.is_err() {
            stop_helper(&mut child);
            return Err(Error::Storage);
        }
        let deadline = Instant::now() + Duration::from_millis(self.timeout_ms);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    stop_helper(&mut child);
                    return Err(Error::Storage);
                }
            }
        };
        if status.code() == Some(3) {
            return Err(Error::Conflict);
        }
        if !status.success() {
            return Err(Error::InvalidInput);
        }
        let raw = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| Error::Storage)?
            .map_err(|_| Error::Storage)?;
        if raw.len() > 2 * 1024 * 1024 {
            return Err(Error::InvalidStore);
        }
        let value: Value = serde_json::from_slice(&raw).map_err(|_| Error::InvalidStore)?;
        // Driver output is canonical ASCII JSON. This also rejects duplicate keys.
        let mut canonical = encode(&value)?;
        canonical.push(b'\n');
        if raw != canonical {
            return Err(Error::InvalidStore);
        }
        Ok(value)
    }
}
fn stop_helper(child: &mut std::process::Child) {
    let _ = child.kill();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Unconfirmed helper termination is never returned as a successful operation.
}
impl Driver {
    pub fn supported(&self) -> bool {
        matches!(self, Self::ProfilePack) || cfg!(any(target_os = "linux", target_os = "macos"))
    }
    pub(crate) fn schema(&self) -> &'static str {
        match self {
            Self::Native(_) => "gateway-native-inventory/v1",
            Self::ProfilePack => "gateway-profile-inventory/v1",
        }
    }
    pub(crate) fn inventory(&self, store: &Path) -> Result<Inventory> {
        let value = match self {
            Self::Native(native) => {
                native.run(vec!["inventory".into(), "--store".into(), store.into()])?
            }
            Self::ProfilePack => {
                serde_json::to_value(manager::inventory(store).map_err(|_| Error::InvalidStore)?)
                    .map_err(|_| Error::InvalidStore)?
            }
        };
        let inventory: Inventory =
            serde_json::from_value(value).map_err(|_| Error::InvalidStore)?;
        inventory.validate(self)?;
        Ok(inventory)
    }
    pub(crate) fn inspect(&self, source: &LocalSource) -> Result<Value> {
        match self {
            Self::Native(native) => {
                let value = native.run(vec![
                    "inspect".into(),
                    "--package".into(),
                    source.path.clone().into(),
                    "--expected-sha256".into(),
                    source.package_sha256.as_str().into(),
                ])?;
                Ok(value["package"].clone())
            }
            Self::ProfilePack => {
                if Digest::of(&bytes(&source.path, 262144, false)?) != source.package_sha256 {
                    return Err(Error::Conflict);
                }
                let value = manager::run(PackCommand::Inspect {
                    package: source.path.clone(),
                })
                .map_err(|_| Error::InvalidInput)?;
                if value["package_sha256"] != source.package_sha256.as_str() {
                    return Err(Error::Conflict);
                }
                Ok(
                    json!({"id":value["package"]["id"],"version":value["package"]["version"],"schema":value["package"]["schema"]}),
                )
            }
        }
    }
    pub(crate) fn mutate(
        &self,
        store: &Path,
        command: &Command,
        source: Option<&LocalSource>,
        binding: Option<&RecorderBinding>,
        before: &Inventory,
    ) -> Result<Outcome> {
        let value = match self {
            Self::Native(native) => {
                let mut args: Vec<OsString> = vec![];
                match command {
                    Command::Install { .. } => {
                        let source = source.ok_or(Error::NotFound)?;
                        args.extend([
                            "install".into(),
                            "--package".into(),
                            source.path.clone().into(),
                            "--expected-sha256".into(),
                            source.package_sha256.as_str().into(),
                        ]);
                    }
                    Command::Enable {
                        package, grants, ..
                    }
                    | Command::Select {
                        package, grants, ..
                    } => {
                        args.push("enable".into());
                        append_selection(&mut args, package);
                        for grant in grants {
                            args.extend(["--grant".into(), grant.into()]);
                        }
                        if let Some(binding) = binding {
                            args.extend([
                                "--recorder-binding-json".into(),
                                String::from_utf8(encode(binding)?)
                                    .map_err(|_| Error::InvalidInput)?
                                    .into(),
                            ]);
                        }
                    }
                    Command::Disable { package } => {
                        args.extend(["disable".into(), "--id".into(), package.as_str().into()])
                    }
                }
                args.extend([
                    "--store".into(),
                    store.into(),
                    "--expected-generation".into(),
                    before.generation.to_string().into(),
                    "--expected-inventory-sha256".into(),
                    before.inventory_sha256.as_str().into(),
                ]);
                native.run(args)?
            }
            Self::ProfilePack => {
                let (command, sha) = match command {
                    Command::Install { .. } => {
                        let source = source.ok_or(Error::NotFound)?;
                        (
                            PackCommand::Install {
                                package: source.path.clone(),
                                store: store.into(),
                            },
                            Some(source.package_sha256.as_str()),
                        )
                    }
                    Command::Enable { package, .. } => (
                        PackCommand::Enable {
                            store: store.into(),
                            id: package.id.as_str().into(),
                            version: package.version.clone(),
                            sha256: package.package_sha256.as_str().into(),
                        },
                        None,
                    ),
                    Command::Disable { package } => (
                        PackCommand::Disable {
                            store: store.into(),
                            id: package.as_str().into(),
                        },
                        None,
                    ),
                    Command::Select { .. } => return Err(Error::Unsupported),
                };
                manager::run_guarded(
                    command,
                    &manager::Precondition {
                        generation: before.generation,
                        inventory_sha256: before.inventory_sha256.as_str().into(),
                    },
                    sha,
                )
                .map_err(|e| match e {
                    manager::GuardedError::Conflict => Error::Conflict,
                    manager::GuardedError::Invalid(_) => Error::InvalidInput,
                })?
            }
        };
        let outcome: Outcome = serde_json::from_value(value).map_err(|_| Error::InvalidStore)?;
        outcome.after.validate(self)?;
        Ok(outcome)
    }
}
fn append_selection(args: &mut Vec<OsString>, package: &Selection) {
    args.extend([
        "--id".into(),
        package.id.as_str().into(),
        "--version".into(),
        package.version.clone().into(),
        "--package-sha256".into(),
        package.package_sha256.as_str().into(),
    ]);
}
