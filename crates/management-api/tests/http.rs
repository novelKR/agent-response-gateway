use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request as HttpRequest, StatusCode, header},
};
use gateway_management::{
    Action, Actor, Digest, Effect, FailureCode, Grant, Id, Identity, Journal, Operation,
    PreparedOperation, Reader, Request, Snapshot,
};
use gateway_management_api::{
    Authenticator, Command, CredentialKind, Dispatcher, Feature, Principal, Query, SCHEMA, Service,
    Submission,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tower::ServiceExt;
const MANAGEMENT: &str = "synthetic-management-token-01234567890123456789";
const READ: &str = "synthetic-readonly-token-01234567890123456789012";
fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn snapshot(revision: u64) -> Snapshot {
    Snapshot {
        revision,
        digest: Digest::of(&revision.to_be_bytes()),
    }
}
fn actions() -> Vec<Action> {
    vec![
        Action::ReadState,
        Action::ReadUsage,
        Action::ReadOperations,
        Action::RuntimeStart,
        Action::RuntimeStop,
        Action::Reconcile,
    ]
}
struct Auth {
    generation: AtomicU64,
    can_write: AtomicBool,
}
impl Auth {
    fn principal(&self, read: bool) -> Principal {
        let grants = actions()
            .into_iter()
            .chain([Action::RuntimeRestart])
            .filter(|a| {
                !read && self.can_write.load(Ordering::SeqCst)
                    || matches!(
                        a,
                        Action::ReadState | Action::ReadUsage | Action::ReadOperations
                    )
            })
            .map(|action| Grant {
                action,
                target: id("gateway"),
            });
        Principal {
            actor: Actor::new(
                Identity {
                    subject: id(if read { "reader" } else { "operator" }),
                    credential: id(if read { "read-key" } else { "management-key" }),
                },
                grants,
            )
            .unwrap(),
            kind: if read {
                CredentialKind::ReadOnly
            } else {
                CredentialKind::Management
            },
            authorization_version: Digest::of(
                &self.generation.load(Ordering::SeqCst).to_be_bytes(),
            ),
        }
    }
}
impl Authenticator for Auth {
    fn authenticate(&self, token: &str) -> Option<Principal> {
        match token {
            READ => Some(self.principal(true)),
            MANAGEMENT => Some(self.principal(false)),
            _ => None,
        }
    }
    fn refresh(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        let p = self.principal(identity.credential == id("read-key"));
        (p.actor.identity() == identity && p.authorization_version == *version).then_some(p)
    }
}
struct Control {
    revision: AtomicU64,
    effects: AtomicU64,
    entered: AtomicBool,
    block: Mutex<bool>,
    released: Condvar,
}
struct FixtureBackend(Arc<Control>);
struct Prepared {
    control: Arc<Control>,
    before: Snapshot,
}
impl PreparedOperation for Prepared {
    fn before(&self) -> &Snapshot {
        &self.before
    }
    fn apply(&mut self) -> Effect {
        self.control.entered.store(true, Ordering::SeqCst);
        let mut block = self.control.block.lock().unwrap();
        while *block {
            block = self.control.released.wait(block).unwrap();
        }
        drop(block);
        let next = self.control.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.control.effects.fetch_add(1, Ordering::SeqCst);
        Effect::Applied {
            after: snapshot(next),
            evidence_sha256: Digest::of(b"synthetic observed effect"),
        }
    }
}
impl Dispatcher for FixtureBackend {
    fn supported(&self) -> Vec<Action> {
        actions()
    }
    fn features(&self) -> Vec<Feature> {
        vec![Feature {
            id: id("synthetic"),
            version: "fixture/v1".into(),
            installed: true,
            enabled: true,
            operations: actions(),
        }]
    }
    fn snapshot(&mut self, _: &Command) -> gateway_management::Result<Snapshot> {
        Ok(snapshot(self.0.revision.load(Ordering::SeqCst)))
    }
    fn read(&mut self, actor: &Actor, query: &Query) -> gateway_management::Result<Value> {
        Ok(match query {
            Query::State => {
                json!({"schema":gateway_management_api::STATE_SCHEMA,"modules":[{"id":"synthetic","contract":"synthetic-state/v1","observation":{"state":"observed","observed_at_ms":1,"data":{"schema":"synthetic-state/v1","snapshot":snapshot(self.0.revision.load(Ordering::SeqCst)),"runtime_observed":false}}}]})
            }
            Query::Usage(_) => {
                json!({"subject":actor.identity().subject,"observed":false,"tokens":null})
            }
            Query::Continuation(_) => return Err(gateway_management::Error::NotFound),
        })
    }
    fn prepare<'a>(
        &'a mut self,
        request: &'a Request,
        command: &'a Command,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        if request.action != command.action()
            || request.parameters_sha256 != command.digest()?
            || !matches!(command, Command::RuntimeStart {} | Command::RuntimeStop {})
        {
            return Err(gateway_management::Error::InvalidInput);
        }
        Ok(Box::new(Prepared {
            control: self.0.clone(),
            before: snapshot(self.0.revision.load(Ordering::SeqCst)),
        }))
    }
    fn reconcile(&mut self, _: &Operation) -> gateway_management::Result<Effect> {
        Ok(Effect::Uncertain {
            code: FailureCode::Unverified,
        })
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    service: Arc<Service>,
    auth: Arc<Auth>,
    control: Arc<Control>,
    authority: String,
}
impl Fixture {
    fn new(authority: &str, web: bool) -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/api-tests");
        std::fs::create_dir_all(&base).unwrap();
        let directory = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let journal = Journal::initialize(directory.path(), 8 * 1024 * 1024).unwrap();
        let reader = Reader::open(directory.path()).unwrap();
        let auth = Arc::new(Auth {
            generation: AtomicU64::new(0),
            can_write: AtomicBool::new(true),
        });
        let control = Arc::new(Control {
            revision: AtomicU64::new(0),
            effects: AtomicU64::new(0),
            entered: AtomicBool::new(false),
            block: Mutex::new(false),
            released: Condvar::new(),
        });
        let service = Service::new(
            id("gateway"),
            authority.parse().unwrap(),
            auth.clone(),
            journal,
            reader,
            Box::new(FixtureBackend(control.clone())),
            web,
        )
        .unwrap();
        Self {
            _directory: directory,
            service,
            auth,
            control,
            authority: authority.into(),
        }
    }
    fn router(&self) -> Router {
        self.service.router()
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        value: Option<Value>,
    ) -> HttpRequest<Body> {
        let mut request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, &self.authority);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if value.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        request
            .body(Body::from(value.map(|v| v.to_string()).unwrap_or_default()))
            .unwrap()
    }
    fn submission(&self, key: &str) -> Submission {
        Submission {
            schema: SCHEMA.into(),
            target: id("gateway"),
            expected: snapshot(self.control.revision.load(Ordering::SeqCst)),
            idempotency_key: id(key),
            command: Command::RuntimeStart {},
        }
    }
    fn release(&self) {
        *self.control.block.lock().unwrap() = false;
        self.control.released.notify_all();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.release();
    }
}
async fn json_response(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}
async fn wait_effects(f: &Fixture, count: u64) {
    for _ in 0..500 {
        if f.control.effects.load(Ordering::SeqCst) == count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    panic!("fixture effect did not finish");
}
#[tokio::test]
async fn authentication_host_origin_and_closed_commands_are_enforced() {
    let f = Fixture::new("127.0.0.1:41001", true);
    assert_eq!(
        f.router()
            .oneshot(f.request("GET", "/management/v1/state?target=gateway", None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut wrong = f.request(
        "GET",
        "/management/v1/state?target=gateway",
        Some(READ),
        None,
    );
    wrong
        .headers_mut()
        .insert(header::HOST, "example.invalid".parse().unwrap());
    assert_eq!(
        f.router().oneshot(wrong).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let mut cross = f.request(
        "GET",
        "/management/v1/state?target=gateway",
        Some(READ),
        None,
    );
    cross
        .headers_mut()
        .insert(header::ORIGIN, "http://example.invalid".parse().unwrap());
    assert_eq!(
        f.router().oneshot(cross).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let mut duplicate = f.request(
        "GET",
        "/management/v1/state?target=gateway",
        Some(READ),
        None,
    );
    duplicate.headers_mut().append(
        header::AUTHORIZATION,
        format!("Bearer {MANAGEMENT}").parse().unwrap(),
    );
    assert_eq!(
        f.router().oneshot(duplicate).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let mut body = serde_json::to_value(f.submission("forged")).unwrap();
    body["subject"] = json!("operator");
    body["role"] = json!("owner");
    assert_eq!(
        f.router()
            .oneshot(f.request("POST", "/management/v1/operations", Some(READ), Some(body)))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 0);
    let mut unsupported = f.submission("unsupported");
    unsupported.command = Command::RuntimeRestart {};
    assert_eq!(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/operations",
                Some(MANAGEMENT),
                Some(serde_json::to_value(unsupported).unwrap())
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_IMPLEMENTED
    );
    let caps = json_response(
        f.router()
            .oneshot(f.request(
                "GET",
                "/management/v1/capabilities?target=gateway",
                Some(READ),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(
        !caps["data"]["allowed_operations"]
            .as_array()
            .unwrap()
            .contains(&json!("runtime_start"))
    );
    assert!(
        caps["data"]["unsupported_operations"]
            .as_array()
            .unwrap()
            .contains(&json!("package_remove"))
    );
}
#[tokio::test]
async fn read_sessions_cannot_mutate_and_refresh_rejects_permission_version_changes() {
    let f = Fixture::new("127.0.0.1:41002", true);
    let mut login = f.request("POST", "/management/v1/session", Some(READ), None);
    login.headers_mut().insert(
        header::ORIGIN,
        format!("http://{}", f.authority).parse().unwrap(),
    );
    let result = f.router().oneshot(login).await.unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let cookie = result.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(!cookie.contains(READ));
    let cookie = cookie.split(';').next().unwrap().to_owned();
    let request = || {
        let mut request = f.request("GET", "/management/v1/state?target=gateway", None, None);
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
        request
    };
    assert_eq!(
        f.router().oneshot(request()).await.unwrap().status(),
        StatusCode::OK
    );
    let mut mutation = f.request(
        "POST",
        "/management/v1/operations",
        None,
        Some(serde_json::to_value(f.submission("cookie-mutation")).unwrap()),
    );
    mutation
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().unwrap());
    assert_eq!(
        f.router().oneshot(mutation).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let mut wrong_purpose = f.request("POST", "/management/v1/session", Some(MANAGEMENT), None);
    wrong_purpose.headers_mut().insert(
        header::ORIGIN,
        format!("http://{}", f.authority).parse().unwrap(),
    );
    assert_eq!(
        f.router().oneshot(wrong_purpose).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    f.auth.generation.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        f.router().oneshot(request()).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn durable_admission_is_queryable_before_effect_and_duplicates_do_not_repeat() {
    let f = Fixture::new("127.0.0.1:41003", false);
    *f.control.block.lock().unwrap() = true;
    let body = serde_json::to_value(f.submission("once")).unwrap();
    let response = f
        .router()
        .oneshot(f.request(
            "POST",
            "/management/v1/operations",
            Some(MANAGEMENT),
            Some(body.clone()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let id = json_response(response).await["data"]["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/management/v1/operations/{id}?target=gateway");
    let op = json_response(
        f.router()
            .oneshot(f.request("GET", &path, Some(READ), None))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(op["data"]["operation"]["id"], id);
    assert!(matches!(
        op["data"]["operation"]["state"].as_str(),
        Some("queued" | "running")
    ));
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 0);
    f.release();
    wait_effects(&f, 1).await;
    let duplicate = json_response(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/operations",
                Some(MANAGEMENT),
                Some(body),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(duplicate["data"]["operation_id"], id);
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 1);
    let mut conflict = f.submission("once");
    conflict.command = Command::RuntimeStop {};
    assert_eq!(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/operations",
                Some(MANAGEMENT),
                Some(serde_json::to_value(conflict).unwrap())
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.router()
            .oneshot(f.request("POST", "/management/v1/session", Some(READ), None))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn preflight_has_no_effect_and_is_not_later_authorization() {
    let f = Fixture::new("127.0.0.1:41004", false);
    let preflight = json!({"schema":SCHEMA,"target":"gateway","idempotency_key":"review","command":{"kind":"runtime_start"}});
    let result = json_response(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/preflight",
                Some(MANAGEMENT),
                Some(preflight),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(result["data"]["applied"], false);
    assert_eq!(result["data"]["authorized_execution"], false);
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 0);
    f.auth.can_write.store(false, Ordering::SeqCst);
    assert_eq!(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/operations",
                Some(MANAGEMENT),
                Some(result["data"]["submission"].clone())
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 0);
    let rows = json_response(
        f.router()
            .oneshot(f.request(
                "GET",
                "/management/v1/operations?target=gateway",
                Some(READ),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(rows["data"]["items"], json!([]));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cli_talks_to_real_http_without_proxy_or_secret_output() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let f = Fixture::new(&address.to_string(), false);
    let router = f.router();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_gateway-management-cli"));
    command
        .env_clear()
        .env("GATEWAY_MANAGEMENT_TOKEN", READ)
        .env("HTTP_PROXY", "http://example.invalid:1")
        .args([
            "--endpoint",
            &format!("http://{address}"),
            "state",
            "--target",
            "gateway",
        ]);
    #[cfg(windows)]
    command.env("SYSTEMROOT", std::env::var_os("SYSTEMROOT").unwrap());
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(READ));
    assert!(!text.contains(MANAGEMENT));
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schema"], SCHEMA);
    assert_eq!(
        value["data"]["modules"][0]["observation"]["data"]["runtime_observed"],
        false
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_work_refreshes_authority_after_obtaining_the_writer() {
    let f = Fixture::new("127.0.0.1:41005", false);
    *f.control.block.lock().unwrap() = true;
    let first = f
        .router()
        .oneshot(f.request(
            "POST",
            "/management/v1/operations",
            Some(MANAGEMENT),
            Some(serde_json::to_value(f.submission("first")).unwrap()),
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let request = f.request(
        "POST",
        "/management/v1/operations",
        Some(MANAGEMENT),
        Some(serde_json::to_value(f.submission("queued")).unwrap()),
    );
    let router = f.router();
    let waiting = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
    tokio::task::yield_now().await;
    f.auth.can_write.store(false, Ordering::SeqCst);
    f.release();
    assert_eq!(waiting.await.unwrap().status(), StatusCode::FORBIDDEN);
    wait_effects(&f, 1).await;
    let rows = json_response(
        f.router()
            .oneshot(f.request(
                "GET",
                "/management/v1/operations?target=gateway",
                Some(READ),
                None,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(rows["data"]["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn stale_preflight_is_rejected_when_target_state_changes() {
    let f = Fixture::new("127.0.0.1:41006", false);
    let stale = serde_json::to_value(f.submission("stale")).unwrap();
    let response = f
        .router()
        .oneshot(f.request(
            "POST",
            "/management/v1/operations",
            Some(MANAGEMENT),
            Some(serde_json::to_value(f.submission("advance")).unwrap()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    wait_effects(&f, 1).await;
    assert_eq!(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/operations",
                Some(MANAGEMENT),
                Some(stale)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn audit_failure_prevents_effect_and_missing_results_are_not_reported_as_live_work() {
    let f = Fixture::new("127.0.0.1:41007", false);
    let db = rusqlite::Connection::open(f._directory.path().join("management.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_admit BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    let response = f
        .router()
        .oneshot(f.request(
            "POST",
            "/management/v1/operations",
            Some(MANAGEMENT),
            Some(serde_json::to_value(f.submission("denied")).unwrap()),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 0);
    db.execute_batch("DROP TRIGGER fail_admit; CREATE TRIGGER fail_result BEFORE INSERT ON events WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    let response = json_response(
        f.router()
            .oneshot(f.request(
                "POST",
                "/management/v1/operations",
                Some(MANAGEMENT),
                Some(serde_json::to_value(f.submission("missing-result")).unwrap()),
            ))
            .await
            .unwrap(),
    )
    .await;
    let operation = response["data"]["operation_id"]
        .as_str()
        .or_else(|| response["operation_id"].as_str())
        .unwrap();
    let path = format!("/management/v1/operations/{operation}?target=gateway");
    let mut view = Value::Null;
    for _ in 0..500 {
        view = json_response(
            f.router()
                .oneshot(f.request("GET", &path, Some(READ), None))
                .await
                .unwrap(),
        )
        .await;
        if view["data"]["observed_state"] == "uncertain" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert_eq!(view["data"]["observed_state"], "uncertain");
    assert_eq!(view["data"]["uncertainty"], "result_record_missing");
    assert_eq!(
        view["data"]["operation"]["state"], "running",
        "the original durable record is not rewritten by a read"
    );
    assert_eq!(f.control.effects.load(Ordering::SeqCst), 1);
    db.execute_batch("DROP TRIGGER fail_result").unwrap();
    let result = f
        .router()
        .oneshot(f.request(
            "POST",
            &format!("/management/v1/operations/{operation}/reconcile"),
            Some(MANAGEMENT),
            Some(json!({"schema":SCHEMA,"target":"gateway"})),
        ))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(
        json_response(result).await["data"]["state"],
        "uncertain",
        "unverified synthetic evidence is never promoted to success"
    );
}
