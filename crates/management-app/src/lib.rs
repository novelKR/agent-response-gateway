//! Optional composition of the real management adapters; the Core never depends on this crate.
pub mod cli;
mod control;
mod dispatch;
mod runtime;
pub mod settings;
#[cfg(feature = "team")]
mod team;
mod web;
use gateway_management::{Error, Id, Journal, Reader, Result, filesystem};
use gateway_management_api::{Authenticator, Service};
use gateway_management_extensions::{
    Driver, Manager as PackageManager, Registration as PackageRegistration,
};
use gateway_management_runtime::Runtime;
use settings::{STORE_BYTES, Settings};
use std::{
    io::Write,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Component {
    Management,
    Runtime,
    Native,
    ProfilePack,
    Continuation,
    Team,
    TeamRequests,
}
fn packages(
    settings: &Settings,
    selected: &settings::Packages,
    native: bool,
) -> Result<PackageRegistration> {
    Ok(PackageRegistration {
        target: settings.target.clone(),
        directory: selected.directory.clone(),
        store: selected.store.clone(),
        driver: if native {
            Driver::Native(selected.driver.clone().ok_or(Error::InvalidInput)?)
        } else {
            Driver::ProfilePack
        },
        sources: selected.sources.clone(),
        recorder_bindings: selected.recorder_bindings.clone(),
    })
}
fn directory(path: &Path) -> Result<()> {
    if path.symlink_metadata().is_ok() {
        filesystem::directory(path)
    } else {
        settings::private_directory(path)
    }
}
/// Explicit initialization of exactly one selected new store. Existing formats are not migrated.
pub fn initialize(settings: &Settings, component: Component) -> Result<()> {
    settings.validate()?;
    match component {
        Component::Management => {
            directory(&settings.journal)?;
            drop(Journal::initialize(&settings.journal, STORE_BYTES)?);
        }
        Component::Runtime => {
            directory(&settings.runtime.directory)?;
            drop(Runtime::initialize(
                settings.runtime_registration(Default::default()),
            )?);
        }
        Component::Native | Component::ProfilePack => {
            let native = matches!(component, Component::Native);
            let p = if native {
                &settings.native
            } else {
                &settings.profile_packs
            };
            let p = p.as_ref().ok_or(Error::Unsupported)?;
            directory(&p.directory)?;
            drop(PackageManager::initialize(packages(settings, p, native)?)?);
        }
        Component::Continuation => {
            let path = settings.continuation.as_ref().ok_or(Error::Unsupported)?;
            directory(path)?;
            let mut file = filesystem::private_new(&path.join("schema"))?;
            file.write_all(b"gateway-management-continuation/v1\n")
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::Storage)?;
        }
        #[cfg(feature = "team")]
        Component::Team => {
            let t = settings.team.as_ref().ok_or(Error::Unsupported)?;
            directory(&t.directory)?;
            drop(gateway_team_access::Manager::initialize(
                &t.directory,
                settings.target.clone(),
                STORE_BYTES,
            )?);
        }
        #[cfg(feature = "team")]
        Component::TeamRequests => {
            let t = settings.team.as_ref().ok_or(Error::Unsupported)?;
            directory(&t.requests)?;
            drop(gateway_team_http::Ledger::initialize(
                &t.requests,
                settings.target.clone(),
                STORE_BYTES,
            )?);
        }
        #[cfg(not(feature = "team"))]
        Component::Team | Component::TeamRequests => return Err(Error::Unsupported),
    }
    Ok(())
}
pub fn backup(settings: &Settings, component: Component, destination: &Path) -> Result<()> {
    settings.validate()?;
    match component {
        Component::Management => Journal::open(&settings.journal, STORE_BYTES)?.backup(destination),
        #[cfg(feature = "team")]
        Component::Team => {
            let t = settings.team.as_ref().ok_or(Error::Unsupported)?;
            gateway_team_access::Manager::open(&t.directory, settings.target.clone(), STORE_BYTES)?
                .backup(destination)
        }
        #[cfg(feature = "team")]
        Component::TeamRequests => {
            let t = settings.team.as_ref().ok_or(Error::Unsupported)?;
            gateway_team_http::Ledger::open(&t.requests, settings.target.clone(), STORE_BYTES)?
                .backup(destination)
        }
        _ => Err(Error::Unsupported),
    }
}
pub struct Application {
    pub target: Id,
    pub api: Arc<Service>,
    runtime: runtime::OwnedRuntime,
    assets: Option<web::Assets>,
    #[cfg(feature = "team")]
    pub team: Option<Arc<gateway_team_http::Service>>,
}
impl Application {
    /// Construct on a blocking host thread. No process starts until an audited Start operation.
    /// The caller owns listeners and supplies their already bound numeric loopback addresses.
    pub fn open(
        settings: Settings,
        bound: SocketAddr,
        team_bound: Option<SocketAddr>,
    ) -> Result<Self> {
        settings.validate()?;
        if !bound.ip().is_loopback()
            || bound.port() == 0
            || bound.ip() != settings.listen.ip()
            || (settings.listen.port() != 0 && bound.port() != settings.listen.port())
        {
            return Err(Error::InvalidInput);
        }
        if settings.team.is_some() != team_bound.is_some() {
            return Err(Error::InvalidInput);
        }
        let (local, environment) = settings.bindings()?;
        let environment = Arc::new(environment);
        let owner = Arc::new(Mutex::new(Runtime::open(
            settings.runtime_registration((*environment).clone()),
        )?));
        let journal = Journal::open(&settings.journal, STORE_BYTES)?;
        let reader = Reader::open(&settings.journal)?;
        let native = settings
            .native
            .as_ref()
            .map(|p| packages(&settings, p, true).and_then(PackageManager::open))
            .transpose()?;
        let profiles = settings
            .profile_packs
            .as_ref()
            .map(|p| packages(&settings, p, false).and_then(PackageManager::open))
            .transpose()?;
        let control = settings
            .continuation
            .clone()
            .map(|d| control::Control::open(d, environment.clone()))
            .transpose()?;
        let assets = settings
            .web
            .as_ref()
            .map(|w| web::Assets::load(w, bound))
            .transpose()?;
        #[cfg(feature = "team")]
        let (team_manager, team_auth, team_http) = if let Some(team) = &settings.team {
            let bound = team_bound.ok_or(Error::InvalidInput)?;
            if !bound.ip().is_loopback()
                || bound.ip() != team.listen.ip()
                || (team.listen.port() != 0 && bound.port() != team.listen.port())
            {
                return Err(Error::InvalidInput);
            }
            let manager = gateway_team_access::Manager::open(
                &team.directory,
                settings.target.clone(),
                STORE_BYTES,
            )?;
            team::validate_namespace(&manager)?;
            let auth = Arc::new(gateway_team_access::Authenticator::open(
                &team.directory,
                settings.target.clone(),
                Reader::open(&settings.journal)?,
            )?);
            let ledger = gateway_team_http::Ledger::open(
                &team.requests,
                settings.target.clone(),
                STORE_BYTES,
            )?;
            let usage = settings
                .usage
                .as_ref()
                .map(|u| {
                    gateway_team_http::SqliteUsage::open(&u.directory)
                        .map(|u| Box::new(u) as Box<dyn gateway_team_http::UsageReader>)
                })
                .transpose()?;
            let peers = team::RuntimePeer {
                target: settings.target.clone(),
                runtime: owner.clone(),
                environment: environment.clone(),
                usage: settings.usage.clone(),
            };
            let http = gateway_team_http::Service::new(
                bound,
                auth.clone(),
                Arc::new(peers),
                ledger,
                usage,
                gateway_team_http::Limits::default(),
            )?;
            (Some(manager), Some(auth), Some(http))
        } else {
            (None, None, None)
        };
        #[cfg(feature = "team")]
        let auth: Arc<dyn Authenticator> = Arc::new(team::Authority {
            local,
            team: team_auth.clone(),
        });
        #[cfg(not(feature = "team"))]
        let auth: Arc<dyn Authenticator> = Arc::new(local);
        let adapters = dispatch::Adapters {
            web_enabled: assets.is_some(),
            target: settings.target.clone(),
            runtime: owner.clone(),
            native,
            profiles,
            usage: settings.usage,
            control,
            local_identities: settings
                .credentials
                .iter()
                .map(|c| (c.subject.clone(), c.credential.clone()))
                .collect(),
            #[cfg(feature = "team")]
            team: team_manager,
            #[cfg(feature = "team")]
            team_auth,
            #[cfg(feature = "team")]
            team_http: team_http.clone(),
            #[cfg(feature = "team")]
            secret: None,
        };
        let api = Service::new(
            settings.target.clone(),
            bound,
            auth,
            journal,
            reader,
            Box::new(adapters),
            assets.is_some(),
        )
        .map_err(|_| Error::InvalidInput)?;
        Ok(Self {
            target: settings.target,
            api,
            runtime: owner,
            assets,
            #[cfg(feature = "team")]
            team: team_http,
        })
    }
    pub fn router(&mut self) -> axum::Router {
        let router = self.api.router();
        if let Some(assets) = self.assets.take() {
            router.merge(assets.router())
        } else {
            router
        }
    }
    /// Emergency owned-child cleanup stays independent of the management journal's health.
    pub fn shutdown(&self) -> Result<()> {
        self.api.close_admission();
        self.runtime
            .lock()
            .map_err(|_| Error::Storage)?
            .stop_owned()
    }
}

#[cfg(test)]
mod tests;
