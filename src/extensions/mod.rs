//! Opt-in, versioned observers. No credential, request-body or routing hooks.
#![cfg_attr(not(unix), allow(dead_code))]
use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ConfigError;

#[cfg(unix)]
mod filesystem;
mod runtime;
pub use runtime::{ExtensionRuntime, ObserverSink};

pub const PACKAGE_SCHEMA: &str = "gateway-extension-package/v1";
pub const LOCK_SCHEMA: &str = "gateway-extension-lock/v1";
pub const OBSERVER_PROTOCOL: &str = "gateway-observer/v1";
pub const EXTENDED_MANIFEST_SCHEMA: &str = "gateway-extended-manifest/v1";
pub const EXTENDED_READY_SCHEMA: &str = "gateway-extended-ready/v1";
pub const PERMISSIONS: [&str; 2] = ["observe_http_metadata", "write_private_state"];
const MAX_JSON: u64 = 65_536;
const MAX_BINARY: u64 = 128 * 1024 * 1024;
const MAX_NOTICE: u64 = 256 * 1024;
const MAX_EXTENSIONS: usize = 4;

fn invalid() -> ConfigError {
    ConfigError("Invalid, incompatible or unverified extension configuration".into())
}

fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, ConfigError> {
    // Value's map is ordered; never hash struct field declaration order.
    let value = serde_json::to_value(value).map_err(|_| invalid())?;
    serde_json::to_vec(&value).map_err(|_| invalid())
}

fn decode<T: serde::de::DeserializeOwned + Serialize>(raw: &[u8]) -> Result<T, ConfigError> {
    let value: T = serde_json::from_slice(raw).map_err(|_| invalid())?;
    let mut expected = canonical(&value)?;
    expected.push(b'\n');
    // Also rejects duplicate map keys, noncanonical numeric forms and extra whitespace.
    if expected != raw {
        return Err(invalid());
    }
    Ok(value)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 6
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|b| b.is_ascii_digit())
        })
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn permissions(values: &[String]) -> bool {
    values.iter().map(String::as_str).eq(PERMISSIONS)
}

fn host_target() -> Result<&'static str, ConfigError> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("linux-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("macos", "x86_64") => Ok("macos-x64"),
        ("macos", "aarch64") => Ok("macos-arm64"),
        _ => Err(ConfigError(
            "Native extensions currently require supported Linux or macOS hosts".into(),
        )),
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Package {
    schema: String,
    id: String,
    version: String,
    target: String,
    protocol: String,
    permissions: Vec<String>,
    state_schema: String,
    files: BTreeMap<String, String>,
}

impl Package {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.schema != PACKAGE_SCHEMA
            || !valid_id(&self.id)
            || !valid_version(&self.version)
            || self.target != host_target()?
            || self.protocol != OBSERVER_PROTOCOL
            || !permissions(&self.permissions)
            || self.state_schema != "observer-state/v1"
            || !(2..=8).contains(&self.files.len())
            || !self.files.contains_key("extension")
            || !self.files.contains_key("LICENSE.txt")
        {
            return Err(invalid());
        }
        for (name, digest) in &self.files {
            if name.is_empty()
                || name.len() > 64
                || !name.as_bytes()[0].is_ascii_alphanumeric()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                || name == "extension.json"
                || !valid_hash(digest)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnabledExtension {
    id: String,
    version: String,
    package_sha256: String,
    grants: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    schema: String,
    generation: u64,
    extensions: Vec<EnabledExtension>,
}

impl Activation {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.schema != LOCK_SCHEMA || self.extensions.len() > MAX_EXTENSIONS {
            return Err(invalid());
        }
        let mut previous: Option<&str> = None;
        for entry in &self.extensions {
            if !valid_id(&entry.id)
                || !valid_version(&entry.version)
                || !valid_hash(&entry.package_sha256)
                || !permissions(&entry.grants)
                || previous.is_some_and(|id| id >= entry.id.as_str())
            {
                return Err(invalid());
            }
            previous = Some(&entry.id);
        }
        Ok(())
    }
}

/// Immutable startup projection. Loading checks local bytes, never starts code or probes a provider.
pub struct ExtensionPlan {
    root: PathBuf,
    activation: Activation,
    packages: Vec<Package>,
    configuration: Value,
    configuration_sha256: String,
    #[cfg(unix)]
    owner: u32,
}

impl ExtensionPlan {
    #[cfg(unix)]
    pub fn load(path: &std::path::Path) -> Result<Self, ConfigError> {
        host_target()?;
        filesystem::no_links(path)?;
        let root = path.parent().ok_or_else(invalid)?.to_path_buf();
        let owner = filesystem::private_dir(&root, None)?;
        let raw = filesystem::read(path, MAX_JSON, owner)?;
        let activation: Activation = decode(&raw)?;
        activation.validate()?;
        let mut packages = Vec::new();
        for entry in &activation.extensions {
            let directory = root
                .join("packages")
                .join(&entry.id)
                .join(&entry.version)
                .join(&entry.package_sha256);
            filesystem::private_dir(&directory, Some(owner))?;
            let raw = filesystem::read(&directory.join("extension.json"), MAX_JSON, owner)?;
            if hash(&raw) != entry.package_sha256 {
                return Err(invalid());
            }
            let package: Package = decode(&raw)?;
            package.validate()?;
            if package.id != entry.id || package.version != entry.version {
                return Err(invalid());
            }
            let mut names = std::collections::BTreeSet::new();
            for item in std::fs::read_dir(&directory).map_err(|_| invalid())? {
                let name = item.map_err(|_| invalid())?.file_name();
                names.insert(name.into_string().map_err(|_| invalid())?);
            }
            let mut expected: std::collections::BTreeSet<_> = package.files.keys().cloned().collect();
            expected.insert("extension.json".into());
            if names != expected {
                return Err(invalid());
            }
            for (name, expected) in &package.files {
                let maximum = if name == "extension" { MAX_BINARY } else { MAX_NOTICE };
                if hash(&filesystem::read(&directory.join(name), maximum, owner)?) != *expected {
                    return Err(invalid());
                }
            }
            filesystem::private_dir(
                &root.join("state").join(&entry.id).join(&entry.package_sha256),
                Some(owner),
            )?;
            packages.push(package);
        }
        let configuration = json!({"schema":"gateway-extension-configuration/v1", "store":root,
            "activation":activation, "packages":packages});
        let configuration_sha256 = hash(&canonical(&configuration)?);
        Ok(Self { root, activation, packages, configuration, configuration_sha256, owner })
    }

    #[cfg(not(unix))]
    pub fn load(_path: &std::path::Path) -> Result<Self, ConfigError> {
        Err(invalid())
    }

    pub fn configuration_sha256(&self) -> &str {
        &self.configuration_sha256
    }

    pub fn configuration(&self) -> &Value {
        &self.configuration
    }

    pub fn manifest(&self, base_manifest: &Value) -> Result<Value, ConfigError> {
        let configuration = json!({"gateway":base_manifest,"extensions":self.configuration});
        let execution_sha256 = hash(&canonical(&configuration)?);
        Ok(json!({"schema":EXTENDED_MANIFEST_SCHEMA, "configuration":configuration,
            "execution_sha256":execution_sha256}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_version_bounds() {
        assert!(valid_id("metadata-counter"));
        assert!(!valid_id("../escape"));
        assert!(!valid_id("User@example"));
        assert!(valid_version("1.2.3"));
        assert!(!valid_version("01.2.3"));
        assert!(!valid_version("1.2.3+latest"));
        assert!(!valid_hash(&"F".repeat(64)));
    }

    #[test]
    fn lock_is_strict_and_canonical() {
        let activation = Activation { schema: LOCK_SCHEMA.into(), generation: 1, extensions: vec![] };
        let mut raw = canonical(&activation).unwrap();
        raw.push(b'\n');
        let decoded: Activation = decode(&raw).unwrap();
        decoded.validate().unwrap();
        assert!(decode::<Activation>(b"{\"extensions\":[],\"generation\":1,\"generation\":2,\"schema\":\"gateway-extension-lock/v1\"}\n").is_err());
        assert!(decode::<Activation>(b"{}\n").is_err());
        assert!(decode::<Activation>(&raw[..raw.len() - 1]).is_err());
    }

    #[test]
    fn unknown_roles_and_duplicate_bindings_fail() {
        let entry = EnabledExtension { id:"observer".into(), version:"0.1.0".into(),
            package_sha256:"a".repeat(64), grants:PERMISSIONS.iter().map(|v| (*v).into()).collect() };
        let mut activation = Activation { schema:LOCK_SCHEMA.into(), generation:1, extensions:vec![entry.clone()] };
        activation.validate().unwrap();
        activation.extensions.push(entry);
        assert!(activation.validate().is_err());
        activation.extensions.pop();
        activation.extensions[0].grants.push("read_credentials".into());
        assert!(activation.validate().is_err());
    }
}
