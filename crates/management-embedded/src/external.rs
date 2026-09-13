use gateway_management::{Digest, Error, Id, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, net::SocketAddr};
pub const EXTERNAL_SCHEMA: &str = "gateway-external-access/v1";
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPath {
    Models,
    Responses,
}
impl ClientPath {
    pub fn method(self) -> &'static str {
        match self {
            Self::Models => "GET",
            Self::Responses => "POST",
        }
    }
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Responses => "responses",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAccess {
    pub schema: String,
    pub target: Id,
    pub public_base: String,
    pub paths: BTreeSet<ClientPath>,
    pub external_credential: Id,
    pub gateway_credential: Id,
}
impl ExternalAccess {
    pub fn validate(&self) -> Result<()> {
        if self.schema != EXTERNAL_SCHEMA {
            return Err(Error::UnsupportedSchema);
        }
        let url = url::Url::parse(&self.public_base).map_err(|_| Error::InvalidInput)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.path().trim_end_matches('/').ends_with("/v1")
            || self.paths.is_empty()
            || self.external_credential == self.gateway_credential
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
    pub fn endpoint(&self, path: ClientPath) -> Result<String> {
        self.validate()?;
        if !self.paths.contains(&path) {
            return Err(Error::Unsupported);
        }
        Ok(format!(
            "{}/{}",
            self.public_base.trim_end_matches('/'),
            path.suffix()
        ))
    }
    /// The host checks actual protected values; distinct reference names alone are insufficient.
    /// No value or verifier is saved, logged or returned in the public contract.
    pub fn validate_credentials(&self, external: &str, gateway: &str) -> Result<()> {
        self.validate()?;
        let valid =
            |v: &str| (32..=4096).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_graphic());
        if !valid(external)
            || !valid(gateway)
            || Digest::of(external.as_bytes()) == Digest::of(gateway.as_bytes())
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
}
/// Expected manifest must already be trusted through the host's artifact/configuration policy.
/// Self-consistent JSON, a digest or this validator alone is not authentication or attestation.
pub struct ExpectedRuntime {
    configuration: Digest,
    execution: Option<Digest>,
    manifest: String,
    ready: String,
    package_version: String,
    listen: SocketAddr,
}
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeObservation {
    pub address: SocketAddr,
    pub base_url: String,
    pub configuration_sha256: Digest,
    pub execution_sha256: Option<Digest>,
    pub manifest_schema: String,
    pub readiness_schema: String,
}
fn digest(value: &Value) -> Result<Digest> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidInput)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(Error::InvalidInput);
    }
    Ok(Digest::of(&bytes))
}
fn field_digest(value: &Value, key: &str) -> Result<Digest> {
    Digest::try_from(value[key].as_str().ok_or(Error::InvalidInput)?.to_owned())
}
fn version(schema: &str, extended: bool) -> Result<u8> {
    let prefix = if extended {
        "gateway-extended-manifest/v"
    } else {
        "gateway-embedded-manifest/v"
    };
    let version = schema
        .strip_prefix(prefix)
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|v| (1..=7).contains(v) && (*v != 2 || extended))
        .ok_or(Error::UnsupportedSchema)?;
    if schema != format!("{prefix}{version}") {
        return Err(Error::UnsupportedSchema);
    }
    Ok(version)
}
impl ExpectedRuntime {
    pub fn from_manifest(expected: &Value) -> Result<Self> {
        let manifest = expected["schema"].as_str().ok_or(Error::InvalidInput)?;
        let extended = manifest.starts_with("gateway-extended-manifest/");
        let v = version(manifest, extended)?;
        let (base, execution) = if extended {
            let execution = field_digest(expected, "execution_sha256")?;
            if digest(&expected["configuration"])? != execution {
                return Err(Error::InvalidInput);
            }
            (&expected["configuration"]["gateway"], Some(execution))
        } else {
            (expected, None)
        };
        version(base["schema"].as_str().ok_or(Error::InvalidInput)?, false)?;
        let configuration = field_digest(base, "configuration_sha256")?;
        if digest(&base["configuration"])? != configuration
            || base["client_api"] != "responses"
            || base["lifecycle"] != "host-supervised-process/v1"
            || base["package"]["name"] != "agent-response-gateway"
        {
            return Err(Error::InvalidInput);
        }
        let package_version = base["package"]["version"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 32)
            .ok_or(Error::InvalidInput)?
            .to_owned();
        let listen: SocketAddr = base["configuration"]["listen"]
            .as_str()
            .ok_or(Error::InvalidInput)?
            .parse()
            .map_err(|_| Error::InvalidInput)?;
        if !listen.ip().is_loopback() {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            configuration,
            execution,
            manifest: manifest.into(),
            ready: format!(
                "{}{}",
                if extended {
                    "gateway-extended-ready/v"
                } else {
                    "gateway-ready/v"
                },
                v
            ),
            package_version,
            listen,
        })
    }
    /// Bind a managed child's wrapper to a host-selected instance. The host still owns
    /// the authenticated parent channel and must implement its bounded shutdown policy.
    pub fn confirm_managed(&self, instance: &Id, frame: &Value) -> Result<RuntimeObservation> {
        if frame.as_object().is_none_or(|v| v.len() != 3)
            || frame["schema"] != "gateway-managed-process/v1"
            || frame["instance_id"] != instance.as_str()
        {
            return Err(Error::InvalidInput);
        }
        self.confirm(&frame["gateway"])
    }
    pub fn confirm(&self, ready: &Value) -> Result<RuntimeObservation> {
        if ready["event"] != "ready"
            || ready["schema"] != self.ready
            || ready["manifest_schema"] != self.manifest
            || ready["version"] != self.package_version
            || field_digest(ready, "configuration_sha256")? != self.configuration
        {
            return Err(Error::InvalidInput);
        }
        let execution = ready
            .get("execution_sha256")
            .map(|_| field_digest(ready, "execution_sha256"))
            .transpose()?;
        if execution != self.execution {
            return Err(Error::InvalidInput);
        }
        let address: SocketAddr = ready["address"]
            .as_str()
            .ok_or(Error::InvalidInput)?
            .parse()
            .map_err(|_| Error::InvalidInput)?;
        let base_url = ready["base_url"]
            .as_str()
            .ok_or(Error::InvalidInput)?
            .to_owned();
        if !address.ip().is_loopback()
            || address.port() == 0
            || address.ip() != self.listen.ip()
            || (self.listen.port() != 0 && address.port() != self.listen.port())
            || base_url != format!("http://{address}/v1")
        {
            return Err(Error::InvalidInput);
        }
        Ok(RuntimeObservation {
            address,
            base_url,
            configuration_sha256: self.configuration.clone(),
            execution_sha256: execution,
            manifest_schema: self.manifest.clone(),
            readiness_schema: self.ready.clone(),
        })
    }
}
