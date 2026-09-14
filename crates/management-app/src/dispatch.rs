use crate::{
    control::Control,
    runtime::{LockedRuntime, OwnedRuntime, effective},
    settings::{Usage, now},
};
use gateway_management::{
    Action, Actor, Digest, Effect, Error, FailureCode, Id, Operation, PreparedOperation, Request,
    Result, Snapshot,
};
use gateway_management_api::{
    Command, Dispatcher, Feature, ModuleView, Observation, PackageFamily, Query, StateView,
};
use gateway_management_extensions::{Command as PackageCommand, Manager, Selection};
use gateway_management_runtime::Command as RuntimeCommand;
use serde_json::Value;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use serde_json::json;
use std::collections::BTreeSet;

pub struct Adapters {
    pub target: Id,
    pub web_enabled: bool,
    pub runtime: OwnedRuntime,
    pub native: Option<Manager>,
    pub profiles: Option<Manager>,
    pub usage: Option<Usage>,
    pub control: Option<Control>,
    pub local_identities: BTreeSet<(Id, Id)>,
    #[cfg(feature = "team")]
    pub team: Option<gateway_team_access::Manager>,
    #[cfg(feature = "team")]
    pub team_auth: Option<std::sync::Arc<gateway_team_access::Authenticator>>,
    #[cfg(feature = "team")]
    pub team_http: Option<std::sync::Arc<gateway_team_http::Service>>,
    #[cfg(feature = "team")]
    pub secret: Option<gateway_team_access::Secret>,
}
fn runtime(command: &Command) -> Option<RuntimeCommand> {
    Some(match command {
        Command::RuntimeStart {} => RuntimeCommand::Start,
        Command::RuntimeStop {} => RuntimeCommand::Stop,
        Command::RuntimeRestart {} => RuntimeCommand::Restart,
        Command::ConfigurationStage {
            source,
            candidate,
            source_sha256,
        } => RuntimeCommand::Stage {
            source: source.clone(),
            candidate: candidate.clone(),
            source_sha256: source_sha256.clone(),
        },
        Command::ConfigurationSelect { candidate } => RuntimeCommand::Select {
            candidate: candidate.clone(),
        },
        _ => return None,
    })
}
fn package(command: &Command) -> Option<(PackageFamily, PackageCommand)> {
    Some(match command {
        Command::PackageInstall { family, source } => (
            *family,
            PackageCommand::Install {
                source: source.clone(),
            },
        ),
        Command::PackageDisable { family, package } => (
            *family,
            PackageCommand::Disable {
                package: package.clone(),
            },
        ),
        Command::PackageEnable {
            family,
            package,
            grants,
            recorder,
        }
        | Command::PackageSelect {
            family,
            package,
            grants,
            recorder,
        } => {
            let package = Selection {
                id: package.id.clone(),
                version: package.version.clone(),
                package_sha256: package.package_sha256.clone(),
            };
            (
                *family,
                if matches!(command, Command::PackageSelect { .. }) {
                    PackageCommand::Select {
                        package,
                        grants: grants.clone(),
                        recorder: recorder.clone(),
                    }
                } else {
                    PackageCommand::Enable {
                        package,
                        grants: grants.clone(),
                        recorder: recorder.clone(),
                    }
                },
            )
        }
        _ => return None,
    })
}
fn feature(
    id: &str,
    version: &str,
    installed: bool,
    enabled: bool,
    operations: Vec<Action>,
) -> Feature {
    Feature {
        id: Id::new(id).expect("fixed ID"),
        version: version.into(),
        installed,
        enabled,
        operations,
    }
}
fn module(id: &str, contract: &str, result: Result<Value>) -> Result<ModuleView> {
    Ok(ModuleView {
        id: Id::new(id)?,
        contract: contract.into(),
        observation: match result {
            Ok(data) => Observation::Observed {
                observed_at_ms: now()?,
                data,
            },
            Err(Error::Unsupported) => Observation::Unsupported {},
            Err(_) => Observation::Unobserved {
                reason: Id::new("adapter_unavailable")?,
            },
        },
    })
}
impl Adapters {
    fn packages(&mut self, family: PackageFamily) -> Result<&mut Manager> {
        match family {
            PackageFamily::Native => self.native.as_mut(),
            PackageFamily::ProfilePack => self.profiles.as_mut(),
        }
        .ok_or(Error::Unsupported)
    }
    fn local(&self, actor: &Actor) -> bool {
        self.local_identities.contains(&(
            actor.identity().subject.clone(),
            actor.identity().credential.clone(),
        ))
    }
    fn usage(&self, actor: &Actor, range: &gateway_management_api::UsageRange) -> Result<Value> {
        range.validate()?;
        if !self.local(actor) {
            #[cfg(feature = "team")]
            {
                let principal = self
                    .team_auth
                    .as_ref()
                    .ok_or(Error::Forbidden)?
                    .management_identity(actor.identity())?;
                let service = self.team_http.as_ref().ok_or(Error::Unsupported)?;
                let report = service.usage_view(
                    &principal,
                    &gateway_team_http::UsageQuery {
                        from_ms: range.from_ms,
                        to_ms: range.to_ms,
                        after: range.after,
                        all: principal.permissions.read_all_usage,
                    },
                )?;
                gateway_team_http::validate_usage_report(&report)?;
                return Ok(report);
            }
            #[cfg(not(feature = "team"))]
            return Err(Error::Forbidden);
        }
        if range.after != 0 {
            return Err(Error::InvalidInput);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let store = gateway_usage_recorder::Store::open(
                &self.usage.as_ref().ok_or(Error::Unsupported)?.directory,
                false,
                false,
            )
            .map_err(|_| Error::Storage)?;
            let mut value = gateway_usage_recorder::query::aggregate(
                &store,
                range.from_ms,
                range.to_ms,
                &range.timezone,
            )
            .map_err(|_| Error::Storage)?;
            value["schema"] = json!(if value["usage_contract"] == "gateway-usage-event/v2" {
                "gateway-management-usage/v2"
            } else {
                "gateway-management-usage/v1"
            });
            value["scope"] = json!("gateway_recorder");
            Ok(value)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Err(Error::Unsupported)
    }
}
impl Dispatcher for Adapters {
    fn features(&self) -> Vec<Feature> {
        let lifecycle = vec![
            Action::ReadState,
            Action::ReadOperations,
            Action::Reconcile,
            Action::RuntimeStart,
            Action::RuntimeStop,
            Action::RuntimeRestart,
            Action::ConfigurationStage,
            Action::ConfigurationSelect,
        ];
        let package_actions = vec![
            Action::PackageInstall,
            Action::PackageEnable,
            Action::PackageDisable,
            Action::PackageSelect,
        ];
        let mut features = vec![
            feature(
                "standalone-web",
                "gateway-management-web/v1",
                true,
                self.web_enabled,
                vec![],
            ),
            feature(
                "management",
                "gateway-management-http/v1",
                true,
                true,
                lifecycle,
            ),
            feature(
                "native-extensions",
                "gateway-extension-status/v1",
                cfg!(any(target_os = "linux", target_os = "macos")),
                self.native.is_some(),
                if self.native.is_some() {
                    package_actions.clone()
                } else {
                    vec![]
                },
            ),
            feature(
                "profile-packs",
                "gateway-extension-status/v1",
                true,
                self.profiles.is_some(),
                if self.profiles.is_some() {
                    package_actions
                        .into_iter()
                        .filter(|a| *a != Action::PackageSelect)
                        .collect()
                } else {
                    vec![]
                },
            ),
            feature(
                "usage",
                "gateway-management-usage/v2",
                cfg!(any(target_os = "linux", target_os = "macos")),
                self.usage.is_some(),
                if self.usage.is_some() {
                    vec![Action::ReadUsage]
                } else {
                    vec![]
                },
            ),
            feature(
                "continuation",
                "gateway-management-continuation/v1",
                true,
                self.control.is_some(),
                if self.control.is_some() {
                    vec![Action::ContinuationTransition]
                } else {
                    vec![]
                },
            ),
        ];
        #[cfg(feature = "team")]
        features.push(feature(
            "team-access",
            "gateway-team-access/v1",
            true,
            self.team.is_some(),
            if self.team.is_some() {
                let mut actions = crate::team::actions();
                actions.push(Action::ReadUsage);
                actions
            } else {
                vec![]
            },
        ));
        #[cfg(not(feature = "team"))]
        features.push(feature(
            "team-access",
            "gateway-team-access/v1",
            false,
            false,
            vec![],
        ));
        features
    }
    fn supported(&self) -> Vec<Action> {
        self.features()
            .into_iter()
            .filter(|f| f.enabled)
            .flat_map(|f| f.operations)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    fn snapshot(&mut self, command: &Command) -> Result<Snapshot> {
        if runtime(command).is_some() {
            return self.runtime.lock().map_err(|_| Error::Storage)?.snapshot();
        }
        if let Some((family, _)) = package(command) {
            return self.packages(family)?.snapshot();
        }
        if matches!(command, Command::ContinuationTransition { .. }) {
            let request = Request {
                target: self.target.clone(),
                action: command.action(),
                expected: Snapshot {
                    revision: 0,
                    digest: Digest::of(b"preflight"),
                },
                idempotency_key: Id::new("preflight")?,
                parameters_sha256: command.digest()?,
            };
            return Ok(self
                .control
                .as_ref()
                .ok_or(Error::Unsupported)?
                .prepare(&self.runtime, &request, command)?
                .before()
                .clone());
        }
        #[cfg(feature = "team")]
        if crate::team::actions().contains(&command.action()) {
            return self.team.as_ref().ok_or(Error::Unsupported)?.snapshot();
        }
        Err(Error::Unsupported)
    }
    fn read(&mut self, actor: &Actor, query: &Query) -> Result<Value> {
        actor.authorize(query.action(), &self.target)?;
        match query {
            Query::Usage(range) => self.usage(actor, range),
            Query::Continuation(id) => {
                if !self.local(actor) {
                    return Err(Error::Forbidden);
                }
                self.control
                    .as_ref()
                    .ok_or(Error::Unsupported)?
                    .view(&self.runtime, id)
            }
            Query::State => {
                let status = self.runtime.lock().map_err(|_| Error::Storage)?.status();
                let runtime_value = status
                    .as_ref()
                    .map_err(|_| Error::Storage)
                    .and_then(|s| serde_json::to_value(s).map_err(|_| Error::Storage));
                let mut modules = vec![module(
                    "runtime",
                    "gateway-runtime-status/v1",
                    runtime_value,
                )?];
                for (id, native, manager) in [
                    ("native", true, &self.native),
                    ("profiles", false, &self.profiles),
                ] {
                    let result = match manager {
                        Some(manager) => {
                            let effective = status
                                .as_ref()
                                .ok()
                                .map(|s| effective(s, native))
                                .transpose()?
                                .flatten();
                            manager
                                .status(effective)
                                .and_then(|s| serde_json::to_value(s).map_err(|_| Error::Storage))
                        }
                        None => Err(Error::Unsupported),
                    };
                    modules.push(module(id, "gateway-extension-status/v1", result)?);
                }
                #[cfg(feature = "team")]
                modules.push(module(
                    "team",
                    "gateway-team-access/v1",
                    self.team
                        .as_ref()
                        .ok_or(Error::Unsupported)
                        .and_then(|t| t.inventory())
                        .and_then(|s| serde_json::to_value(s).map_err(|_| Error::Storage)),
                )?);
                let view = StateView {
                    schema: gateway_management_api::STATE_SCHEMA.into(),
                    modules,
                };
                view.validate()?;
                serde_json::to_value(view).map_err(|_| Error::Storage)
            }
        }
    }
    fn prepare<'a>(
        &'a mut self,
        request: &'a Request,
        command: &'a Command,
    ) -> Result<Box<dyn PreparedOperation + 'a>> {
        command.validate()?;
        let intent = command.digest()?;
        if request.target != self.target
            || request.action != command.action()
            || request.parameters_sha256 != intent
        {
            return Err(Error::InvalidInput);
        }
        if let Some(command) = runtime(command) {
            return Ok(Box::new(LockedRuntime::new(
                &self.runtime,
                request,
                command,
                intent,
            )?));
        }
        if let Some((family, command)) = package(command) {
            return self
                .packages(family)?
                .prepare_mapped(request, &command, &intent);
        }
        if matches!(command, Command::ContinuationTransition { .. }) {
            return self.control.as_ref().ok_or(Error::Unsupported)?.prepare(
                &self.runtime,
                request,
                command,
            );
        }
        #[cfg(feature = "team")]
        {
            let command = crate::team::command(command)?;
            if let gateway_team_access::Command::Register { permissions, .. }
            | gateway_team_access::Command::PermissionsChange { permissions, .. } = &command
                && permissions
                    .management
                    .iter()
                    .any(|g| g.target != self.target)
            {
                return Err(Error::InvalidInput);
            }
            self.team
                .as_mut()
                .ok_or(Error::Unsupported)?
                .prepare_mapped(request, &command, &intent, &mut self.secret)
        }
        #[cfg(not(feature = "team"))]
        Err(Error::Unsupported)
    }
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect> {
        if operation.request.target != self.target {
            return Err(Error::InvalidInput);
        }
        match operation.request.action {
            Action::RuntimeStart
            | Action::RuntimeStop
            | Action::RuntimeRestart
            | Action::ConfigurationSelect
            | Action::ConfigurationStage => self
                .runtime
                .lock()
                .map_err(|_| Error::Storage)?
                .reconcile(operation),
            Action::PackageInstall
            | Action::PackageEnable
            | Action::PackageDisable
            | Action::PackageSelect => {
                let mut evidence = None;
                for manager in [&mut self.native, &mut self.profiles].into_iter().flatten() {
                    let observed = manager.reconcile(operation)?;
                    if matches!(observed, Effect::Applied { .. }) {
                        if evidence.is_some() {
                            return Err(Error::InvalidStore);
                        }
                        evidence = Some(observed);
                    }
                }
                Ok(evidence.unwrap_or(Effect::Uncertain {
                    code: FailureCode::Unverified,
                }))
            }
            Action::ContinuationTransition => self
                .control
                .as_ref()
                .ok_or(Error::Unsupported)?
                .reconcile(operation),
            #[cfg(feature = "team")]
            action if crate::team::actions().contains(&action) => self
                .team
                .as_mut()
                .ok_or(Error::Unsupported)?
                .reconcile(operation),
            _ => Err(Error::Unsupported),
        }
    }
    #[cfg(feature = "team")]
    fn completed(
        &mut self,
        result: &Result<Operation>,
    ) -> Option<gateway_management_api::SecretDelivery> {
        let secret = self.secret.take()?;
        let operation = result.as_ref().ok()?;
        if operation.state != gateway_management::State::Succeeded {
            return None;
        }
        Some(gateway_management_api::SecretDelivery::new(
            secret.credential_id().clone(),
            secret.expose().to_owned(),
        ))
    }
}
