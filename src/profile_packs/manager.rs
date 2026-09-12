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
    match command {
        Command::Package { source, output } => {
            let raw = filesystem::read(&source, MAX_PACKAGE)?;
            // Reject duplicate keys even in the intentionally noncanonical source form.
            let value = crate::adapters::json::decode(&raw).map_err(|_| invalid())?;
            let package: Package = serde_json::from_value(value).map_err(|_| invalid())?;
            package.validate()?;
            let raw = canonical(&package)?;
            if raw.len() as u64 > MAX_PACKAGE {
                return Err(invalid());
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
            let entry = Entry {
                id: package.id,
                version: package.version,
                package_sha256: hash(&raw),
            };
            filesystem::directory(&store)?;
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
            Ok(json!({"installed":entry,"activation_changed":false,"executed":false}))
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
            let mut activation = activation(&store)?;
            load_package(&entry.path(&store), Some(&entry))?;
            if let Some(existing) = activation.packs.iter().find(|p| p.id == entry.id) {
                if existing.version == entry.version
                    && existing.package_sha256 == entry.package_sha256
                {
                    return Ok(json!({"activation":activation,"changed":false}));
                }
                // Replacement must be explicit: disable, then enable the exact new binding.
                return Err(ConfigError(
                    "Profile pack already enabled; disable it before selecting another binding"
                        .into(),
                ));
            }
            activation.packs.push(entry);
            activation.packs.sort_by(|a, b| a.id.cmp(&b.id));
            commit(&store, activation)
        }
        Command::Disable { store, id } => {
            if !identifier(&id) {
                return Err(invalid());
            }
            let _writer = filesystem::Writer::acquire(&store)?;
            // Do not require a damaged active package to be readable before disabling it.
            let mut activation: Activation =
                decode(&filesystem::read(&store.join("active.json"), MAX_LOCK)?)?;
            activation.validate()?;
            let before = activation.packs.len();
            activation.packs.retain(|entry| entry.id != id);
            if activation.packs.len() == before {
                return Err(invalid());
            }
            for entry in &activation.packs {
                load_package(&entry.path(&store), Some(entry))?;
            }
            commit(&store, activation)
        }
        Command::Status { store } => {
            let plan = ProfilePackPlan::load(&store.join("active.json"))?;
            Ok(json!({"activation":plan.activation,"verified":true,"executed":false}))
        }
    }
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
