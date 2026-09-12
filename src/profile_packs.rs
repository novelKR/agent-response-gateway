//! Offline, non-executable capability and compatibility declarations.
//! A digest proves selected bytes, never provider conformance or publisher trust.
use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Config, ConfigError, compatibility::CompatibilityPolicy, config::ModelProfile};

mod filesystem;
pub mod manager;

const PACKAGE_SCHEMA: &str = "gateway-profile-pack/v1";
const LOCK_SCHEMA: &str = "gateway-profile-pack-lock/v1";
const MAX_PACKAGE: u64 = 262_144;
const MAX_LOCK: u64 = 16_384;
const MAX_PACKS: usize = 16;

fn invalid() -> ConfigError {
    ConfigError("Invalid, conflicting or unverified profile pack configuration".into())
}

fn hash(raw: &[u8]) -> String {
    crate::continuation::hex(&crate::digest::sha256(raw))
}

fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, ConfigError> {
    let value = serde_json::to_value(value).map_err(|_| invalid())?;
    let mut bytes = serde_json::to_vec(&value).map_err(|_| invalid())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn decode<T: serde::de::DeserializeOwned + Serialize>(raw: &[u8]) -> Result<T, ConfigError> {
    let value: T = serde_json::from_slice(raw).map_err(|_| invalid())?;
    // Re-encoding also rejects duplicate map keys, omitted defaults and noncanonical JSON.
    if canonical(&value)? != raw {
        return Err(invalid());
    }
    Ok(value)
}

fn identifier(id: &str) -> bool {
    if matches!(id, "con" | "prn" | "aux" | "nul")
        || (id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && matches!(id.as_bytes()[3], b'1'..=b'9'))
    {
        return false;
    }
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && id.as_bytes()[0].is_ascii_lowercase()
}

fn version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 9
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn label(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 200
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_/.:".contains(&b))
}

fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CapabilityTemplate {
    api: crate::ir::ApiProtocol,
    reasoning_contract: Option<crate::ir::reasoning::ReasoningContract>,
    context_window: u64,
    max_output_tokens: u64,
    tested_codex_version: String,
    support: BTreeMap<crate::ir::capability::Feature, crate::config::DeclaredSupport>,
}

impl CapabilityTemplate {
    fn bind(&self, version: &str, provider: &str, model: &str) -> ModelProfile {
        ModelProfile {
            version: version.into(),
            provider: provider.into(),
            upstream_model: model.into(),
            api: self.api,
            reasoning_contract: self.reasoning_contract.clone(),
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
            tested_codex_version: self.tested_codex_version.clone(),
            support: self.support.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    description: String,
    source_url: Option<String>,
    artifact_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Package {
    schema: String,
    id: String,
    version: String,
    capabilities: BTreeMap<String, CapabilityTemplate>,
    policies: BTreeMap<String, CompatibilityPolicy>,
    evidence: Vec<Evidence>,
    /// Original notice text, kept inside the single data file; never interpreted as paths.
    notices: BTreeMap<String, String>,
}

impl Package {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.schema != PACKAGE_SCHEMA
            || !identifier(&self.id)
            || !version(&self.version)
            || self.capabilities.len() + self.policies.len() == 0
            || self.capabilities.len() + self.policies.len() > 64
            || self.evidence.len() > 16
            || !self.notices.contains_key("LICENSE")
            || self.notices.len() > 16
        {
            return Err(invalid());
        }
        for (name, text) in &self.notices {
            if !matches!(name.as_str(), "LICENSE" | "NOTICE")
                || text.is_empty()
                || text.len() > 131_072
                || text.contains('\0')
            {
                return Err(invalid());
            }
        }
        for (id, template) in &self.capabilities {
            if !identifier(id)
                || !label(&template.tested_codex_version)
                || template.context_window == 0
                || template.max_output_tokens == 0
                || template.max_output_tokens > template.context_window
                || template
                    .bind(&self.version, "host", "host")
                    .capabilities(id)
                    .validate()
                    .is_err()
            {
                return Err(invalid());
            }
        }
        for (id, policy) in &self.policies {
            if !identifier(id) {
                return Err(invalid());
            }
            policy.validate()?;
        }
        for evidence in &self.evidence {
            if evidence.description.is_empty()
                || evidence.description.len() > 2048
                || evidence.description.chars().any(char::is_control)
                || evidence
                    .artifact_sha256
                    .as_ref()
                    .is_some_and(|v| !digest(v))
            {
                return Err(invalid());
            }
            if let Some(source) = &evidence.source_url {
                let url = url::Url::parse(source).map_err(|_| invalid())?;
                if source.len() > 2048
                    || url.scheme() != "https"
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    version: String,
    package_sha256: String,
}

impl Entry {
    fn validate(&self) -> Result<(), ConfigError> {
        if !identifier(&self.id) || !version(&self.version) || !digest(&self.package_sha256) {
            return Err(invalid());
        }
        Ok(())
    }
    fn path(&self, root: &Path) -> std::path::PathBuf {
        root.join("packages")
            .join(&self.id)
            .join(&self.version)
            .join(format!("{}.json", self.package_sha256))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    schema: String,
    generation: u64,
    packs: Vec<Entry>,
}

impl Activation {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.schema != LOCK_SCHEMA || self.packs.len() > MAX_PACKS {
            return Err(invalid());
        }
        let mut previous: Option<&str> = None;
        for entry in &self.packs {
            entry.validate()?;
            if previous.is_some_and(|id| id >= entry.id.as_str()) {
                return Err(invalid());
            }
            previous = Some(&entry.id);
        }
        Ok(())
    }
}

/// Fully verified, frozen package bytes. No file is reread during request processing.
#[derive(Clone, Debug)]
pub struct ProfilePackPlan {
    activation: Activation,
    packages: BTreeMap<String, Package>,
}

impl ProfilePackPlan {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let activation: Activation = decode(&filesystem::read(path, MAX_LOCK)?)?;
        activation.validate()?;
        let root = path.parent().ok_or_else(invalid)?;
        let mut packages = BTreeMap::new();
        for entry in &activation.packs {
            let package = load_package(&entry.path(root), Some(entry))?;
            packages.insert(entry.id.clone(), package);
        }
        Ok(Self {
            activation,
            packages,
        })
    }

    fn projection(&self) -> Value {
        json!({"schema":"gateway-profile-pack-configuration/v1", "activation":self.activation,
            "packages":self.packages, "evidence_status":"publisher_claims_not_attestation"})
    }

    fn selected(&self, pack: &str, export: &str) -> Result<&Package, ConfigError> {
        if !identifier(pack) || !identifier(export) {
            return Err(invalid());
        }
        self.packages.get(pack).ok_or_else(invalid)
    }

    fn entry(&self, pack: &str, export: &str) -> Value {
        let entry = self
            .activation
            .packs
            .iter()
            .find(|entry| entry.id == pack)
            .expect("resolved pack");
        json!({"pack":entry.id,"version":entry.version,"package_sha256":entry.package_sha256,"export":export})
    }
}

fn load_package(path: &Path, expected: Option<&Entry>) -> Result<Package, ConfigError> {
    let raw = filesystem::read(path, MAX_PACKAGE)?;
    let package: Package = decode(&raw)?;
    package.validate()?;
    if expected.is_some_and(|entry| {
        hash(&raw) != entry.package_sha256
            || package.id != entry.id
            || package.version != entry.version
    }) {
        return Err(invalid());
    }
    Ok(package)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityImport {
    pub pack: String,
    pub export: String,
    pub provider: String,
    pub upstream_model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyImport {
    pub pack: String,
    pub export: String,
}

impl Config {
    /// Assemble explicit host imports, rejecting overrides before ordinary route validation.
    pub fn parse_with_profile_packs(raw: &str, plan: ProfilePackPlan) -> Result<Self, ConfigError> {
        let mut config: Self = toml::from_str(raw).map_err(|_| {
            ConfigError("Invalid TOML configuration or unknown configuration field".into())
        })?;
        config.import_profile_packs(plan)?;
        config.validate()?;
        Ok(config)
    }
    pub(crate) fn import_profile_packs(
        &mut self,
        plan: ProfilePackPlan,
    ) -> Result<(), ConfigError> {
        let config = self;
        for (id, import) in &config.capability_profile_imports {
            let package = plan.selected(&import.pack, &import.export)?;
            let template = package
                .capabilities
                .get(&import.export)
                .ok_or_else(invalid)?;
            let profile = template.bind(&package.version, &import.provider, &import.upstream_model);
            if config
                .capability_profiles
                .insert(id.clone(), profile)
                .is_some()
            {
                return Err(invalid());
            }
        }
        for (id, import) in &config.compatibility_policy_imports {
            let policy = plan
                .selected(&import.pack, &import.export)?
                .policies
                .get(&import.export)
                .ok_or_else(invalid)?;
            if config
                .compatibility_policies
                .insert(id.clone(), policy.clone())
                .is_some()
            {
                return Err(invalid());
            }
        }
        config.profile_packs = Some(plan);
        Ok(())
    }

    pub(crate) fn validate_profile_imports(&self) -> Result<(), ConfigError> {
        if self.capability_profile_imports.len() + self.compatibility_policy_imports.len() > 128 {
            return Err(invalid());
        }
        if self.profile_packs.is_none()
            && (!self.capability_profile_imports.is_empty()
                || !self.compatibility_policy_imports.is_empty())
        {
            return Err(invalid());
        }
        if let Some(plan) = &self.profile_packs {
            for (id, import) in &self.capability_profile_imports {
                let package = plan.selected(&import.pack, &import.export)?;
                let template = package
                    .capabilities
                    .get(&import.export)
                    .ok_or_else(invalid)?;
                let expected =
                    template.bind(&package.version, &import.provider, &import.upstream_model);
                let actual = self.capability_profiles.get(id).ok_or_else(invalid)?;
                if !label(id) || canonical(actual)? != canonical(&expected)? {
                    return Err(invalid());
                }
            }
            for (id, import) in &self.compatibility_policy_imports {
                let policy = plan
                    .selected(&import.pack, &import.export)?
                    .policies
                    .get(&import.export)
                    .ok_or_else(invalid)?;
                if !label(id) || self.compatibility_policies.get(id) != Some(policy) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn profile_pack_projection(&self) -> Option<Value> {
        self.profile_packs.as_ref().map(|plan| {
            let mut value = plan.projection();
            value["capability_imports"] = json!(self.capability_profile_imports);
            value["policy_imports"] = json!(self.compatibility_policy_imports);
            value
        })
    }

    pub(crate) fn route_pack_projection(&self, model: &crate::Model) -> Option<Value> {
        let plan = self.profile_packs.as_ref()?;
        let capability = model
            .capability_profile
            .as_ref()
            .and_then(|id| self.capability_profile_imports.get(id));
        let policy = model
            .compatibility_policy
            .as_ref()
            .and_then(|id| self.compatibility_policy_imports.get(id));
        if capability.is_none() && policy.is_none() {
            return None;
        }
        Some(
            json!({"capability":capability.map(|i| plan.entry(&i.pack, &i.export)), "policy":policy.map(|i| plan.entry(&i.pack, &i.export))}),
        )
    }
}
