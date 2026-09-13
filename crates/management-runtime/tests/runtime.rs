use axum::{Json, Router, http::HeaderMap, routing::post};
use gateway_management::{
    Action, Actor, Digest, Error, Grant, Id, Identity, Journal, Request, State,
};
use gateway_management_runtime::{
    Command, Registration, Runtime, Source,
    protocol::{LAUNCH_SCHEMA, Launch, Ready},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, Read, Write},
    path::PathBuf,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const LOCAL: &str = "synthetic-local-only-token-0123456789";
const PROVIDER: &str = "synthetic-provider-only-token";
fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn directory() -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/runtime-tests");
    fs::create_dir_all(&base).unwrap();
    let d = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    d
}
fn raw(upstream: &str, listen: &str) -> String {
    format!(
        "listen=\"{listen}\"\nlocal_token_env=\"LOCAL_KEY\"\n[limits]\nshutdown_grace_ms=100\n[providers.mock]\nbase_url=\"{upstream}/v1\"\napi_key_env=\"PROVIDER_KEY\"\n[models.writer]\nprovider=\"mock\"\nupstream_model=\"synthetic-model\"\n"
    )
}
fn actor() -> Actor {
    Actor::new(
        Identity {
            subject: id("operator"),
            credential: id("management-key-id"),
        },
        [
            Action::ConfigurationStage,
            Action::ConfigurationSelect,
            Action::RuntimeStart,
            Action::RuntimeStop,
            Action::RuntimeRestart,
            Action::Reconcile,
        ]
        .into_iter()
        .map(|action| Grant {
            action,
            target: id("instance"),
        }),
    )
    .unwrap()
}
fn registration(path: PathBuf, config: PathBuf) -> Registration {
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_gateway-managed-child"))
        .canonicalize()
        .unwrap();
    let environment = BTreeMap::from([
        ("LOCAL_KEY".into(), LOCAL.into()),
        ("PROVIDER_KEY".into(), PROVIDER.into()),
        (
            "MANAGEMENT_UNUSED".into(),
            "synthetic-manager-secret".into(),
        ),
        ("TEAM_UNUSED".into(), "synthetic-team-secret".into()),
    ]);
    #[cfg(windows)]
    let environment = {
        let mut environment = environment;
        environment.insert("SYSTEMROOT".into(), std::env::var("SYSTEMROOT").unwrap());
        environment
    };
    Registration {
        target: id("instance"),
        directory: path,
        executable_sha256: Digest::of(&fs::read(&executable).unwrap()),
        executable,
        credential_generation: id("synthetic-generation"),
        sources: BTreeMap::from([(
            id("source"),
            Source {
                configuration: config,
                extensions_lock: None,
                profile_packs_lock: None,
            },
        )]),
        environment,
        startup_timeout: Duration::from_secs(20),
        stop_timeout: Duration::from_secs(3),
    }
}
fn execute(
    runtime: &mut Runtime,
    journal: &mut Journal,
    command: Command,
    key: &str,
) -> gateway_management::Operation {
    let request = Request {
        target: id("instance"),
        action: command.action(),
        expected: runtime.snapshot().unwrap(),
        idempotency_key: id(key),
        parameters_sha256: command.digest().unwrap(),
    };
    journal
        .execute(&actor(), &request, &mut runtime.bind(command))
        .unwrap()
}
fn configure(runtime: &mut Runtime, journal: &mut Journal, contents: &str) {
    assert_eq!(
        execute(
            runtime,
            journal,
            Command::Stage {
                source: id("source"),
                candidate: id("candidate"),
                source_sha256: Digest::of(contents.as_bytes())
            },
            "stage"
        )
        .state,
        State::Succeeded
    );
    assert_eq!(
        execute(
            runtime,
            journal,
            Command::Select {
                candidate: id("candidate")
            },
            "select"
        )
        .state,
        State::Succeeded
    );
}
struct Fixture {
    _source: tempfile::TempDir,
    _runtime: tempfile::TempDir,
    _journal: tempfile::TempDir,
    config: PathBuf,
    root: PathBuf,
    runtime: Runtime,
    journal: Journal,
}
impl Fixture {
    fn new(contents: &str) -> Self {
        let source = directory();
        let dir = directory();
        let journal_dir = directory();
        let config = source.path().join("source.toml");
        fs::write(&config, contents).unwrap();
        let root = dir.path().to_path_buf();
        let runtime = Runtime::initialize(registration(root.clone(), config.clone())).unwrap();
        let journal = Journal::initialize(journal_dir.path(), 8 * 1024 * 1024).unwrap();
        Self {
            _source: source,
            _runtime: dir,
            _journal: journal_dir,
            config,
            root,
            runtime,
            journal,
        }
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_child_uses_selected_configuration_and_bounded_active_shutdown() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = upstream.local_addr().unwrap();
    let app=Router::new().route("/v1/responses",post(move|headers:HeaderMap,Json(body):Json<Value>|{
        let count=count.clone();
        async move {
            assert_eq!(headers["authorization"],format!("Bearer {PROVIDER}"));
            assert!(headers.get("x-management-token").is_none());
            count.fetch_add(1,Ordering::SeqCst);
            if body["stream"]==true {
                let stream=async_stream::stream!{
                    yield Ok::<_,std::io::Error>(axum::body::Bytes::from_static(b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n"));
                    std::future::pending::<()>().await;
                };
                axum::response::Response::builder().header("content-type","text/event-stream").body(axum::body::Body::from_stream(stream)).unwrap()
            }else{
                axum::response::Response::builder().header("content-type","application/json")
                    .body(axum::body::Body::from(json!({"id":"synthetic","status":"completed","output":[]}).to_string())).unwrap()
            }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(upstream, app).await.unwrap() });
    let contents = raw(&format!("http://{address}"), "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
    let op = execute(&mut f.runtime, &mut f.journal, Command::Start, "start");
    assert_eq!(op.state, State::Succeeded);
    let status = f.runtime.status().unwrap();
    let ready = status.running.unwrap();
    assert!(!status.restart_required);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let result = client
        .post(format!("{}/responses", ready.gateway.base_url))
        .bearer_auth(LOCAL)
        .json(&json!({"model":"writer","input":"synthetic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 200);
    assert_eq!(result.json::<Value>().await.unwrap()["status"], "completed");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    let mut streaming = client
        .post(format!("{}/responses", ready.gateway.base_url))
        .bearer_auth(LOCAL)
        .json(&json!({"model":"writer","input":"synthetic","stream":true}))
        .send()
        .await
        .unwrap();
    assert!(streaming.chunk().await.unwrap().is_some());
    let start = Instant::now();
    assert_eq!(
        execute(&mut f.runtime, &mut f.journal, Command::Stop, "stop").state,
        State::Succeeded
    );
    assert!(start.elapsed() < Duration::from_secs(4));
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
    let remaining = streaming.bytes().await;
    if let Ok(bytes) = remaining {
        assert!(!String::from_utf8_lossy(&bytes).contains("response.completed"));
    }
    let serialized = serde_json::to_string(&f.runtime.status().unwrap()).unwrap();
    for secret in [
        LOCAL,
        PROVIDER,
        "synthetic-manager-secret",
        "synthetic-team-secret",
    ] {
        assert!(!serialized.contains(secret));
    }
    server.abort();
}

#[test]
fn stale_and_external_configuration_changes_do_not_start_a_child() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    let command = Command::Start;
    let request = Request {
        target: id("instance"),
        action: command.action(),
        expected: f.runtime.snapshot().unwrap(),
        idempotency_key: id("stale"),
        parameters_sha256: command.digest().unwrap(),
    };
    execute(
        &mut f.runtime,
        &mut f.journal,
        Command::Select {
            candidate: id("candidate"),
        },
        "reselect",
    );
    assert!(matches!(
        f.journal
            .execute(&actor(), &request, &mut f.runtime.bind(command)),
        Err(Error::Conflict)
    ));
    let state = f.root.join("state.json");
    let mut bytes = fs::read(&state).unwrap();
    bytes.push(b' ');
    fs::write(&state, &bytes).unwrap();
    assert!(f.runtime.status().unwrap().external_change);
    let cmd = Command::Start;
    let request = Request {
        target: id("instance"),
        action: cmd.action(),
        expected: f.runtime.snapshot().unwrap(),
        idempotency_key: id("external"),
        parameters_sha256: cmd.digest().unwrap(),
    };
    assert!(matches!(
        f.journal
            .execute(&actor(), &request, &mut f.runtime.bind(cmd)),
        Err(Error::Conflict)
    ));
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
    assert_eq!(fs::read(state).unwrap(), bytes);
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn launch_unowned(f: &Fixture, contents: &str) -> (Process, Ready) {
    let config = agent_response_gateway::Config::parse(contents).unwrap();
    let manifest = config.manifest().unwrap();
    let launch = Launch {
        schema: LAUNCH_SCHEMA.into(),
        instance_id: id("parent-loss-run"),
        directory: f.root.clone(),
        configuration: f.config.clone(),
        extensions_lock: None,
        profile_packs_lock: None,
        configuration_sha256: Digest::try_from(manifest.configuration_sha256().to_owned()).unwrap(),
        execution_sha256: None,
    };
    let mut command = ProcessCommand::new(env!("CARGO_BIN_EXE_gateway-managed-child"));
    command
        .env_clear()
        .env("LOCAL_KEY", LOCAL)
        .env("PROVIDER_KEY", PROVIDER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    command.env("SYSTEMROOT", std::env::var("SYSTEMROOT").unwrap());
    let mut child = command.spawn().unwrap();
    let mut frame = serde_json::to_vec(&launch).unwrap();
    frame.push(b'\n');
    child.stdin.as_mut().unwrap().write_all(&frame).unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = vec![];
        std::io::BufReader::new(stdout)
            .take(65537)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        let _ = tx.send(bytes);
    });
    let p = Process(child);
    let bytes = rx.recv_timeout(Duration::from_secs(20)).unwrap();
    (p, serde_json::from_slice(&bytes).unwrap())
}
#[test]
fn parent_connection_loss_stops_child_without_adopting_its_identity() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    let (mut process, ready) = launch_unowned(&f, &contents);
    assert_eq!(ready.instance_id, id("parent-loss-run"));
    assert_eq!(f.runtime.status().unwrap().ownership, "unowned");
    assert!(f.runtime.stop_owned().is_err());
    drop(process.0.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned child must stop after its parent channel closes"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
}

#[test]
fn wrong_launch_identity_rejects_before_credential_resolution() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let f = Fixture::new(&contents);
    let launch = Launch {
        schema: LAUNCH_SCHEMA.into(),
        instance_id: id("wrong-run"),
        directory: f.root.clone(),
        configuration: f.config.clone(),
        extensions_lock: None,
        profile_packs_lock: None,
        configuration_sha256: Digest::of(b"wrong"),
        execution_sha256: None,
    };
    assert!(matches!(
        gateway_management_runtime::prepare_launch(&launch),
        Err(Error::Conflict)
    ));
}

#[test]
fn selection_preserves_running_identity_until_explicit_restart() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    assert_eq!(
        execute(&mut f.runtime, &mut f.journal, Command::Start, "start").state,
        State::Succeeded
    );
    let first = f.runtime.status().unwrap().running.unwrap();
    let revised = contents.replace("synthetic-model", "synthetic-model-two");
    fs::write(&f.config, &revised).unwrap();
    assert_eq!(
        execute(
            &mut f.runtime,
            &mut f.journal,
            Command::Stage {
                source: id("source"),
                candidate: id("second"),
                source_sha256: Digest::of(revised.as_bytes())
            },
            "stage-second"
        )
        .state,
        State::Succeeded
    );
    assert!(!f.runtime.status().unwrap().restart_required);
    assert_eq!(
        execute(
            &mut f.runtime,
            &mut f.journal,
            Command::Select {
                candidate: id("second")
            },
            "select-second"
        )
        .state,
        State::Succeeded
    );
    let pending = f.runtime.status().unwrap();
    assert!(pending.restart_required);
    assert_ne!(pending.desired, pending.running_manifest);
    assert_eq!(pending.running.unwrap().instance_id, first.instance_id);
    assert_eq!(
        execute(&mut f.runtime, &mut f.journal, Command::Restart, "restart").state,
        State::Succeeded
    );
    let running = f.runtime.status().unwrap();
    assert!(!running.restart_required);
    assert_eq!(running.desired, running.running_manifest);
    assert_ne!(running.running.unwrap().instance_id, first.instance_id);
    // Selecting an exact earlier snapshot is a forward operation, not a data rollback.
    assert_eq!(
        execute(
            &mut f.runtime,
            &mut f.journal,
            Command::Select {
                candidate: id("candidate")
            },
            "select-previous"
        )
        .state,
        State::Succeeded
    );
    assert!(f.runtime.status().unwrap().restart_required);
    assert_eq!(f.runtime.status().unwrap().candidates.len(), 2);
}

#[test]
fn failed_bind_is_uncertain_and_does_not_adopt_the_existing_listener() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap();
    let contents = raw("http://127.0.0.1:9", &address.to_string());
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    let op = execute(
        &mut f.runtime,
        &mut f.journal,
        Command::Start,
        "bind-failure",
    );
    assert_eq!(op.state, State::Uncertain);
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
    assert!(f.runtime.status().unwrap().running.is_none());
    assert!(occupied.local_addr().is_ok());
    // Absence of a completion receipt is not evidence for a historical success.
    let reconciled = f
        .journal
        .reconcile(
            &actor(),
            &id("instance"),
            &op.id,
            &mut f.runtime.bind(Command::Start),
        )
        .unwrap();
    assert_eq!(reconciled.state, State::Uncertain);
}

#[test]
fn tampered_candidate_rejects_before_start_and_preserves_the_files() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    let file = fs::read_dir(f.root.join("candidates"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let changed = contents.replace("synthetic-model", "modified");
    fs::write(&file, &changed).unwrap();
    let snapshot = f.runtime.snapshot().unwrap();
    assert!(!f.runtime.status().unwrap().desired_valid);
    let request = Request {
        target: id("instance"),
        action: Action::RuntimeStart,
        expected: snapshot,
        idempotency_key: id("tampered"),
        parameters_sha256: Command::Start.digest().unwrap(),
    };
    assert!(matches!(
        f.journal
            .execute(&actor(), &request, &mut f.runtime.bind(Command::Start)),
        Err(Error::Conflict)
    ));
    assert_eq!(fs::read_to_string(file).unwrap(), changed);
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
}

#[test]
fn managed_readiness_rejects_ambiguous_fields_and_versions() {
    let value = json!({"schema":"gateway-managed-process/v1", "instance_id":"synthetic-run", "gateway": {
        "event":"ready", "address":"127.0.0.1:12345", "base_url":"http://127.0.0.1:12345/v1", "version":"0.1.0", "schema":"gateway-ready/v1", "manifest_schema":"gateway-embedded-manifest/v1", "configuration_sha256":Digest::of(b"synthetic")
    }});
    let encoded = value.to_string();
    assert!(serde_json::from_str::<Ready>(&encoded).is_ok());
    let ambiguous = encoded.replace(
        "\"event\":\"ready\"",
        "\"event\":\"ready\",\"event\":\"ready\"",
    );
    assert!(serde_json::from_str::<Ready>(&ambiguous).is_err());
    let mut unknown = value;
    unknown["gateway"]["subject"] = json!("untrusted");
    assert!(serde_json::from_value::<Ready>(unknown).is_err());
}

#[cfg(unix)]
#[test]
fn malformed_readiness_and_silent_children_are_bounded_and_not_published() {
    use std::os::unix::fs::PermissionsExt;
    for script in [
        "#!/bin/sh\nIFS= read -r launch\nprintf '%s\\n' '{\"schema\":\"wrong\"}'\nexec /bin/sleep 30\n",
        "#!/bin/sh\nexec /bin/sleep 30\n",
        "#!/bin/sh\n/usr/bin/env > observed.env\nIFS= read -r launch\nexec /bin/sleep 30\n",
    ] {
        let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
        let f = Fixture::new(&contents);
        let executable = f._source.path().join("synthetic-child");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let Fixture {
            _source,
            _runtime,
            _journal,
            config,
            root,
            runtime,
            mut journal,
        } = f;
        drop(runtime);
        let mut reg = registration(root.clone(), config);
        reg.executable_sha256 = Digest::of(script.as_bytes());
        reg.executable = executable;
        reg.startup_timeout = Duration::from_millis(300);
        reg.stop_timeout = Duration::from_millis(100);
        let mut runtime = Runtime::open(reg).unwrap();
        configure(&mut runtime, &mut journal, &contents);
        let started = Instant::now();
        assert_eq!(
            execute(&mut runtime, &mut journal, Command::Start, "bad-child").state,
            State::Uncertain
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(runtime.status().unwrap().ownership, "stopped");
        assert!(runtime.status().unwrap().running.is_none());
        if root.join("observed.env").exists() {
            let environment = fs::read_to_string(root.join("observed.env")).unwrap();
            assert!(
                environment
                    .lines()
                    .any(|line| line == format!("LOCAL_KEY={LOCAL}"))
            );
            assert!(
                environment
                    .lines()
                    .any(|line| line == format!("PROVIDER_KEY={PROVIDER}"))
            );
            for forbidden in [
                "MANAGEMENT_UNUSED",
                "TEAM_UNUSED",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "CARGO_",
            ] {
                assert!(!environment.contains(forbidden));
            }
        }
    }
}

#[test]
fn audit_failure_blocks_changes_and_completion_recovery_uses_retained_evidence() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    let db = rusqlite::Connection::open(f._journal.path().join("management.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_admission BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let request = Request {
        target: id("instance"),
        action: Action::RuntimeStart,
        expected: f.runtime.snapshot().unwrap(),
        idempotency_key: id("blocked"),
        parameters_sha256: Command::Start.digest().unwrap(),
    };
    assert!(matches!(
        f.journal
            .execute(&actor(), &request, &mut f.runtime.bind(Command::Start)),
        Err(Error::Storage)
    ));
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
    db.execute_batch("DROP TRIGGER fail_admission; CREATE TRIGGER fail_result BEFORE INSERT ON events WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let request = Request {
        idempotency_key: id("lost-result"),
        ..request
    };
    let Error::Uncertain(operation) = f
        .journal
        .execute(&actor(), &request, &mut f.runtime.bind(Command::Start))
        .unwrap_err()
    else {
        panic!("result uncertainty must identify its committed intent")
    };
    let running = f.runtime.status().unwrap().running.unwrap();
    db.execute_batch("DROP TRIGGER fail_result").unwrap();
    let outcome = f
        .journal
        .reconcile(
            &actor(),
            &id("instance"),
            &operation,
            &mut f.runtime.bind(Command::Start),
        )
        .unwrap();
    assert_eq!(outcome.state, State::Succeeded);
    assert_eq!(outcome.events[2].phase, gateway_management::Phase::Recovery);
    assert_eq!(
        f.runtime.status().unwrap().running.unwrap().instance_id,
        running.instance_id
    );
    // Emergency owned-handle cleanup works even if all later management admissions fail.
    db.execute_batch("CREATE TRIGGER fail_admission BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    f.runtime.stop_owned().unwrap();
    assert_eq!(f.runtime.status().unwrap().ownership, "stopped");
}

#[cfg(windows)]
#[test]
fn windows_requires_an_explicit_system_root_without_inheriting_other_environment() {
    let contents = raw("http://127.0.0.1:9", "127.0.0.1:0");
    let mut f = Fixture::new(&contents);
    configure(&mut f.runtime, &mut f.journal, &contents);
    let Fixture {
        _source,
        _runtime,
        _journal,
        config,
        root,
        runtime,
        mut journal,
    } = f;
    drop(runtime);
    let mut registered = registration(root, config);
    registered.environment.remove("SYSTEMROOT");
    let mut runtime = Runtime::open(registered).unwrap();
    let request = Request {
        target: id("instance"),
        action: Action::RuntimeStart,
        expected: runtime.snapshot().unwrap(),
        idempotency_key: id("missing-os-binding"),
        parameters_sha256: Command::Start.digest().unwrap(),
    };
    assert!(matches!(
        journal.execute(&actor(), &request, &mut runtime.bind(Command::Start)),
        Err(Error::InvalidInput)
    ));
    assert_eq!(runtime.status().unwrap().ownership, "stopped");
}
