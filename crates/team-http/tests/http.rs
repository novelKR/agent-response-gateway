use agent_response_gateway::{Config, Secrets};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::StreamExt;
use gateway_management::{Action, Actor, Digest, Grant, Id, Identity, Journal, Reader, Request};
use gateway_team_access::{Authenticator, Command, Manager, Permissions, Purpose, Secret};
use gateway_team_http::{Ledger, Limits, Peer, PeerSource, Route, Service, SqliteUsage};
use gateway_usage_contract::{EventKind, Finality, Outcome, Profile, UsageEvent};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::task::JoinHandle;
const LOCAL: &str = "synthetic-gateway-local-key-01234567890123456789";
const PROVIDER: &str = "synthetic-provider-key-not-a-team-key";
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn private(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
struct Accounts {
    root: tempfile::TempDir,
    manager: Manager,
    journal: Journal,
    admin: Actor,
    auth: Arc<Authenticator>,
    alice: Secret,
    bob: Secret,
    operator: Secret,
    serial: u64,
}
impl Accounts {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/team-http-tests");
        private(&base);
        let root = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
        let team = root.path().join("credentials");
        let audit = root.path().join("audit");
        private(&team);
        private(&audit);
        let mut manager = Manager::initialize(&team, id("gateway"), 8 * 1024 * 1024).unwrap();
        let mut journal = Journal::initialize(&audit, 8 * 1024 * 1024).unwrap();
        let admin = Actor::new(
            Identity {
                subject: id("bootstrap"),
                credential: id("local-admin"),
            },
            [
                Action::TeamSubjectRegister,
                Action::TeamCredentialIssue,
                Action::TeamCredentialRevoke,
                Action::TeamPermissionsChange,
            ]
            .into_iter()
            .map(|action| Grant {
                action,
                target: id("gateway"),
            }),
        )
        .unwrap();
        let mut serial = 0u64;
        let mut run = |command: Command| {
            serial += 1;
            let request = Request {
                target: id("gateway"),
                action: command.action(),
                expected: manager.snapshot().unwrap(),
                idempotency_key: id(&format!("setup-{serial}")),
                parameters_sha256: command.digest().unwrap(),
            };
            manager
                .execute(&mut journal, &admin, &request, &command)
                .unwrap()
        };
        for (subject, route, all) in [
            ("alice", "a", false),
            ("bob", "b", false),
            ("operator", "a", true),
        ] {
            run(Command::Register {
                subject: id(subject),
                permissions: Permissions {
                    enabled: true,
                    routes: BTreeSet::from([route.to_owned()]),
                    management: BTreeSet::new(),
                    read_all_usage: all,
                },
            });
        }
        let alice = run(Command::Issue {
            subject: id("alice"),
            credential: id("alice-key"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
        let bob = run(Command::Issue {
            subject: id("bob"),
            credential: id("bob-key"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
        let operator = run(Command::Issue {
            subject: id("operator"),
            credential: id("operator-key"),
            purpose: Purpose::Model,
        })
        .secret
        .unwrap();
        let auth = Arc::new(
            Authenticator::open(&team, id("gateway"), Reader::open(&audit).unwrap()).unwrap(),
        );
        Self {
            root,
            manager,
            journal,
            admin,
            auth,
            alice,
            bob,
            operator,
            serial,
        }
    }
    fn ledger(&self) -> (Ledger, PathBuf) {
        let path = self.root.path().join("requests");
        private(&path);
        (
            Ledger::initialize(&path, id("gateway"), 32 * 1024 * 1024).unwrap(),
            path,
        )
    }
    fn revoke_alice(&mut self) {
        self.serial += 1;
        let command = Command::Revoke {
            credential: id("alice-key"),
        };
        let request = Request {
            target: id("gateway"),
            action: command.action(),
            expected: self.manager.snapshot().unwrap(),
            idempotency_key: id(&format!("change-{}", self.serial)),
            parameters_sha256: command.digest().unwrap(),
        };
        self.manager
            .execute(&mut self.journal, &self.admin, &request, &command)
            .unwrap();
    }
}
struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, task }
}
#[derive(Default)]
struct Mock {
    calls: AtomicUsize,
    authorized: AtomicUsize,
    cancelled: AtomicBool,
}
struct DropFlag(Arc<Mock>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
    }
}
async fn provider(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    mock.calls.fetch_add(1, Ordering::SeqCst);
    if headers.get("authorization").and_then(|h| h.to_str().ok())
        == Some(format!("Bearer {PROVIDER}").as_str())
    {
        mock.authorized.fetch_add(1, Ordering::SeqCst);
    }
    assert!(headers.get("x-team-session").is_none());
    assert!(headers.get("x-gateway-session").is_none());
    if body["input"] == "delay" {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if body["stream"] == true {
        let hold = body["input"] == "hold";
        let flag = DropFlag(mock);
        let stream = futures_util::stream::unfold((0, flag), move |(n, flag)| async move {
            if n == 0 {
                Some((Ok::<Bytes,std::io::Error>(Bytes::from_static(b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"synthetic\",\"status\":\"in_progress\"}}\n\n")),(1,flag)))
            } else if hold {
                std::future::pending().await
            } else if n == 1 {
                Some((Ok(Bytes::from_static(b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"synthetic\",\"status\":\"completed\",\"output\":[]}}\n\n")),(2,flag)))
            } else {
                None
            }
        });
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap();
    }
    Json(json!({"id":"synthetic","object":"response","status":"completed","output":[],"model":body["model"]})).into_response()
}
fn config(url: &str) -> Config {
    Config::parse(&format!("listen='127.0.0.1:0'\n[providers.mock]\nbase_url='{url}/v1'\napi_key_env='SYNTHETIC_PROVIDER_KEY'\n[models.a]\nprovider='mock'\nupstream_model='native-a'\n[models.b]\nprovider='mock'\nupstream_model='native-b'\n")).unwrap()
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}
async fn team(
    accounts: &Accounts,
    peers: Arc<dyn PeerSource>,
    ledger: Ledger,
    usage: Option<Box<dyn gateway_team_http::UsageReader>>,
    limits: Limits,
) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let service = Service::new(bound, accounts.auth.clone(), peers, ledger, usage, limits).unwrap();
    let router = service.router();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server {
        url: format!("http://{bound}"),
        task,
    }
}
async fn report(client: &reqwest::Client, server: &Server, secret: &Secret, all: bool) -> Value {
    let url = format!(
        "{}/team/v1/usage?from_ms={}&to_ms={}&all={all}",
        server.url,
        now() - 60000,
        now() + 60000
    );
    let r = client
        .get(url)
        .bearer_auth(secret.expose())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    r.json().await.unwrap()
}
fn peer(url: &str, configuration: Digest, producer: Option<Id>) -> Peer {
    Peer::new(
        gateway_team_http::PeerIdentity {
            target: id("gateway"),
            instance: id("synthetic-instance"),
            configuration_sha256: configuration,
            producer,
        },
        url,
        BTreeMap::from([
            ("a".into(), Route { managed: None }),
            ("b".into(), Route { managed: None }),
        ]),
        LOCAL.into(),
        None,
    )
    .unwrap()
}
#[tokio::test]
async fn real_core_enforces_stateless_routes_credentials_and_exact_usage_scope() {
    let mut accounts = Accounts::new();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/v1/responses", post(provider))
            .with_state(mock.clone()),
    )
    .await;
    let cfg = config(&upstream.url);
    let configuration =
        Digest::try_from(cfg.manifest().unwrap().configuration_sha256().to_owned()).unwrap();
    let gateway = serve(
        agent_response_gateway::router(
            cfg,
            Secrets {
                local_token: LOCAL.into(),
                upstream_keys: BTreeMap::from([("mock".into(), PROVIDER.into())]),
            },
        )
        .unwrap(),
    )
    .await;
    let usage_dir = accounts.root.path().join("recorder");
    private(&usage_dir);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut recorder = gateway_usage_recorder::Store::open(&usage_dir, true, true).unwrap();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let producer_id = Some(id(&recorder.producer().unwrap()));
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let producer_id: Option<Id> = None;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let usage: Option<Box<dyn gateway_team_http::UsageReader>> =
        Some(Box::new(SqliteUsage::open(&usage_dir).unwrap()));
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let usage: Option<Box<dyn gateway_team_http::UsageReader>> = None;
    let (ledger, ledger_path) = accounts.ledger();
    let endpoint = team(
        &accounts,
        Arc::new(peer(
            &gateway.url,
            configuration.clone(),
            producer_id.clone(),
        )),
        ledger,
        usage,
        Limits::default(),
    )
    .await;
    let client = client();
    let models: Value = client
        .get(format!("{}/v1/models", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"].as_array().unwrap().len(), 1);
    assert_eq!(models["data"][0]["id"], "a");
    let rejected = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"b","input":"synthetic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    for extra in [
        json!({"previous_response_id":"other-subject-response"}),
        json!({"conversation":"other-subject-conversation"}),
    ] {
        let mut body = json!({"model":"a","input":"synthetic"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let r = client
            .post(format!("{}/v1/responses", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let _: Value = r.json().await.unwrap();
    }
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    let mut observed = Vec::new();
    for (secret, model) in [(&accounts.alice, "a"), (&accounts.bob, "b")] {
        let r = client
            .post(format!("{}/v1/responses", endpoint.url))
            .bearer_auth(secret.expose())
            .json(&json!({"model":model,"input":"synthetic-private-body"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let request = r.headers()["x-request-id"].to_str().unwrap().to_owned();
        let _: Value = r.json().await.unwrap();
        observed.push((request, model));
    }
    assert_eq!(mock.authorized.load(Ordering::SeqCst), 2);
    let first = report(&client, &endpoint, &accounts.alice, false).await;
    assert!(
        first["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["record"]["admission"]["subject"] == "alice")
    );
    assert!(
        first["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["usage"]["state"]
                == if producer_id.is_some() {
                    "unobserved"
                } else {
                    "unattributed"
                })
    );
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let rec_config = gateway_usage_contract::RecorderConfig {
            schema: "gateway-usage-recorder-config/v1".into(),
            destinations: vec![],
        };
        for (index, (request, model)) in observed.iter().enumerate() {
            let event = UsageEvent {
                schema: gateway_usage_contract::SCHEMA.into(),
                producer_id: producer_id.as_ref().unwrap().to_string(),
                request_id: request.clone(),
                attempt_id: format!("attempt-{index}"),
                event_id: format!("event-{index}"),
                revision: 1,
                kind: EventKind::AttemptFinished,
                started_at_ms: now(),
                observed_at_ms: now(),
                provider: "mock".into(),
                model_alias: (*model).into(),
                upstream_model: format!("native-{model}"),
                reported_model: None,
                provider_request_id: None,
                provider_response_id: None,
                profile: Profile::ResponsesV1,
                configuration_sha256: configuration.as_str().into(),
                upstream: Outcome::Incomplete,
                gateway: Outcome::TransportLost,
                finality: Finality::Partial,
                observation_incomplete: true,
                usage: gateway_usage_contract::normalize(
                    Profile::ResponsesV1,
                    gateway_usage_contract::extract(
                        Profile::ResponsesV1,
                        &json!({"input_tokens":42}),
                    ),
                ),
            };
            recorder.record(&event, &rec_config).unwrap();
        }
        let alice = report(&client, &endpoint, &accounts.alice, false).await;
        let observed_alice: Vec<_> = alice["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["usage"]["state"] == "observed")
            .collect();
        assert_eq!(observed_alice.len(), 1);
        assert_eq!(
            observed_alice[0]["usage"]["attempts"][0]["finality"],
            "partial"
        );
        assert!(
        observed_alice[0]["usage"]["attempts"][0]["usage"]["counters"]["output_tokens"]["value"]
            .is_null()
    );
    }
    let bob = report(&client, &endpoint, &accounts.bob, false).await;
    assert_eq!(bob["requests"].as_array().unwrap().len(), 1);
    assert_eq!(bob["requests"][0]["record"]["admission"]["subject"], "bob");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert_eq!(
        bob["requests"][0]["usage"]["attempts"][0]["model_alias"],
        "b"
    );
    let all = report(&client, &endpoint, &accounts.operator, true).await;
    assert_eq!(all["requests"].as_array().unwrap().len(), 4);
    let denied = client
        .get(format!(
            "{}/team/v1/usage?from_ms={}&to_ms={}&all=true",
            endpoint.url,
            now() - 60000,
            now() + 60000
        ))
        .bearer_auth(accounts.alice.expose())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    for suffix in ["", "-wal"] {
        let p = ledger_path.join(format!("team-requests.sqlite3{suffix}"));
        if p.exists() {
            let text = std::fs::read(p).unwrap();
            for secret in [
                accounts.alice.expose(),
                LOCAL,
                PROVIDER,
                "synthetic-private-body",
            ] {
                assert!(!text.windows(secret.len()).any(|v| v == secret.as_bytes()));
            }
        }
    }
    accounts.revoke_alice();
    assert_eq!(
        client
            .get(format!("{}/v1/models", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn streaming_eof_and_client_cancellation_are_transport_only() {
    let accounts = Accounts::new();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/v1/responses", post(provider))
            .with_state(mock.clone()),
    )
    .await;
    let cfg = config(&upstream.url);
    let digest =
        Digest::try_from(cfg.manifest().unwrap().configuration_sha256().to_owned()).unwrap();
    let gateway = serve(
        agent_response_gateway::router(
            cfg,
            Secrets {
                local_token: LOCAL.into(),
                upstream_keys: BTreeMap::from([("mock".into(), PROVIDER.into())]),
            },
        )
        .unwrap(),
    )
    .await;
    let (ledger, _) = accounts.ledger();
    let endpoint = team(
        &accounts,
        Arc::new(peer(&gateway.url, digest, None)),
        ledger,
        None,
        Limits::default(),
    )
    .await;
    let client = client();
    let stream = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"a","input":"synthetic","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    let text = stream.text().await.unwrap();
    assert!(text.contains("response.created") && text.contains("response.completed"));
    mock.cancelled.store(false, Ordering::SeqCst);
    let response = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"a","input":"hold","stream":true}))
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream();
    assert!(!stream.next().await.unwrap().unwrap().is_empty());
    drop(stream);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !mock.cancelled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let data = report(&client, &endpoint, &accounts.alice, false).await;
    let rows = data["requests"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["record"]["finished"]["transport"], "eof");
    assert_eq!(
        rows[1]["record"]["finished"]["transport"],
        "client_disconnected"
    );
    assert!(rows.iter().all(|r| r["usage"]["state"] == "unattributed"));
}
#[tokio::test]
async fn missing_headers_connection_loss_and_ledger_failure_never_invent_attribution() {
    let accounts = Accounts::new();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/v1/responses", post(provider))
            .with_state(mock.clone()),
    )
    .await;
    let cfg = config(&upstream.url);
    let digest =
        Digest::try_from(cfg.manifest().unwrap().configuration_sha256().to_owned()).unwrap();
    let gateway = serve(
        agent_response_gateway::router(
            cfg,
            Secrets {
                local_token: LOCAL.into(),
                upstream_keys: BTreeMap::from([("mock".into(), PROVIDER.into())]),
            },
        )
        .unwrap(),
    )
    .await;
    let (ledger, path) = accounts.ledger();
    let endpoint = team(
        &accounts,
        Arc::new(peer(&gateway.url, digest, Some(id("registered-producer")))),
        ledger,
        None,
        Limits {
            header_timeout: Duration::from_millis(40),
            ..Limits::default()
        },
    )
    .await;
    let client = client();
    let response = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"a","input":"delay"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let data = report(&client, &endpoint, &accounts.alice, false).await;
    assert_eq!(
        data["requests"][0]["record"]["finished"]["transport"],
        "header_timeout"
    );
    assert_eq!(data["requests"][0]["usage"]["state"], "unattributed");
    let db = rusqlite::Connection::open(path.join("team-requests.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_request BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let before = mock.calls.load(Ordering::SeqCst);
    let response = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"a","input":"synthetic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(mock.calls.load(Ordering::SeqCst), before);
    db.execute_batch("DROP TRIGGER fail_request; CREATE TRIGGER fail_result BEFORE INSERT ON request_events BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let response = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"a","input":"synthetic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _: Value = response.json().await.unwrap();
    let data = report(&client, &endpoint, &accounts.alice, false).await;
    assert_eq!(data["requests"][1]["transport_observation"], "unconfirmed");
    assert_eq!(data["requests"][1]["usage"]["state"], "unattributed");
    for (path, header_name, value) in [
        ("/v1/models", "Origin", "https://untrusted.example"),
        ("/v1/models", "Host", "untrusted.example"),
        ("/v1/models", "x-gateway-session", "forged-session"),
    ] {
        let response = client
            .get(format!("{}{path}", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .header(header_name, value)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    assert_eq!(
        client
            .post(format!("{}/__continuation/sessions", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .json(&json!({"subject":"operator"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
struct BlockingPeer {
    peer: Peer,
    entered: AtomicBool,
    release: Mutex<bool>,
    wake: std::sync::Condvar,
}
impl PeerSource for BlockingPeer {
    fn current(&self) -> gateway_management::Result<Peer> {
        self.entered.store(true, Ordering::SeqCst);
        let mut released = self.release.lock().unwrap();
        while !*released {
            released = self.wake.wait(released).unwrap();
        }
        Ok(self.peer.clone())
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_blocking_admission_does_not_leave_a_ghost_active_request() {
    use tower::ServiceExt;
    let accounts = Accounts::new();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/v1/responses", post(provider))
            .with_state(mock.clone()),
    )
    .await;
    let cfg = config(&upstream.url);
    let digest =
        Digest::try_from(cfg.manifest().unwrap().configuration_sha256().to_owned()).unwrap();
    let gateway = serve(
        agent_response_gateway::router(
            cfg,
            Secrets {
                local_token: LOCAL.into(),
                upstream_keys: BTreeMap::from([("mock".into(), PROVIDER.into())]),
            },
        )
        .unwrap(),
    )
    .await;
    let peer = Arc::new(BlockingPeer {
        peer: peer(&gateway.url, digest, None),
        entered: AtomicBool::new(false),
        release: Mutex::new(false),
        wake: std::sync::Condvar::new(),
    });
    let (ledger, path) = accounts.ledger();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = Service::new(
        listener.local_addr().unwrap(),
        accounts.auth.clone(),
        peer.clone(),
        ledger,
        None,
        Limits::default(),
    )
    .unwrap();
    let router = service.router();
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("host", listener.local_addr().unwrap().to_string())
        .header(
            "authorization",
            format!("Bearer {}", accounts.alice.expose()),
        )
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"a","input":"synthetic"}).to_string(),
        ))
        .unwrap();
    let task = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !peer.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    *peer.release.lock().unwrap() = true;
    peer.wake.notify_all();
    let db = rusqlite::Connection::open(path.join("team-requests.sqlite3")).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM request_events WHERE phase='finished'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            if count == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let evidence: String = db
        .query_row(
            "SELECT evidence FROM request_events WHERE phase='finished'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let value: Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(value["transport"], "client_disconnected");
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
}
struct ManagedProcess {
    _child: OwnedChild,
    peer: Peer,
}
struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl ManagedProcess {
    fn start(accounts: &Accounts, upstream: &str) -> Self {
        use std::io::BufRead;
        let store = accounts.root.path().join("continuation");
        private(&store);
        let db =
            agent_response_gateway::continuation::SqliteStore::open(&store, true, 16 * 1024 * 1024)
                .unwrap();
        let store_id = db.identity().unwrap();
        drop(db);
        let mut raw = format!(
            "listen='127.0.0.1:0'\n[providers.mock]\nbase_url='{upstream}/v1'\napi_key_env='SYNTHETIC_PROVIDER_KEY'\n"
        );
        for alias in ["a", "b"] {
            raw += &format!(
                "[models.{alias}]\nprovider='mock'\nupstream_model='synthetic-model'\napi='messages'\nauth='api_key'\nmessages_version='2023-06-01'\ncapability_profile='managed'\ncontinuation_mode='managed'\n"
            );
        }
        raw += &format!(
            "[capability_profiles.managed]\nversion='1'\nprovider='mock'\nupstream_model='synthetic-model'\napi='messages'\ncontext_window=32768\nmax_output_tokens=8192\ntested_codex_version='0.154.0'\n[capability_profiles.managed.reasoning_contract]\nkind='claude_adaptive'\nversion=1\nefforts=['low','medium','high']\ndefault_effort='medium'\n[capability_profiles.managed.support]\nfunction_tools='native'\nmax_output_tokens='native'\nreasoning_effort='native'\nreasoning_summary='native'\nreasoning_items='native'\ntool_choice='native'\n[continuation]\ndirectory={}\nstore_id='{store_id}'\nrealm='synthetic-team'\ngeneration='1'\nkey_id='synthetic-key'\nkey_env='SYNTHETIC_PROTECTION'\ncontrol_token_env='SYNTHETIC_CONTROL'\nmax_store_bytes=16777216\n",
            serde_json::to_string(&store).unwrap()
        );
        let cfg = Config::parse(&raw).unwrap();
        let manifest = cfg.manifest().unwrap();
        let configuration = Digest::try_from(manifest.configuration_sha256().to_owned()).unwrap();
        let mut routes = BTreeMap::new();
        for alias in ["a", "b"] {
            let mut route = manifest.configuration()["routes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["alias"] == alias)
                .unwrap()
                .clone();
            route.as_object_mut().unwrap().remove("api_key_env");
            routes.insert(
                alias.to_owned(),
                Route {
                    managed: Some(gateway_team_http::ManagedOrigin {
                        route,
                        realm: "synthetic-team".into(),
                        generation: "1".into(),
                    }),
                },
            );
        }
        let config_path = accounts.root.path().join("gateway.toml");
        std::fs::write(&config_path, raw).unwrap();
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!(
                "agent-response-gateway{}",
                std::env::consts::EXE_SUFFIX
            ));
        assert!(
            binary.is_file(),
            "Build the Gateway binary before the team process fixtures"
        );
        let control = "synthetic-control-key-01234567890123456789";
        let mut command = std::process::Command::new(binary);
        command
            .args(["serve", "--config"])
            .arg(config_path)
            .env_clear()
            .env("ARG_LOCAL_TOKEN", LOCAL)
            .env("SYNTHETIC_PROVIDER_KEY", PROVIDER)
            .env("SYNTHETIC_PROTECTION", "1".repeat(64))
            .env("SYNTHETIC_CONTROL", control)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            command.env(
                "SYSTEMROOT",
                std::env::var_os("SYSTEMROOT").expect("Windows requires registered SYSTEMROOT"),
            );
        }
        let mut child = OwnedChild(command.spawn().unwrap());
        let stdout = child.0.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut line = String::new();
            let result = std::io::BufReader::new(stdout).read_line(&mut line);
            let _ = send.send(result.map(|_| line));
        });
        let ready = receive.recv_timeout(Duration::from_secs(10));
        if ready.is_err() {
            let _ = child.0.kill();
            let _ = child.0.wait();
        }
        let ready: Value =
            serde_json::from_str(&ready.expect("Managed fixture readiness deadline").unwrap())
                .unwrap();
        reader.join().unwrap();
        assert_eq!(ready["configuration_sha256"], configuration.as_str());
        let address: std::net::SocketAddr = ready["address"].as_str().unwrap().parse().unwrap();
        let peer = Peer::new(
            gateway_team_http::PeerIdentity {
                target: id("gateway"),
                instance: id("managed-fixture"),
                configuration_sha256: configuration,
                producer: None,
            },
            &format!("http://{address}"),
            routes,
            LOCAL.into(),
            Some(control.into()),
        )
        .unwrap();
        Self {
            _child: child,
            peer,
        }
    }
}
async fn messages(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(_): Json<Value>,
) -> Json<Value> {
    mock.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(headers["x-api-key"], PROVIDER);
    Json(
        json!({"id":"synthetic-message","type":"message","role":"assistant","model":"synthetic-model","content":[{"type":"thinking","thinking":"synthetic reasoning","signature":"synthetic-signature"},{"type":"text","text":"synthetic answer"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":7,"output_tokens":3}}),
    )
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_managed_process_enforces_subject_route_and_replay_session_ownership() {
    let mut accounts = Accounts::new();
    accounts.serial += 1;
    let command = Command::PermissionsChange {
        subject: id("bob"),
        permissions: Permissions {
            enabled: true,
            routes: BTreeSet::from(["a".into(), "b".into()]),
            management: BTreeSet::new(),
            read_all_usage: false,
        },
    };
    let request = Request {
        target: id("gateway"),
        action: command.action(),
        expected: accounts.manager.snapshot().unwrap(),
        idempotency_key: id("bob-shared-route"),
        parameters_sha256: command.digest().unwrap(),
    };
    accounts
        .manager
        .execute(&mut accounts.journal, &accounts.admin, &request, &command)
        .unwrap();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/v1/messages", post(messages))
            .with_state(mock.clone()),
    )
    .await;
    let gateway = ManagedProcess::start(&accounts, &upstream.url);
    let (ledger, _) = accounts.ledger();
    let endpoint = team(
        &accounts,
        Arc::new(gateway.peer.clone()),
        ledger,
        None,
        Limits::default(),
    )
    .await;
    let client = client();
    let create = |secret: &Secret, key: &str| {
        client
            .post(format!("{}/team/v1/sessions", endpoint.url))
            .bearer_auth(secret.expose())
            .json(&json!({"model":"a","idempotency_key":key}))
    };
    let alice: Value = create(&accounts.alice, "alice-session")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(alice["state"], "bound");
    let alice_id = alice["session"].as_str().unwrap();
    let same: Value = create(&accounts.alice, "alice-session")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(same["session"], alice["session"]);
    let bob: Value = create(&accounts.bob, "bob-session")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bob["state"], "bound");
    let bob_id = bob["session"].as_str().unwrap();
    assert_ne!(alice_id, bob_id);
    let denied = client
        .get(format!("{}/team/v1/sessions/{alice_id}", endpoint.url))
        .bearer_auth(accounts.bob.expose())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let body = json!({"model":"a","input":"synthetic question","reasoning":{"effort":"medium","summary":"auto"}});
    let cross = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.bob.expose())
        .header("x-team-session", alice_id)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(cross.status(), StatusCode::NOT_FOUND);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    let wrong = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.bob.expose())
        .header("x-team-session", bob_id)
        .json(&json!({"model":"b","input":"synthetic question"}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::CONFLICT);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    let reply = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .header("x-team-session", alice_id)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status(), StatusCode::OK);
    let reply: Value = reply.json().await.unwrap();
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    let status: Value = client
        .get(format!("{}/team/v1/sessions/{alice_id}", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["has_head"], true);
    assert!(status.get("origin").is_none());
    assert!(status.get("internal").is_none());
    let mut input = vec![
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"synthetic question"}]}),
    ];
    input.extend(reply["output"].as_array().unwrap().iter().cloned());
    input.push(
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"continue"}]}),
    );
    let stolen = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.bob.expose())
        .header("x-team-session", bob_id)
        .json(&json!({"model":"a","input":input,"reasoning":{"effort":"medium","summary":"auto"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(stolen.status(), StatusCode::CONFLICT);
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn stopped_peer_and_mismatched_targets_are_not_adopted_or_attributed() {
    let accounts = Accounts::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let (ledger, _) = accounts.ledger();
    let endpoint = team(
        &accounts,
        Arc::new(peer(
            &url,
            Digest::of(b"configuration"),
            Some(id("producer")),
        )),
        ledger,
        None,
        Limits::default(),
    )
    .await;
    let client = client();
    let response = client
        .post(format!("{}/v1/responses", endpoint.url))
        .bearer_auth(accounts.alice.expose())
        .json(&json!({"model":"a","input":"synthetic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let data = report(&client, &endpoint, &accounts.alice, false).await;
    assert_eq!(
        data["requests"][0]["record"]["finished"]["transport"],
        "gateway_connection_failed"
    );
    assert_eq!(data["requests"][0]["usage"]["state"], "unattributed");
    let other = accounts.root.path().join("other-target");
    private(&other);
    let ledger = Ledger::initialize(&other, id("other"), 8 * 1024 * 1024).unwrap();
    let bound = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(
        Service::new(
            bound.local_addr().unwrap(),
            accounts.auth.clone(),
            Arc::new(peer(&url, Digest::of(b"configuration"), None)),
            ledger,
            None,
            Limits::default()
        )
        .is_err()
    );
    let path = accounts.root.path().join("wrong-peer");
    private(&path);
    let ledger = Ledger::initialize(&path, id("gateway"), 8 * 1024 * 1024).unwrap();
    let mut wrong = peer(&url, Digest::of(b"configuration"), None);
    wrong.identity.target = id("other");
    let wrong_endpoint = team(&accounts, Arc::new(wrong), ledger, None, Limits::default()).await;
    assert_eq!(
        client
            .get(format!("{}/v1/models", wrong_endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
struct ChangingUsage {
    mode: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    configuration: Digest,
}
impl gateway_team_http::UsageReader for ChangingUsage {
    fn lookup(
        &mut self,
        producer: &Id,
        request: &Id,
    ) -> gateway_management::Result<Vec<UsageEvent>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mode = self.mode.load(Ordering::SeqCst);
        if mode == 0 {
            return Err(gateway_management::Error::Storage);
        }
        let value = UsageEvent {
            schema: gateway_usage_contract::SCHEMA.into(),
            producer_id: producer.to_string(),
            request_id: request.to_string(),
            attempt_id: "synthetic-attempt".into(),
            event_id: "synthetic-event".into(),
            revision: 1,
            kind: EventKind::AttemptFinished,
            started_at_ms: now(),
            observed_at_ms: now(),
            provider: "mock".into(),
            model_alias: if mode == 1 { "b" } else { "a" }.into(),
            upstream_model: "native-a".into(),
            reported_model: None,
            provider_request_id: None,
            provider_response_id: None,
            profile: Profile::ResponsesV1,
            configuration_sha256: self.configuration.as_str().into(),
            upstream: Outcome::Unknown,
            gateway: Outcome::Unknown,
            finality: Finality::Unobserved,
            observation_incomplete: true,
            usage: Default::default(),
        };
        Ok(if mode == 2 {
            vec![value.clone(), value]
        } else {
            vec![value]
        })
    }
}
#[tokio::test]
async fn recorder_failures_mismatched_routes_and_duplicate_attempts_stay_unobserved() {
    let accounts = Accounts::new();
    let mock = Arc::new(Mock::default());
    let upstream = serve(
        Router::new()
            .route("/v1/responses", post(provider))
            .with_state(mock),
    )
    .await;
    let cfg = config(&upstream.url);
    let digest =
        Digest::try_from(cfg.manifest().unwrap().configuration_sha256().to_owned()).unwrap();
    let gateway = serve(
        agent_response_gateway::router(
            cfg,
            Secrets {
                local_token: LOCAL.into(),
                upstream_keys: BTreeMap::from([("mock".into(), PROVIDER.into())]),
            },
        )
        .unwrap(),
    )
    .await;
    let (ledger, _) = accounts.ledger();
    let mode = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let usage = ChangingUsage {
        mode: mode.clone(),
        calls: calls.clone(),
        configuration: digest.clone(),
    };
    let endpoint = team(
        &accounts,
        Arc::new(peer(&gateway.url, digest, Some(id("producer")))),
        ledger,
        Some(Box::new(usage)),
        Limits::default(),
    )
    .await;
    let client = client();
    for _ in 0..2 {
        let response = client
            .post(format!("{}/v1/responses", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .json(&json!({"model":"a","input":"synthetic"}))
            .send()
            .await
            .unwrap();
        let _: Value = response.json().await.unwrap();
    }
    let data = report(&client, &endpoint, &accounts.alice, false).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        data["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["usage"]["reason"] == "recorder_unavailable")
    );
    for kind in [1, 2] {
        mode.store(kind, Ordering::SeqCst);
        let data = report(&client, &endpoint, &accounts.alice, false).await;
        assert!(
            data["requests"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["usage"]["state"] == "unobserved"
                    && r["usage"]["reason"] == "correlation_mismatch")
        );
    }
    mode.store(3, Ordering::SeqCst);
    let data = report(&client, &endpoint, &accounts.alice, false).await;
    assert_eq!(
        data["requests"][0]["usage"]["attempts"][0]["upstream"],
        "unknown"
    );
    assert!(
        data["requests"][0]["usage"]["attempts"][0]["usage"]["counters"]["input_tokens"]["value"]
            .is_null()
    );
    let forged = client
        .get(format!(
            "{}/team/v1/usage?from_ms={}&to_ms={}&subject=bob",
            endpoint.url,
            now() - 60000,
            now() + 60000
        ))
        .bearer_auth(accounts.alice.expose())
        .send()
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::BAD_REQUEST);
}
#[test]
fn ledger_reopen_backup_and_session_retry_preserve_unconfirmed_evidence() {
    let accounts = Accounts::new();
    let principal = accounts
        .auth
        .authenticate_model(accounts.alice.expose())
        .unwrap();
    let (mut ledger, path) = accounts.ledger();
    let peer = peer(
        "http://127.0.0.1:47311",
        Digest::of(b"configuration"),
        Some(id("producer")),
    );
    let admission = ledger.admit(&principal, &peer, "a".into(), None).unwrap();
    let origin = Digest::of(b"origin");
    let (session, fresh) = ledger
        .session_intent(&principal, "a", &origin, id("create-once"))
        .unwrap();
    assert!(fresh);
    let (duplicate, fresh) = ledger
        .session_intent(&principal, "a", &origin, id("create-once"))
        .unwrap();
    assert!(!fresh);
    assert_eq!(duplicate.id, session.id);
    assert_eq!(
        ledger.session_view(&principal, &session.id).unwrap()["state"],
        "unconfirmed"
    );
    assert!(
        ledger
            .session_intent(
                &principal,
                "a",
                &Digest::of(b"different origin"),
                id("create-once")
            )
            .is_err()
    );
    let query = gateway_team_http::UsageQuery {
        from_ms: now() - 60000,
        to_ms: now() + 60000,
        after: 0,
        all: false,
    };
    let records = ledger.records(&principal, &query).unwrap();
    assert_eq!(records[0].admission.id, admission.id);
    assert!(records[0].headers.is_none());
    assert!(records[0].finished.is_none());
    let backup = accounts.root.path().join("backup");
    private(&backup);
    ledger
        .backup(&backup.join("team-requests.sqlite3"))
        .unwrap();
    assert!(
        ledger
            .backup(&backup.join("team-requests.sqlite3"))
            .is_err()
    );
    drop(ledger);
    let restored = Ledger::open(&backup, id("gateway"), 32 * 1024 * 1024).unwrap();
    assert_eq!(
        restored.records(&principal, &query).unwrap()[0]
            .admission
            .id,
        admission.id
    );
    let reopened = Ledger::open(&path, id("gateway"), 32 * 1024 * 1024).unwrap();
    assert_eq!(
        reopened.session_view(&principal, &session.id).unwrap()["state"],
        "unconfirmed"
    );
    let bob = accounts
        .auth
        .authenticate_model(accounts.bob.expose())
        .unwrap();
    assert!(reopened.session(&bob, &session.id).is_err());
    assert!(reopened.records(&bob, &query).unwrap().is_empty());
}
#[derive(Clone)]
struct BrokenPeer {
    mode: usize,
    calls: Arc<AtomicUsize>,
}
async fn broken(State(peer): State<BrokenPeer>, headers: HeaderMap) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {LOCAL}"));
    peer.calls.fetch_add(1, Ordering::SeqCst);
    let stream = futures_util::stream::unfold(0, move |step| async move {
        if step == 0 {
            return Some((
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: {\"synthetic\":true}\n\n")),
                1,
            ));
        }
        if step > 1 {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
        match peer.mode {
            0 => Some((Err(std::io::Error::other("synthetic disconnect")), 2)),
            1 => std::future::pending().await,
            _ => Some((Ok(Bytes::from(vec![b'x'; 2048])), 2)),
        }
    });
    Response::builder()
        .header("x-request-id", uuid::Uuid::new_v4().to_string())
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}
#[tokio::test]
async fn body_loss_idle_timeout_and_response_limit_never_become_eof_success() {
    for (mode, expected) in [
        (0, "gateway_body_lost"),
        (1, "idle_timeout"),
        (2, "response_limit"),
    ] {
        let accounts = Accounts::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let gateway = serve(
            Router::new()
                .route("/v1/responses", post(broken))
                .with_state(BrokenPeer {
                    mode,
                    calls: calls.clone(),
                }),
        )
        .await;
        let (ledger, _) = accounts.ledger();
        let endpoint = team(
            &accounts,
            Arc::new(peer(&gateway.url, Digest::of(b"configuration"), None)),
            ledger,
            None,
            Limits {
                idle_timeout: Duration::from_millis(80),
                max_response_bytes: 1024,
                ..Limits::default()
            },
        )
        .await;
        let client = client();
        let response = client
            .post(format!("{}/v1/responses", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .json(&json!({"model":"a","stream":true,"input":"synthetic"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().await.is_err());
        let data = report(&client, &endpoint, &accounts.alice, false).await;
        assert_eq!(
            data["requests"][0]["record"]["finished"]["transport"],
            expected
        );
        assert_eq!(data["requests"][0]["usage"]["state"], "unattributed");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn uncertain_session_creation_is_not_automatically_reissued() {
    let accounts = Accounts::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let copy = calls.clone();
    let gateway = serve(Router::new().route(
        "/__continuation/sessions",
        post(move |headers: HeaderMap| {
            let calls = copy.clone();
            async move {
                assert_eq!(
                    headers["authorization"],
                    "Bearer synthetic-control-key-01234567890123456789"
                );
                calls.fetch_add(1, Ordering::SeqCst);
                Json(json!({"id":uuid::Uuid::new_v4().to_string(),"origin":{},"status":"ready"}))
            }
        }),
    ))
    .await;
    let mut routes = BTreeMap::new();
    routes.insert(
        "a".into(),
        Route {
            managed: Some(gateway_team_http::ManagedOrigin {
                route: json!({"alias":"a"}),
                realm: "synthetic".into(),
                generation: "1".into(),
            }),
        },
    );
    let registration = Peer::new(
        gateway_team_http::PeerIdentity {
            target: id("gateway"),
            instance: id("synthetic"),
            configuration_sha256: Digest::of(b"configuration"),
            producer: None,
        },
        &gateway.url,
        routes,
        LOCAL.into(),
        Some("synthetic-control-key-01234567890123456789".into()),
    )
    .unwrap();
    let (ledger, _) = accounts.ledger();
    let endpoint = team(
        &accounts,
        Arc::new(registration),
        ledger,
        None,
        Limits::default(),
    )
    .await;
    let client = client();
    let mut previous = None;
    for _ in 0..2 {
        let response = client
            .post(format!("{}/team/v1/sessions", endpoint.url))
            .bearer_auth(accounts.alice.expose())
            .json(&json!({"model":"a","idempotency_key":"create-once"}))
            .send()
            .await
            .unwrap();
        let value: Value = response.json().await.unwrap();
        assert_eq!(value["state"], "unconfirmed");
        if let Some(id) = &previous {
            assert_eq!(id, &value["session"]);
        }
        previous = Some(value["session"].clone());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permission_change_during_peer_selection_rejects_the_stale_admission() {
    use tower::ServiceExt;
    let mut accounts = Accounts::new();
    let peer = Arc::new(BlockingPeer {
        peer: peer("http://127.0.0.1:47311", Digest::of(b"configuration"), None),
        entered: AtomicBool::new(false),
        release: Mutex::new(false),
        wake: std::sync::Condvar::new(),
    });
    let (ledger, path) = accounts.ledger();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = Service::new(
        listener.local_addr().unwrap(),
        accounts.auth.clone(),
        peer.clone(),
        ledger,
        None,
        Limits::default(),
    )
    .unwrap();
    let router = service.router();
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("host", listener.local_addr().unwrap().to_string())
        .header(
            "authorization",
            format!("Bearer {}", accounts.alice.expose()),
        )
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"a","input":"synthetic"}).to_string(),
        ))
        .unwrap();
    let task = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !peer.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    accounts.revoke_alice();
    *peer.release.lock().unwrap() = true;
    peer.wake.notify_all();
    assert_eq!(task.await.unwrap().status(), StatusCode::FORBIDDEN);
    let db = rusqlite::Connection::open(path.join("team-requests.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn builtin_recorder_capability_does_not_enable_unsupported_native_platforms() {
    assert_eq!(
        SqliteUsage::supported(),
        cfg!(any(target_os = "linux", target_os = "macos"))
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    assert!(matches!(
        SqliteUsage::open(Path::new("unregistered-store")),
        Err(gateway_management::Error::Unsupported)
    ));
}
