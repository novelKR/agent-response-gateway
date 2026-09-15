//! Explicitly selected provider semantics; the host keeps transport and durable ownership.
pub(crate) mod execution;
pub(crate) use execution::{
    AuthorizedProviderHistory, PreparedProvider, ProviderLimits, ProviderOutput, ProviderStream,
};
mod process;
mod usage;
pub(crate) mod contract {
    pub use gateway_plugin_contract::provider::*;
}
use gateway_plugin_contract::Capabilities;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(crate) struct Binding {
    pub protocol: String,
    pub provider_protocol: String,
    pub id: String,
    pub version: String,
    pub package_sha256: String,
    pub executable_sha256: String,
    pub capabilities: Capabilities,
    pub executable: PathBuf,
    pub directory: PathBuf,
    #[cfg(unix)]
    pub owner: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProviderIdentity {
    pub protocol: String,
    pub provider_protocol: String,
    pub id: String,
    pub version: String,
    pub package_sha256: String,
    pub executable_sha256: String,
}
impl Binding {
    pub(crate) fn projection(&self) -> Value {
        json!({"protocol":self.protocol,"provider_protocol":self.provider_protocol,
            "id":self.id,"version":self.version,"package_sha256":self.package_sha256,
            "executable_sha256":self.executable_sha256,"capabilities":self.capabilities,
            "permissions":crate::extensions::CODEC_PERMISSIONS})
    }
    fn identity(&self) -> ProviderIdentity {
        ProviderIdentity {
            protocol: self.protocol.clone(),
            provider_protocol: self.provider_protocol.clone(),
            id: self.id.clone(),
            version: self.version.clone(),
            package_sha256: self.package_sha256.clone(),
            executable_sha256: self.executable_sha256.clone(),
        }
    }
}
// Retained in dispatch now; recorder v2 consumes these fields in the following change.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct ProviderObservation {
    pub identity: ProviderIdentity,
    pub usage: gateway_usage_contract::CanonicalUsage,
}
#[cfg(test)]
mod tests;

impl ProviderIdentity {
    pub(crate) fn state_binding(&self) -> crate::ir::continuity::ProviderStateBinding {
        crate::ir::continuity::ProviderStateBinding {
            protocol: self.protocol.clone(),
            provider_protocol: self.provider_protocol.clone(),
            id: self.id.clone(),
            version: self.version.clone(),
            package_sha256: self.package_sha256.clone(),
            executable_sha256: self.executable_sha256.clone(),
        }
    }
}
