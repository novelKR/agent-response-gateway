use gateway_management::{
    Action, Actor, Backend, Digest, Effect, Error, Grant, Id, Identity, Journal, Operation,
    PreparedOperation, Reader, Request, State,
};
use gateway_management_api::Authenticator as ManagementAuth;
use gateway_team_access::{Authenticator, Command, Completion, Manager, Permissions, Purpose};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
const MAXIMUM: u64 = 8 * 1024 * 1024;
fn private(path: &Path) {
    fs::create_dir_all(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
fn permissions(route: &str) -> Permissions {
    Permissions {
        enabled: true,
        routes: BTreeSet::from([route.into()]),
        management: [
            Action::ReadState,
            Action::ReadUsage,
            Action::ReadOperations,
            Action::RuntimeStart,
        ]
        .into_iter()
        .map(|action| Grant {
            action,
            target: id("gateway"),
        })
        .collect(),
        read_all_usage: false,
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    team: PathBuf,
    audit: PathBuf,
    manager: Manager,
    journal: Journal,
    actor: Actor,
    serial: u64,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/team-tests");
        private(&base);
        let root = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
        let team = root.path().join("team");
        let audit = root.path().join("audit");
        private(&team);
        private(&audit);
        let manager = Manager::initialize(&team, id("gateway"), MAXIMUM).unwrap();
        let journal = Journal::initialize(&audit, MAXIMUM).unwrap();
        let actor = Actor::new(
            Identity {
                subject: id("administrator"),
                credential: id("local-admin"),
            },
            [
                Action::TeamSubjectRegister,
                Action::TeamPermissionsChange,
                Action::TeamCredentialIssue,
                Action::TeamCredentialRevoke,
                Action::TeamCredentialRotate,
                Action::ReadOperations,
                Action::Reconcile,
            ]
            .into_iter()
            .map(|action| Grant {
                action,
                target: id("gateway"),
            }),
        )
        .unwrap();
        Self {
            _root: root,
            team,
            audit,
            manager,
            journal,
            actor,
            serial: 0,
        }
    }
    fn request(&mut self, command: &Command) -> Request {
        self.serial += 1;
        Request {
            target: id("gateway"),
            action: command.action(),
            expected: self.manager.snapshot().unwrap(),
            idempotency_key: id(&format!("request-{}", self.serial)),
            parameters_sha256: command.digest().unwrap(),
        }
    }
    fn execute(&mut self, command: &Command) -> Completion {
        let request = self.request(command);
        self.manager
            .execute(&mut self.journal, &self.actor, &request, command)
            .unwrap()
    }
    fn register(&mut self, subject: &str, route: &str) {
        let c = self.execute(&Command::Register {
            subject: id(subject),
            permissions: permissions(route),
        });
        assert_eq!(c.operation.state, State::Succeeded);
        assert!(c.secret.is_none());
    }
    fn auth(&self) -> Authenticator {
        Authenticator::open(
            &self.team,
            id("gateway"),
            Reader::open(&self.audit).unwrap(),
        )
        .unwrap()
    }
    fn raw_db(&self, team: bool) -> rusqlite::Connection {
        rusqlite::Connection::open(if team {
            self.team.join("team.sqlite3")
        } else {
            self.audit.join("management.sqlite3")
        })
        .unwrap()
    }
}
#[test]
fn purpose_scope_rotation_and_authorization_versions_are_independent() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    f.register("bob", "model-b");
    let model = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("alice-model"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    let manage = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("alice-manage"),
            purpose: Purpose::Management,
        })
        .secret
        .unwrap();
    let read = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("alice-read"),
            purpose: Purpose::ReadOnly,
        })
        .secret
        .unwrap();
    let bob = f
        .execute(&Command::Issue {
            subject: id("bob"),
            credential: id("bob-model"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    let auth = f.auth();
    let alice = auth.authenticate_model(model.expose()).unwrap();
    let bob_claim = auth.authenticate_model(bob.expose()).unwrap();
    assert!(alice.permissions.permits_route("model-a"));
    assert!(!alice.permissions.permits_route("model-b"));
    assert!(!bob_claim.permissions.permits_route("model-a"));
    assert!(auth.authenticate(model.expose()).is_none());
    assert!(auth.authenticate_model(manage.expose()).is_none());
    assert!(auth.authenticate_model(read.expose()).is_none());
    let m = auth.authenticate(manage.expose()).unwrap();
    assert!(
        m.actor
            .authorize(Action::RuntimeStart, &id("gateway"))
            .is_ok()
    );
    let view = auth.authenticate(read.expose()).unwrap();
    assert!(
        view.actor
            .authorize(Action::ReadState, &id("gateway"))
            .is_ok()
    );
    assert!(
        view.actor
            .authorize(Action::RuntimeStart, &id("gateway"))
            .is_err()
    );
    let mut reduced = permissions("model-b");
    reduced.management.retain(|g| g.action == Action::ReadState);
    f.execute(&Command::PermissionsChange {
        subject: id("alice"),
        permissions: reduced,
    });
    assert!(
        auth.refresh(&view.actor.identity().clone(), &view.authorization_version)
            .is_none()
    );
    assert!(
        auth.refresh_model(&alice.identity, &alice.authorization_version)
            .is_none()
    );
    assert!(
        auth.refresh_model(&bob_claim.identity, &bob_claim.authorization_version)
            .is_some()
    );
    let current = auth.authenticate_model(model.expose()).unwrap();
    assert!(!current.permissions.permits_route("model-a"));
    assert!(current.permissions.permits_route("model-b"));
    let replacement = f
        .execute(&Command::Rotate {
            credential: id("alice-model"),
            replacement: id("alice-model-2"),
        })
        .secret
        .unwrap();
    assert!(auth.authenticate_model(model.expose()).is_none());
    assert!(auth.authenticate_model(replacement.expose()).is_some());
    assert!(auth.credential(&id("alice-model")).unwrap().revoked);
    f.execute(&Command::Revoke {
        credential: id("alice-model-2"),
    });
    assert!(auth.authenticate_model(replacement.expose()).is_none());
    assert!(f.auth().authenticate_model(replacement.expose()).is_none());
}
#[test]
fn issue_is_one_time_and_no_raw_secret_is_persisted_or_replayed() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let command = Command::Issue {
        subject: id("alice"),
        credential: id("alice-key"),
        purpose: Purpose::Model,
    };
    let request = f.request(&command);
    let first = f
        .manager
        .execute(&mut f.journal, &f.actor, &request, &command)
        .unwrap();
    let secret = first.secret.unwrap();
    assert_eq!(secret.credential_id(), &id("alice-key"));
    let replay = f
        .manager
        .execute(&mut f.journal, &f.actor, &request, &command)
        .unwrap();
    assert_eq!(replay.operation.id, first.operation.id);
    assert!(replay.secret.is_none());
    assert!(
        !serde_json::to_string(&replay.operation)
            .unwrap()
            .contains(secret.expose())
    );
    assert!(
        !serde_json::to_string(&f.manager.inventory().unwrap())
            .unwrap()
            .contains("verifier")
    );
    for root in [&f.team, &f.audit] {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let bytes = fs::read(path).unwrap();
                assert!(
                    !bytes
                        .windows(secret.expose().len())
                        .any(|part| part == secret.expose().as_bytes())
                );
            }
        }
    }
    let invalid = Request {
        parameters_sha256: Digest::of(b"different intent"),
        ..request
    };
    assert!(matches!(
        f.manager
            .execute(&mut f.journal, &f.actor, &invalid, &command),
        Err(Error::InvalidInput)
    ));
}
#[test]
fn preconditions_and_durable_intent_precede_all_team_effects() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let command = Command::Issue {
        subject: id("alice"),
        credential: id("alice-key"),
        purpose: Purpose::Model,
    };
    let stale = f.request(&command);
    f.register("bob", "model-b");
    let before = f.manager.snapshot().unwrap();
    assert!(matches!(
        f.manager
            .execute(&mut f.journal, &f.actor, &stale, &command),
        Err(Error::Conflict)
    ));
    assert_eq!(f.manager.snapshot().unwrap(), before);
    let db = f.raw_db(false);
    db.execute_batch("CREATE TRIGGER fail_intent BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let request = f.request(&command);
    assert!(matches!(
        f.manager
            .execute(&mut f.journal, &f.actor, &request, &command),
        Err(Error::Storage)
    ));
    assert_eq!(f.manager.snapshot().unwrap(), before);
    assert!(f.manager.inventory().unwrap().credentials.is_empty());
    db.execute_batch("DROP TRIGGER fail_intent; CREATE TRIGGER fail_start BEFORE INSERT ON events WHEN json_extract(NEW.event,'$.phase')='started' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let request = f.request(&command);
    assert!(
        f.manager
            .execute(&mut f.journal, &f.actor, &request, &command)
            .is_err()
    );
    assert!(f.manager.inventory().unwrap().credentials.is_empty());
}
struct Reconcile<'a>(&'a mut Manager);
impl Backend for Reconcile<'_> {
    fn prepare<'a>(
        &'a mut self,
        _: &'a Request,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        Err(Error::Unsupported)
    }
    fn reconcile(&mut self, operation: &Operation) -> gateway_management::Result<Effect> {
        self.0.reconcile(operation)
    }
}
#[test]
fn interrupted_issuance_remains_uncertain_and_never_redisplays_a_secret() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let db = f.raw_db(false);
    db.execute_batch("CREATE TRIGGER fail_result BEFORE INSERT ON events WHEN json_extract(NEW.event,'$.phase')='finished' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let command = Command::Issue {
        subject: id("alice"),
        credential: id("orphan"),
        purpose: Purpose::Model,
    };
    let request = f.request(&command);
    let error = f
        .manager
        .execute(&mut f.journal, &f.actor, &request, &command)
        .err()
        .unwrap();
    let Error::Uncertain(operation) = error else {
        panic!("expected uncertainty")
    };
    assert_eq!(
        f.manager.inventory().unwrap().credentials[0].issued_operation,
        operation
    );
    db.execute_batch("DROP TRIGGER fail_result").unwrap();
    let retry = f
        .manager
        .execute(&mut f.journal, &f.actor, &request, &command)
        .unwrap();
    assert_eq!(retry.operation.state, State::Uncertain);
    assert!(retry.secret.is_none());
    let reconciled = f
        .journal
        .reconcile(
            &f.actor,
            &id("gateway"),
            &operation,
            &mut Reconcile(&mut f.manager),
        )
        .unwrap();
    assert_eq!(reconciled.state, State::Succeeded);
    let retry = f
        .manager
        .execute(&mut f.journal, &f.actor, &request, &command)
        .unwrap();
    assert!(retry.secret.is_none());
    assert_eq!(retry.operation.state, State::Succeeded);
    let replacement = f
        .execute(&Command::Rotate {
            credential: id("orphan"),
            replacement: id("replacement"),
        })
        .secret
        .unwrap();
    assert!(f.auth().authenticate_model(replacement.expose()).is_some());
    assert!(f.auth().credential(&id("orphan")).unwrap().revoked);
}
#[test]
fn intermediate_rotation_failure_rolls_back_revocation_and_preserves_state() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let original = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("original"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    let auth = f.auth();
    assert!(auth.authenticate_model(original.expose()).is_some());
    let before = f.manager.snapshot().unwrap();
    let db = f.raw_db(true);
    db.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON credentials BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let failed = f.execute(&Command::Rotate {
        credential: id("original"),
        replacement: id("rejected"),
    });
    assert_eq!(failed.operation.state, State::Failed);
    assert!(failed.secret.is_none());
    assert_eq!(before, f.manager.snapshot().unwrap());
    assert!(auth.authenticate_model(original.expose()).is_some());
}
#[test]
fn audit_result_failure_does_not_restore_revoked_credentials_or_stop_existing_authority() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let a = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("a"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    let b = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("b"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    let auth = f.auth();
    assert!(auth.authenticate_model(a.expose()).is_some());
    assert!(auth.authenticate_model(b.expose()).is_some());
    f.raw_db(false).execute_batch("CREATE TRIGGER fail_result BEFORE INSERT ON events WHEN json_extract(NEW.event,'$.phase')='finished' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let command = Command::Revoke {
        credential: id("a"),
    };
    let request = f.request(&command);
    assert!(matches!(
        f.manager
            .execute(&mut f.journal, &f.actor, &request, &command),
        Err(Error::Uncertain(_))
    ));
    assert!(auth.authenticate_model(a.expose()).is_none());
    assert!(auth.authenticate_model(b.expose()).is_some());
}
#[test]
fn explicit_store_ownership_backup_schema_and_actor_boundaries() {
    let mut f = Fixture::new();
    assert!(matches!(
        Manager::open(&f.team, id("gateway"), MAXIMUM),
        Err(Error::AlreadyOwned)
    ));
    let denied = Actor::new(
        Identity {
            subject: id("outsider"),
            credential: id("forged"),
        },
        [],
    )
    .unwrap();
    let command = Command::Register {
        subject: id("alice"),
        permissions: permissions("model-a"),
    };
    let request = f.request(&command);
    assert!(matches!(
        f.manager
            .execute(&mut f.journal, &denied, &request, &command),
        Err(Error::Forbidden)
    ));
    assert!(f.manager.inventory().unwrap().subjects.is_empty());
    f.register("alice", "model-a");
    let backup = f._root.path().join("backup");
    private(&backup);
    let destination = backup.join("team.sqlite3");
    f.manager.backup(&destination).unwrap();
    assert!(f.manager.backup(&destination).is_err());
    let restored = Manager::open(&backup, id("gateway"), MAXIMUM).unwrap();
    assert_eq!(restored.snapshot().unwrap(), f.manager.snapshot().unwrap());
    drop(restored);
    assert!(
        Authenticator::open(&f.team, id("different"), Reader::open(&f.audit).unwrap()).is_err()
    );
    f.raw_db(true)
        .execute("UPDATE metadata SET schema='unsupported/v9'", [])
        .unwrap();
    assert!(matches!(
        Authenticator::open(&f.team, id("gateway"), Reader::open(&f.audit).unwrap()),
        Err(Error::UnsupportedSchema)
    ));
}
#[test]
fn credential_binding_and_wrong_audit_store_cannot_forge_issuance_proof() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let issued = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("original"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    let auth = f.auth();
    assert!(auth.authenticate_model(issued.expose()).is_some());
    let other = f._root.path().join("other-audit");
    private(&other);
    let _journal = Journal::initialize(&other, MAXIMUM).unwrap();
    let wrong = Authenticator::open(&f.team, id("gateway"), Reader::open(&other).unwrap()).unwrap();
    assert!(wrong.authenticate_model(issued.expose()).is_none());
    let db = f.raw_db(true);
    let text: String = db
        .query_row(
            "SELECT data FROM credentials WHERE id='original'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let forged = format!("gwt1_model_{}", "0".repeat(64));
    let verifier = Digest::of(forged.as_bytes());
    value["verifier"] = serde_json::to_value(&verifier).unwrap();
    db.execute(
        "UPDATE credentials SET verifier=?1,data=?2 WHERE id='original'",
        rusqlite::params![verifier.as_str(), serde_json::to_string(&value).unwrap()],
    )
    .unwrap();
    assert!(auth.authenticate_model(&forged).is_none());
    assert!(f.auth().authenticate_model(&forged).is_none());
    assert!(auth.authenticate_model(issued.expose()).is_none());
}
struct ReadHost;
impl gateway_management_api::Dispatcher for ReadHost {
    fn features(&self) -> Vec<gateway_management_api::Feature> {
        vec![]
    }
    fn supported(&self) -> Vec<Action> {
        vec![Action::ReadState]
    }
    fn snapshot(
        &mut self,
        _: &gateway_management_api::Command,
    ) -> gateway_management::Result<gateway_management::Snapshot> {
        Err(Error::Unsupported)
    }
    fn read(
        &mut self,
        _: &Actor,
        _: &gateway_management_api::Query,
    ) -> gateway_management::Result<serde_json::Value> {
        Ok(serde_json::json!({"schema":gateway_management_api::STATE_SCHEMA,"modules":[]}))
    }
    fn prepare<'a>(
        &'a mut self,
        _: &'a Request,
        _: &'a gateway_management_api::Command,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        Err(Error::Unsupported)
    }
    fn reconcile(&mut self, _: &Operation) -> gateway_management::Result<Effect> {
        Err(Error::Unsupported)
    }
}
#[tokio::test]
async fn actual_management_sessions_reject_model_keys_and_invalidate_on_rotation_and_permissions() {
    use axum::{
        Router,
        body::Body,
        http::{Request as HttpRequest, StatusCode, header},
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let read = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("view"),
            purpose: Purpose::ReadOnly,
        })
        .secret
        .unwrap();
    let model = f
        .execute(&Command::Issue {
            subject: id("alice"),
            credential: id("model"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
    // Only session transport needs its own disposable journal here. Issuance evidence remains
    // in the separate fixture's real journal; product single-writer assembly is tested separately.
    let transport = f._root.path().join("transport");
    private(&transport);
    let journal = Journal::initialize(&transport, MAXIMUM).unwrap();
    let reader = Reader::open(&transport).unwrap();
    let router = gateway_management_api::Service::new(
        id("gateway"),
        "127.0.0.1:47310".parse().unwrap(),
        Arc::new(f.auth()),
        journal,
        reader,
        Box::new(ReadHost),
        true,
    )
    .unwrap()
    .router();
    async fn login(router: &Router, secret: &str) -> axum::response::Response {
        router
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/management/v1/session")
                    .header(header::HOST, "127.0.0.1:47310")
                    .header(header::ORIGIN, "http://127.0.0.1:47310")
                    .header(header::AUTHORIZATION, format!("Bearer {secret}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn read_state(router: &Router, cookie: &str) -> StatusCode {
        router
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/management/v1/state?target=gateway")
                    .header(header::HOST, "127.0.0.1:47310")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }
    assert_eq!(
        login(&router, model.expose()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = login(&router, read.expose()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(read_state(&router, &cookie).await, StatusCode::OK);
    let next = f
        .execute(&Command::Rotate {
            credential: id("view"),
            replacement: id("view-next"),
        })
        .secret
        .unwrap();
    assert_eq!(read_state(&router, &cookie).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        login(&router, read.expose()).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let response = login(&router, next.expose()).await;
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(read_state(&router, &cookie).await, StatusCode::OK);
    let mut reduced = permissions("model-a");
    reduced.management.clear();
    f.execute(&Command::PermissionsChange {
        subject: id("alice"),
        permissions: reduced,
    });
    assert_eq!(read_state(&router, &cookie).await, StatusCode::UNAUTHORIZED);
}
#[test]
fn team_writer_remains_locked_while_audit_admission_waits() {
    let mut f = Fixture::new();
    f.register("alice", "model-a");
    let command = Command::Issue {
        subject: id("alice"),
        credential: id("concurrent-key"),
        purpose: Purpose::Model,
    };
    let request = f.request(&command);
    let audit = f.raw_db(false);
    audit.execute_batch("BEGIN IMMEDIATE").unwrap();
    let competitor = f.raw_db(true);
    competitor.busy_timeout(std::time::Duration::ZERO).unwrap();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            f.manager
                .execute(&mut f.journal, &f.actor, &request, &command)
        });
        let mut held = false;
        for _ in 0..100 {
            match competitor.execute_batch("BEGIN IMMEDIATE") {
                Ok(()) => {
                    competitor.execute_batch("ROLLBACK").unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error)
                    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy) =>
                {
                    held = true;
                    break;
                }
                Err(error) => panic!("unexpected SQLite fixture error: {error}"),
            }
        }
        audit.execute_batch("ROLLBACK").unwrap();
        let result = worker.join().unwrap().unwrap();
        assert!(held);
        assert_eq!(result.operation.state, State::Succeeded);
        assert!(result.secret.is_some());
    });
}
