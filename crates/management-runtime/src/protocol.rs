use crate::files;
use agent_response_gateway::{Config, extensions::ExtensionPlan, profile_packs::ProfilePackPlan};
use gateway_management::{Digest, Error, Id, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const LAUNCH_SCHEMA: &str = "gateway-managed-launch/v1";
pub const READY_SCHEMA: &str = "gateway-managed-process/v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub schema: String,
    pub instance_id: Id,
    pub directory: PathBuf,
    pub configuration: PathBuf,
    pub extensions_lock: Option<PathBuf>,
    pub profile_packs_lock: Option<PathBuf>,
    pub configuration_sha256: Digest,
    pub execution_sha256: Option<Digest>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ready {
    pub schema: String,
    pub instance_id: Id,
    pub gateway: GatewayReady,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayReady {
    pub event: String,
    pub address: std::net::SocketAddr,
    pub base_url: String,
    pub version: String,
    pub schema: String,
    pub manifest_schema: String,
    pub configuration_sha256: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_sha256: Option<Digest>,
}
pub(crate) struct Inspection {
    pub config: Config,
    pub extensions: Option<ExtensionPlan>,
    pub configuration_sha256: Digest,
    pub execution_sha256: Option<Digest>,
    pub manifest: Value,
}
pub(crate) fn inspect(
    configuration: &std::path::Path,
    extensions: Option<&std::path::Path>,
    packs: Option<&std::path::Path>,
) -> Result<Inspection> {
    let bytes = files::read(configuration, files::MAX_CONFIG, false)?;
    inspect_bytes(&bytes, extensions, packs)
}
pub(crate) fn inspect_bytes(
    bytes: &[u8],
    extensions: Option<&std::path::Path>,
    packs: Option<&std::path::Path>,
) -> Result<Inspection> {
    let raw = std::str::from_utf8(bytes).map_err(|_| Error::InvalidInput)?;
    let extensions = extensions
        .map(ExtensionPlan::load)
        .transpose()
        .map_err(|_| Error::InvalidInput)?;
    let packs = packs
        .map(ProfilePackPlan::load)
        .transpose()
        .map_err(|_| Error::InvalidInput)?;
    let config =
        Config::parse_startup(raw, packs, extensions.as_ref()).map_err(|_| Error::InvalidInput)?;
    let base = config.manifest().map_err(|_| Error::InvalidInput)?;
    let configuration_sha256 = Digest::try_from(base.configuration_sha256().to_owned())?;
    let value = serde_json::to_value(&base).map_err(|_| Error::InvalidInput)?;
    let manifest = extensions
        .as_ref()
        .map(|p| p.manifest(&value))
        .transpose()
        .map_err(|_| Error::InvalidInput)?
        .unwrap_or(value);
    let execution_sha256 = manifest
        .get("execution_sha256")
        .map(|v| Digest::try_from(v.as_str().ok_or(Error::InvalidInput)?.to_owned()))
        .transpose()?;
    Ok(Inspection {
        config,
        extensions,
        configuration_sha256,
        execution_sha256,
        manifest,
    })
}
