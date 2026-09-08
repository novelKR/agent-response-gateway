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
