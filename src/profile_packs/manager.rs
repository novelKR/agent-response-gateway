//! Offline package lifecycle. Uses the same validation and canonical bytes as startup.
use super::*;
use clap::Subcommand;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum Command {
    /// Validate source JSON and create a canonical single-file data package.
    Package {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Inspect a canonical package without installing or executing anything.
    Inspect {
        #[arg(long)]
        package: PathBuf,
    },
    /// Install immutable bytes into a local store; does not enable the package.
    Install {
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        store: PathBuf,
    },
    /// Enable an exact installed package; no version selection or download.
    Enable {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        sha256: String,
    },
    /// Remove one activation binding; retain installed package bytes.
    Disable {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        id: String,
    },
    /// Validate and report the active snapshot without reading host configuration.
    Status {
        #[arg(long)]
        store: PathBuf,
    },
}

pub fn run(command: Command) -> Result<Value, ConfigError> {
    execute(command, None, None).map_err(|error| match error {
        GuardedError::Conflict => ConfigError("Profile pack inventory changed".into()),
        GuardedError::Invalid(error) => error,
    })
}

/// A trusted host may add an exact store condition without changing CLI semantics.
pub fn run_guarded(
    command: Command,
    expected: &Precondition,
    package_sha256: Option<&str>,
) -> Result<Value, GuardedError> {
    if !matches!(
        command,
        Command::Install { .. } | Command::Enable { .. } | Command::Disable { .. }
    ) {
        return Err(invalid().into());
    }
    if matches!(command, Command::Install { .. }) != package_sha256.is_some()
        || package_sha256.is_some_and(|sha| !digest(sha))
    {
        return Err(invalid().into());
    }
    execute(command, Some(expected), package_sha256)
}

#[derive(Debug)]
pub enum GuardedError {
    Conflict,
    Invalid(ConfigError),
}
impl From<ConfigError> for GuardedError {
    fn from(error: ConfigError) -> Self {
        Self::Invalid(error)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Precondition {
    pub generation: u64,
    pub inventory_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub generation: u64,
    pub inventory_sha256: String,
    pub inventory: Value,
}

/// Read installed and selected data under the same writer marker as mutations.
/// The caller supplies an existing store; this never initializes one.
pub fn inventory(store: &Path) -> Result<Inventory, ConfigError> {
    let _writer = filesystem::Writer::acquire(store)?;
    inventory_unlocked(store)
}

fn check_expected(store: &Path, expected: Option<&Precondition>) -> Result<(), GuardedError> {
    if let Some(expected) = expected {
        if !digest(&expected.inventory_sha256) {
            return Err(invalid().into());
        }
        let actual = inventory_unlocked(store)?;
        if actual.generation != expected.generation
            || actual.inventory_sha256 != expected.inventory_sha256
        {
            return Err(GuardedError::Conflict);
        }
    }
    Ok(())
}

fn guarded_result(
    store: &Path,
    expected: Option<&Precondition>,
    result: Value,
) -> Result<Value, GuardedError> {
    if expected.is_some() {
        return Ok(json!({"result":result,"after":inventory_unlocked(store)?}));
    }
    Ok(result)
}

fn execute(
    command: Command,
    expected: Option<&Precondition>,
    package_sha256: Option<&str>,
) -> Result<Value, GuardedError> {
    match command {
        Command::Package { source, output } => {
            let raw = filesystem::read(&source, MAX_PACKAGE)?;
            // Reject duplicate keys even in the intentionally noncanonical source form.
            let value = crate::adapters::json::decode(&raw).map_err(|_| invalid())?;
            let package: Package = serde_json::from_value(value).map_err(|_| invalid())?;
            package.validate()?;
            let raw = canonical(&package)?;
            if raw.len() as u64 > MAX_PACKAGE {
                return Err(invalid().into());
            }
            filesystem::create(&output, &raw)?;
            Ok(
                json!({"id":package.id,"version":package.version,"package_sha256":hash(&raw),"executed":false}),
            )
        }
        Command::Inspect { package } => {
            let package = load_package(&package, None)?;
            Ok(
                json!({"package_sha256":hash(&canonical(&package)?),"package":package,"executed":false,"evidence_status":"publisher_claims_not_attestation"}),
            )
        }
        Command::Install { package, store } => {
            let package = load_package(&package, None)?;
            let raw = canonical(&package)?;
            if package_sha256.is_some_and(|expected| expected != hash(&raw)) {
                return Err(GuardedError::Conflict);
            }
            let entry = Entry {
                id: package.id,
                version: package.version,
                package_sha256: hash(&raw),
            };
            if expected.is_none() {
                filesystem::directory(&store)?;
            }
            let _writer = filesystem::Writer::acquire(&store)?;
            check_expected(&store, expected)?;
            for dir in [
                store.join("packages"),
                store.join("packages").join(&entry.id),
                store.join("packages").join(&entry.id).join(&entry.version),
            ] {
                filesystem::directory(&dir)?;
            }
            let path = entry.path(&store);
            if path.exists() {
                load_package(&path, Some(&entry))?;
            } else {
                filesystem::create(&path, &raw)?;
            }
            guarded_result(
                &store,
                expected,
                json!({"installed":entry,"activation_changed":false,"executed":false}),
            )
        }
        Command::Enable {
            store,
            id,
            version,
            sha256,
        } => {
            let entry = Entry {
                id,
                version,
                package_sha256: sha256,
            };
            entry.validate()?;
            let _writer = filesystem::Writer::acquire(&store)?;
            check_expected(&store, expected)?;
            let mut activation = activation(&store)?;
            load_package(&entry.path(&store), Some(&entry))?;
            if let Some(existing) = activation.packs.iter().find(|p| p.id == entry.id) {
                if existing.version == entry.version
                    && existing.package_sha256 == entry.package_sha256
                {
                    return guarded_result(
                        &store,
                        expected,
                        json!({"activation":activation,"changed":false}),
                    );
                }
                // Replacement must be explicit: disable, then enable the exact new binding.
                return Err(ConfigError(
                    "Profile pack already enabled; disable it before selecting another binding"
                        .into(),
                )
                .into());
            }
            activation.packs.push(entry);
            activation.packs.sort_by(|a, b| a.id.cmp(&b.id));
            let result = commit(&store, activation)?;
            guarded_result(&store, expected, result)
        }
        Command::Disable { store, id } => {
            if !identifier(&id) {
                return Err(invalid().into());
            }
            let _writer = filesystem::Writer::acquire(&store)?;
            check_expected(&store, expected)?;
            // Do not require a damaged active package to be readable before disabling it.
            let mut activation: Activation =
                decode(&filesystem::read(&store.join("active.json"), MAX_LOCK)?)?;
            activation.validate()?;
            let before = activation.packs.len();
            activation.packs.retain(|entry| entry.id != id);
            if activation.packs.len() == before {
                return Err(invalid().into());
            }
            for entry in &activation.packs {
                load_package(&entry.path(&store), Some(entry))?;
            }
            let result = commit(&store, activation)?;
            guarded_result(&store, expected, result)
        }
        Command::Status { store } => {
            let plan = ProfilePackPlan::load(&store.join("active.json"))?;
            Ok(json!({"activation":plan.activation,"verified":true,"executed":false}))
        }
    }
}

fn inventory_unlocked(store: &Path) -> Result<Inventory, ConfigError> {
    fn entries(path: &Path, budget: &mut usize) -> Result<Vec<PathBuf>, ConfigError> {
        filesystem::no_links(path)?;
        let mut paths = vec![];
        for entry in std::fs::read_dir(path).map_err(|_| invalid())? {
            *budget = budget.checked_sub(1).ok_or_else(invalid)?;
            paths.push(entry.map_err(|_| invalid())?.path());
        }
        paths.sort();
        Ok(paths)
    }
    fn name(path: &Path) -> Result<&str, ConfigError> {
        path.file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(invalid)
    }
    let active = store.join("active.json");
    let activation: Activation = match active.symlink_metadata() {
        Ok(_) => decode(&filesystem::read(&active, MAX_LOCK)?)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Activation {
            schema: LOCK_SCHEMA.into(),
            generation: 0,
            packs: vec![],
        },
        Err(_) => return Err(invalid()),
    };
    activation.validate()?;
    let mut installed = vec![];
    let mut budget = 1024;
    let packages = store.join("packages");
    match packages.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(invalid()),
        Ok(_) => {
            for identity in entries(&packages, &mut budget)? {
                let id = name(&identity)?;
                if !identifier(id) {
                    return Err(invalid());
                }
                for release in entries(&identity, &mut budget)? {
                    let version = name(&release)?;
                    if !super::version(version) {
                        return Err(invalid());
                    }
                    for package in entries(&release, &mut budget)? {
                        let sha = name(&package)?
                            .strip_suffix(".json")
                            .filter(|s| digest(s))
                            .ok_or_else(invalid)?;
                        if installed.len() >= 128 {
                            return Err(invalid());
                        }
                        let entry = Entry {
                            id: id.into(),
                            version: version.into(),
                            package_sha256: sha.into(),
                        };
                        let validated=load_package(&package,Some(&entry)).ok().map(|p| json!({
                        "schema":p.schema,"capabilities":p.capabilities.keys().collect::<Vec<_>>(),
                        "policies":p.policies.keys().collect::<Vec<_>>(),"editing_policies":p.editing_policies.keys().collect::<Vec<_>>()
                    }));
                        installed.push(json!({"id":id,"version":version,"package_sha256":sha,"verified":validated.is_some(),"package":validated}));
                    }
                }
            }
        }
    }
    let generation = activation.generation;
    let value = json!({"schema":"gateway-profile-inventory/v1","activation":activation,"installed":installed,"runtime_checked":false,"removal_supported":false});
    Ok(Inventory {
        generation,
        inventory_sha256: hash(&canonical(&value)?),
        inventory: value,
    })
}

fn activation(store: &Path) -> Result<Activation, ConfigError> {
    let path = store.join("active.json");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(ProfilePackPlan::load(&path)?.activation),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Activation {
            schema: LOCK_SCHEMA.into(),
            generation: 0,
            packs: vec![],
        }),
        Err(_) => Err(invalid()),
    }
}

fn commit(store: &Path, mut activation: Activation) -> Result<Value, ConfigError> {
    activation.generation = activation.generation.checked_add(1).ok_or_else(invalid)?;
    activation.validate()?;
    let raw = canonical(&activation)?;
    if raw.len() as u64 > MAX_LOCK {
        return Err(invalid());
    }
    filesystem::replace_activation(store, &raw)?;
    Ok(json!({"activation":activation,"changed":true,"restart_required":true}))
}
