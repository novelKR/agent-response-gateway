//! Explicit request-scoped native codec execution through versioned IPC.
pub mod contract;
pub(crate) mod dispatch;
pub mod engine;
pub(crate) mod execution;
mod process;
mod verification;

use crate::{Config, ConfigError};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

/// Frozen activation reference; not a grant to credentials, transport or storage.
#[derive(Clone, Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) struct Binding {
    pub id: String,
    pub version: String,
    pub package_sha256: String,
    pub executable_sha256: String,
    pub executable: PathBuf,
    pub directory: PathBuf,
    #[cfg(unix)]
    pub owner: u32,
}
impl Binding {
    pub(crate) fn projection(&self) -> Value {
        json!({"id":self.id,"version":self.version,"package_sha256":self.package_sha256,"executable_sha256":self.executable_sha256,"protocol":contract::PROTOCOL,"replay_versions":[1],"permissions":crate::extensions::CODEC_PERMISSIONS})
    }
}
impl Config {
    /// Resolve both explicit data imports and activated executable references before validation.
    pub fn parse_startup(
        raw: &str,
        packs: Option<crate::profile_packs::ProfilePackPlan>,
        extensions: Option<&crate::extensions::ExtensionPlan>,
    ) -> Result<Self, ConfigError> {
        let mut config: Self = toml::from_str(raw).map_err(|_| {
            ConfigError("Invalid TOML configuration or unknown configuration field".into())
        })?;
        if let Some(packs) = packs {
            config.import_profile_packs(packs)?;
        }
        config.codecs = extensions.map_or_else(
            BTreeMap::new,
            crate::extensions::ExtensionPlan::codec_bindings,
        );
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests;
