#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use axum::{Json, Router, routing::post};
use serde_json::{Value, json};

const TOKEN: &str = "local-token-for-cli-smoke-0123456789";
const UPSTREAM_KEY: &str = "synthetic-cli-upstream-key";

fn scratch() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-state");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

fn config_file(root: &Path, upstream: &str) -> PathBuf {
    let path = root.join("config.toml");
    std::fs::write(
        &path,
        format!(
            r#"
local_token_env = "ARG_LOCAL_TOKEN"
[limits]
shutdown_grace_ms = 100
[providers.mock]
base_url = "{upstream}/v1"
api_key_env = "ARG_UPSTREAM_KEY"
[models.writer]
provider = "mock"
upstream_model = "actual-model"
"#
        ),
    )
    .unwrap();
    path
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Process {
    fn launch(path: &Path) -> (Self, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agent-response-gateway"))
            .args(["serve", "--config"])
            .arg(path)
            .env_clear()
            .env("ARG_LOCAL_TOKEN", TOKEN)
            .env("ARG_UPSTREAM_KEY", UPSTREAM_KEY)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let process = Self(child);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = tx.send(result);
        });
        let line = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("process announces readiness")
            .unwrap();
        let ready: Value = serde_json::from_str(&line).expect("readiness is one JSON line");
        assert_eq!(ready["event"], "ready");
        (process, ready)
    }

    fn terminate(&mut self) -> String {
        assert!(
            Command::new("/bin/kill")
                .args(["-TERM", &self.0.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "registered handler exits successfully: {status}"
                );
                break;
            }
            assert!(Instant::now() < deadline, "shutdown is bounded");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut logs = String::new();
        self.0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut logs)
            .unwrap();
        logs
    }
}

#[test]
fn check_config_is_offline_and_startup_errors_do_not_echo_secrets() {
    let root = scratch();
    let path = config_file(root.path(), "http://127.0.0.1:1");
    let check = Command::new(env!("CARGO_BIN_EXE_agent-response-gateway"))
        .args(["check-config", "--config"])
        .arg(&path)
        .env_clear()
        .output()
        .unwrap();
    assert!(check.status.success());
    let value: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(value["credentials_checked"], false);
    assert_eq!(value["provider_probe"], false);
    let failed = Command::new(env!("CARGO_BIN_EXE_agent-response-gateway"))
        .args(["serve", "--config"])
        .arg(&path)
        .env_clear()
        .env("ARG_LOCAL_TOKEN", "SENSITIVE-short")
        .env("ARG_UPSTREAM_KEY", UPSTREAM_KEY)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    let message = String::from_utf8(failed.stderr).unwrap();
    assert!(!message.contains("SENSITIVE-short"));
    assert!(!message.contains(UPSTREAM_KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn executable_serves_generic_http_client_and_shuts_down_without_secret_logs() {
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let mock = Router::new().route(
        "/v1/responses",
        post(move |Json(value): Json<Value>| {
            let observed = observed.clone();
            async move {
                assert_eq!(value["model"], "actual-model");
                assert_eq!(value["store"], false);
                observed.fetch_add(1, Ordering::SeqCst);
                Json(json!({"id":"mock_response", "output":[], "synthetic":"okay"}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async {
        axum::serve(listener, mock).await.unwrap();
    });
    let root = scratch();
    let path = config_file(root.path(), &upstream);
    let (mut process, ready) = Process::launch(&path);
    let base_url = ready["base_url"].as_str().unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let response = client
        .post(format!("{base_url}/responses"))
        .bearer_auth(TOKEN)
        .json(&json!({"model":"writer", "input":"SECRET-SYNTHETIC-PROMPT"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap()["id"],
        "mock_response"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let logs = process.terminate();
    for secret in [TOKEN, UPSTREAM_KEY, "SECRET-SYNTHETIC-PROMPT"] {
        assert!(!logs.contains(secret));
    }
    assert!(logs.contains("response_headers"));
    server.abort();
}

#[test]
fn termination_immediately_after_ready_uses_registered_handler() {
    let root = scratch();
    let path = config_file(root.path(), "http://127.0.0.1:1");
    let (mut process, _) = Process::launch(&path);
    process.terminate();
}

#[test]
fn manifest_is_offline_and_readiness_binds_the_same_effective_configuration() {
    let root = scratch();
    let path = config_file(root.path(), "http://127.0.0.1:1");
    let inspect = || {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-response-gateway"))
            .args(["manifest", "--config"])
            .arg(&path)
            .env_clear()
            .env("ARG_LOCAL_TOKEN", "SECRET-INVALID-LOCAL")
            .env("ARG_UPSTREAM_KEY", "SECRET-UPSTREAM-VALUE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "manifest does not validate or read secret values"
        );
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(!text.contains("SECRET-INVALID-LOCAL"));
        assert!(!text.contains("SECRET-UPSTREAM-VALUE"));
        serde_json::from_str::<Value>(&text).unwrap()
    };
    let manifest = inspect();
    let (mut process, ready) = Process::launch(&path);
    assert_eq!(ready["schema"], "gateway-ready/v1");
    assert_eq!(ready["manifest_schema"], manifest["schema"]);
    assert_eq!(ready["version"], manifest["package"]["version"]);
    assert_eq!(
        ready["configuration_sha256"],
        manifest["configuration_sha256"]
    );
    process.terminate();
    let raw = std::fs::read_to_string(&path)
        .unwrap()
        .replace("actual-model", "different-model");
    std::fs::write(&path, raw).unwrap();
    let changed = inspect();
    assert_ne!(
        manifest["configuration_sha256"],
        changed["configuration_sha256"]
    );
    let (mut process, ready) = Process::launch(&path);
    assert_eq!(
        ready["configuration_sha256"],
        changed["configuration_sha256"]
    );
    process.terminate();
}

#[test]
fn bind_failure_never_announces_readiness() {
    let root = scratch();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let path = config_file(root.path(), "http://127.0.0.1:1");
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!("listen=\"{}\"\n{raw}", listener.local_addr().unwrap()),
    )
    .unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_agent-response-gateway"))
        .args(["serve", "--config"])
        .arg(&path)
        .env_clear()
        .env("ARG_LOCAL_TOKEN", TOKEN)
        .env("ARG_UPSTREAM_KEY", UPSTREAM_KEY)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&failed.stderr).contains(UPSTREAM_KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn termination_with_active_response_uses_the_bounded_grace_window() {
    use axum::{
        body::{Body, Bytes},
        http::Response,
    };
    use futures_util::StreamExt;
    let upstream=Router::new().route("/v1/responses",post(||async {
        let stream=async_stream::stream! {
            yield Ok::<_,std::convert::Infallible>(Bytes::from_static(b"event: response.created\ndata: {}\n\n"));
            std::future::pending::<()>().await;
        };
        Response::builder().header("content-type","text/event-stream").body(Body::from_stream(stream)).unwrap()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async { axum::serve(listener, upstream).await.unwrap() });
    let root = scratch();
    let path = config_file(root.path(), &base);
    let (mut process, ready) = Process::launch(&path);
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
        .post(format!("{}/responses", ready["base_url"].as_str().unwrap()))
        .bearer_auth(TOKEN)
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream();
    stream.next().await.unwrap().unwrap();
    let logs = process.terminate();
    assert!(logs.contains("shutdown_grace_expired"));
    let result = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap();
    assert!(result.is_none() || result.unwrap().is_err());
    server.abort();
}
