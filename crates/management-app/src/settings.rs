use gateway_management::{Digest, Error, Grant, Id, Identity, Result, filesystem};
use gateway_management_api::{CredentialKind, LocalAuthenticator, LocalCredential};
use gateway_management_extensions::{LocalSource, NativeDriver, RecorderBinding};
use gateway_management_runtime::{Registration, Source};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

pub const SCHEMA: &str = "gateway-management-registration/v1";
pub const STORE_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema: String,
    pub target: Id,
    pub listen: SocketAddr,
    pub journal: PathBuf,
    pub runtime: RuntimeSettings,
    pub credentials: Vec<CredentialBinding>,
    pub native: Option<Packages>,
    pub profile_packs: Option<Packages>,
    pub usage: Option<Usage>,
    pub continuation: Option<PathBuf>,
    pub web: Option<Web>,
    pub team: Option<Team>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSettings {
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub executable_sha256: Digest,
    pub credential_generation: Id,
    pub sources: BTreeMap<Id, Source>,
    /// Child environment name -> explicitly registered parent environment name.
    pub environment: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialBinding {
    pub subject: Id,
    pub credential: Id,
    pub token_env: String,
    pub read_only: bool,
    pub grants: Vec<Grant>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    pub directory: PathBuf,
    pub store: PathBuf,
    pub driver: Option<NativeDriver>,
    pub sources: BTreeMap<Id, LocalSource>,
    pub recorder_bindings: BTreeMap<Id, RecorderBinding>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    /// Existing native recorder store, never initialized by the management program.
    pub directory: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Web {
    pub directory: PathBuf,
    pub manifest_sha256: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub directory: PathBuf,
    pub requests: PathBuf,
    pub listen: SocketAddr,
}
pub fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| d.as_millis().try_into().ok())
        .ok_or(Error::Storage)
}
pub fn bytes(path: &Path, limit: u64, private: bool) -> Result<Vec<u8>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidInput);
    }
    for ancestor in path.ancestors() {
        let metadata = ancestor
            .symlink_metadata()
            .map_err(|_| Error::InvalidStore)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidStore);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(Error::InvalidStore);
            }
        }
    }
    if private {
        filesystem::directory(path.parent().ok_or(Error::InvalidStore)?)?;
        filesystem::regular(path)?;
    }
    let file = File::open(path).map_err(|_| Error::Storage)?;
    if !file.metadata().map_err(|_| Error::Storage)?.is_file() {
        return Err(Error::InvalidStore);
    }
    let mut result = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut result)
        .map_err(|_| Error::Storage)?;
    if result.len() as u64 > limit {
        return Err(Error::InvalidInput);
    }
    Ok(result)
}
pub fn private_directory(path: &Path) -> Result<()> {
    filesystem::directory(path.parent().ok_or(Error::InvalidStore)?)?;
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|_| Error::Storage)?;
    filesystem::directory(path)
}
fn environment(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::InvalidInput);
    }
    let value = std::env::var(name).map_err(|_| Error::InvalidInput)?;
    if value.is_empty() || value.len() > 4096 {
        return Err(Error::InvalidInput);
    }
    Ok(value)
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let settings: Self = serde_json::from_slice(&bytes(path, 256 * 1024, true)?)
            .map_err(|_| Error::InvalidInput)?;
        settings.validate()?;
        Ok(settings)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            return Err(Error::UnsupportedSchema);
        }
        if !self.listen.ip().is_loopback()
            || self.credentials.is_empty()
            || self.credentials.len() > 128
            || self
                .team
                .as_ref()
                .is_some_and(|t| !t.listen.ip().is_loopback())
            || self.native.as_ref().is_some_and(|p| p.driver.is_none())
            || self
                .profile_packs
                .as_ref()
                .is_some_and(|p| p.driver.is_some() || !p.recorder_bindings.is_empty())
            || self.runtime.environment.len() > 128
        {
            return Err(Error::InvalidInput);
        }
        if self.team.is_some() && !cfg!(feature = "team") {
            return Err(Error::Unsupported);
        }
        if (self.native.is_some() || self.usage.is_some())
            && !cfg!(any(target_os = "linux", target_os = "macos"))
        {
            return Err(Error::Unsupported);
        }
        for binding in &self.credentials {
            if !binding.subject.as_str().starts_with("local:")
                || !binding.credential.as_str().starts_with("local:")
                || binding.grants.iter().any(|g| g.target != self.target)
            {
                return Err(Error::InvalidInput);
            }
        }
        let mut directories = vec![&self.journal, &self.runtime.directory];
        directories.extend(self.continuation.as_ref());
        directories.extend(self.native.as_ref().map(|p| &p.directory));
        directories.extend(self.profile_packs.as_ref().map(|p| &p.directory));
        if let Some(team) = &self.team {
            directories.extend([&team.directory, &team.requests]);
        }
        for (i, path) in directories.iter().enumerate() {
            if !path.is_absolute()
                || directories
                    .iter()
                    .skip(i + 1)
                    .any(|other| path.starts_with(other) || other.starts_with(path))
            {
                return Err(Error::InvalidInput);
            }
        }
        Ok(())
    }
    pub fn bindings(&self) -> Result<(LocalAuthenticator, BTreeMap<String, String>)> {
        let mut credentials = Vec::new();
        let mut hashes = Vec::new();
        for binding in &self.credentials {
            let token = environment(&binding.token_env)?;
            if token.starts_with("gwt1_") {
                return Err(Error::InvalidInput);
            }
            hashes.push(Digest::of(token.as_bytes()));
            credentials.push(LocalCredential {
                token,
                identity: Identity {
                    subject: binding.subject.clone(),
                    credential: binding.credential.clone(),
                },
                kind: if binding.read_only {
                    CredentialKind::ReadOnly
                } else {
                    CredentialKind::Management
                },
                grants: binding.grants.clone(),
            });
        }
        let auth = LocalAuthenticator::new(credentials)?;
        let mut gateway = BTreeMap::new();
        for (name, reference) in &self.runtime.environment {
            if name.is_empty()
                || name.len() > 128
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::InvalidInput);
            }
            let value = environment(reference)?;
            if value.starts_with("gwt1_") || hashes.contains(&Digest::of(value.as_bytes())) {
                return Err(Error::Forbidden);
            }
            gateway.insert(name.clone(), value);
        }
        Ok((auth, gateway))
    }
    pub fn runtime_registration(&self, environment: BTreeMap<String, String>) -> Registration {
        Registration {
            target: self.target.clone(),
            directory: self.runtime.directory.clone(),
            executable: self.runtime.executable.clone(),
            executable_sha256: self.runtime.executable_sha256.clone(),
            credential_generation: self.runtime.credential_generation.clone(),
            sources: self.runtime.sources.clone(),
            environment,
            startup_timeout: Duration::from_secs(15),
            stop_timeout: Duration::from_secs(5),
        }
    }
}
