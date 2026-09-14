//! Synthetic independent provider exercised through the shared host HTTP transport.
use crate::{Config, Secrets, extensions::ExtensionPlan};
use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

struct Upstream {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn upstream(app: Router) -> Upstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Upstream { url, task }
}
fn python() -> String {
    std::env::var("MANAGEMENT_TEST_PYTHON").unwrap_or_else(|_| "python3".into())
}
fn command(arguments: &[&str]) -> Vec<u8> {
    let output = Command::new(python())
        .arg("-B")
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic package tool failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn target() -> String {
    format!(
        "{}-{}",
        if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        },
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        }
    )
}
struct Package {
    _directory: tempfile::TempDir,
    lock: PathBuf,
}
fn package(inert_transport_fields: bool) -> Package {
    package_patched(inert_transport_fields, None)
}
fn package_patched(inert_transport_fields: bool, patch: Option<(&str, &str)>) -> Package {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let parent = root.join(".local/provider-proxy-tests");
    fs::create_dir_all(&parent).unwrap();
    let directory = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    let project = directory.path().join("external");
    fs::create_dir(&project).unwrap();
    let source = root.join("tools/plugin-conformance/examples/provider");
    for file in fs::read_dir(&source).unwrap() {
        let file = file.unwrap();
        if file.file_type().unwrap().is_file() {
            fs::copy(file.path(), project.join(file.file_name())).unwrap();
        }
    }
    if inert_transport_fields {
        let script = project.join("provider.py");
        let text = fs::read_to_string(&script).unwrap();
        let original = "payload = {'query': value['request'], 'cursor': self.counter}";
        assert!(text.contains(original));
        fs::write(script,text.replace(original,"payload = {'query': value['request'], 'cursor': self.counter, 'url': 'http://127.0.0.1:1/forbidden', 'headers': {'Authorization': 'plugin-cannot-select-auth'}}")).unwrap();
    }
    if let Some((old, new)) = patch {
        let path = project.join("provider.py");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(old));
        fs::write(path, text.replace(old, new)).unwrap();
    }
    let built = directory.path().join("package");
    let digest = String::from_utf8(command(&[
        project.join("package.py").to_str().unwrap(),
        "--output",
        built.to_str().unwrap(),
        "--target",
        &target(),
    ]))
    .unwrap()
    .trim()
    .to_owned();
    let store = directory.path().join("store");
    command(&[
        root.join("scripts/extension_manager.py").to_str().unwrap(),
        "install",
        "--store",
        store.to_str().unwrap(),
        "--package",
        built.to_str().unwrap(),
        "--expected-sha256",
        &digest,
    ]);
    // Only this internal qualification harness selects the unavailable role.
    let state = store.join("state/synthetic-provider").join(&digest);
    fs::create_dir_all(&state).unwrap();
    for p in state.ancestors().take_while(|p| p.starts_with(&store)) {
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let lock = store.join("active.json");
    let value = json!({"schema":"gateway-extension-lock/v1","generation":1,"extensions":[{"id":"synthetic-provider","version":"1.0.0","package_sha256":digest,"grants":["read_model_payload","transform_model_protocol"]}]});
    fs::write(&lock, format!("{value}\n")).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    Package {
        _directory: directory,
        lock,
    }
}
fn configuration(package: &Package, upstream: &str) -> Config {
    let plan = ExtensionPlan::load_provider_qualification(&package.lock).unwrap();
    let text = format!(
        r#"
[providers.synthetic]
base_url="{upstream}/vendor"
api_key_env="SYNTHETIC_KEY"
[models.demo]
provider="synthetic"
upstream_model="synthetic-model"
api="plugin"
auth="bearer"
provider_plugin="synthetic-provider"
provider_protocol="synthetic-provider/v1"
provider_path="generate"
capability_profile="synthetic"
[capability_profiles.synthetic]
version="1"
provider="synthetic"
upstream_model="synthetic-model"
api="plugin"
context_window=4096
max_output_tokens=128
tested_codex_version="0.154.0"
[capability_profiles.synthetic.support]
function_tools="native"
tool_choice="native"
max_output_tokens="native"
"#
    );
    let config = Config::parse_provider_qualification(&text, &plan).unwrap();
    assert!(
        config
            .resolved_usage_profile(&config.models["demo"])
            .is_none()
    );
    config
}
fn gateway(package: &Package, upstream: &str) -> Router {
    crate::router(configuration(package, upstream), fixture_secrets()).unwrap()
}
fn fixture_secrets() -> Secrets {
    Secrets {
        local_token: "L".repeat(40),
        upstream_keys: BTreeMap::from([("synthetic".into(), "host-selected-key".into())]),
    }
}

async fn request(app: Router, value: Value) -> (Upstream, reqwest::Response) {
    let server = upstream(app).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let response = client
        .post(format!("{}/v1/responses", server.url))
        .bearer_auth("L".repeat(40))
        .json(&value)
        .send()
        .await
        .unwrap();
    (server, response)
}
async fn body(response: reqwest::Response) -> Vec<u8> {
    response.bytes().await.unwrap().to_vec()
}
fn native(answer: Value) -> Value {
    json!({"answer":answer,"meter":{"input_tokens":7,"output_tokens":2,"total_tokens":9}})
}

#[tokio::test]
async fn independent_provider_json_keeps_transport_and_auth_host_owned() {
    let package = package(true);
    let server = upstream(Router::new().route(
        "/vendor/generate",
        post(
            |headers: axum::http::HeaderMap, axum::Json(value): axum::Json<Value>| async move {
                assert_eq!(headers[header::AUTHORIZATION], "Bearer host-selected-key");
                assert_eq!(
                    value["headers"]["Authorization"],
                    "plugin-cannot-select-auth"
                );
                assert_eq!(value["query"]["model"], "synthetic-model");
                assert!(value.get("model").is_none());
                axum::Json(native(json!([{"text":"independent answer"}])))
            },
        ),
    ))
    .await;
    let (_gateway, response) = request(
        gateway(&package, &server.url),
        json!({"model":"demo","input":"synthetic input"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(&body(response).await).unwrap();
    assert_eq!(
        value["output"][0]["content"][0]["text"],
        "independent answer"
    );
    assert_eq!(value["usage"]["input_tokens"], 7);
    assert!(value["id"].as_str().unwrap().starts_with("resp_"));
}

#[tokio::test]
async fn provider_sse_delivers_progress_before_upstream_semantic_completion() {
    let package = package(false);
    let finish = Arc::new(tokio::sync::Notify::new());
    let server_finish = finish.clone();
    let server=upstream(Router::new().route("/vendor/generate",post(move||{
        let finish=server_finish.clone();async move {
            let stream=async_stream::stream! {
                yield Ok::<_,std::io::Error>(axum::body::Bytes::from_static(b"event: piece\ndata: {\"text\":\"incremental\"}\n\n"));
                finish.notified().await;
                yield Ok(axum::body::Bytes::from(format!("event: end\ndata: {}\n\n",native(json!([{"text":"incremental"}])))));
            };
            ([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream)).into_response()
        }
    }))).await;
    let (_gateway, response) = request(
        gateway(&package, &server.url),
        json!({"model":"demo","input":"synthetic input","stream":true}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut chunks = response.bytes_stream();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = chunks.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
            if String::from_utf8_lossy(&bytes).contains("response.output_text.delta") {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("incremental"));
    assert!(!String::from_utf8_lossy(&bytes).contains("response.completed"));
    finish.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = chunks.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
    })
    .await
    .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("response.output_text.done"));
    assert!(text.contains("response.completed"));
}

#[tokio::test]
async fn provider_function_result_roundtrip_preserves_admitted_tool_identity() {
    let package = package(false);
    let server = upstream(Router::new().route(
        "/vendor/generate",
        post(|axum::Json(value): axum::Json<Value>| async move {
            let result = value["query"]["input"].as_array().and_then(|items| {
                items
                    .iter()
                    .find(|item| item["type"] == "function_call_output")
            });
            match result {
                Some(result) => {
                    assert_eq!(result["call_id"], "synthetic_call");
                    assert_eq!(result["output"], "42");
                    axum::Json(native(json!([{"text":"tool result accepted"}])))
                }
                None => axum::Json(native(
                    json!([{"call":"synthetic_call","name":"lookup","arguments":"{}"}]),
                )),
            }
        }),
    ))
    .await;
    let tools = json!([{"type":"function","name":"lookup","parameters":{"type":"object","properties":{},"additionalProperties":false}}]);
    let (_gateway, response) = request(
        gateway(&package, &server.url),
        json!({"model":"demo","input":"use the synthetic tool","tools":tools}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let first: Value = serde_json::from_slice(&body(response).await).unwrap();
    let call = first["output"][0].clone();
    assert_eq!(call["type"], "function_call");
    assert_eq!(call["name"], "lookup");
    let (_gateway, response)=request(gateway(&package,&server.url),json!({"model":"demo","tools":tools,"input":[call,{"type":"function_call_output","call_id":"synthetic_call","output":"42"}]})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let second: Value = serde_json::from_slice(&body(response).await).unwrap();
    assert_eq!(
        second["output"][0]["content"][0]["text"],
        "tool result accepted"
    );
}

struct ManagedHost {
    url: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for ManagedHost {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
impl ManagedHost {
    async fn close(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
        }
    }
}
async fn managed_host(
    package: &Package,
    url: &str,
    directory: &Path,
    initialize: bool,
) -> (
    ManagedHost,
    crate::continuation::Runtime,
    crate::continuation::Origin,
) {
    managed_host_with_usage(package, url, directory, initialize, None).await
}
async fn managed_host_with_usage(
    package: &Package,
    url: &str,
    directory: &Path,
    initialize: bool,
    usage: Option<crate::usage::UsageSink>,
) -> (
    ManagedHost,
    crate::continuation::Runtime,
    crate::continuation::Origin,
) {
    use crate::continuation::{ContinuationStore, Protector, Runtime, SqliteStore};
    fs::create_dir_all(directory).unwrap();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    let directory = directory.canonicalize().unwrap();
    let mut store = SqliteStore::open(&directory, initialize, 64 * 1024 * 1024).unwrap();
    let store_id = store.identity().unwrap();
    let key = [7; 32];
    let key_id = "synthetic-key";
    let binding_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key);
    let fingerprint = crate::continuation::hex(
        ring::hmac::sign(
            &binding_key,
            format!("gateway-store-protection/v1:{store_id}:{key_id}").as_bytes(),
        )
        .as_ref(),
    );
    store.bind_protection(&fingerprint).unwrap();
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new(key_id.into(), &key).unwrap(),
    );
    let mut config = configuration(package, url);
    config.models.get_mut("demo").unwrap().continuation_mode =
        Some(crate::config::ContinuationMode::Managed);
    config.continuation = Some(crate::continuation::Configuration {
        directory,
        store_id,
        realm: "synthetic-realm".into(),
        generation: "1".into(),
        key_id: key_id.into(),
        key_env: "SYNTHETIC_STATE_KEY".into(),
        control_token_env: "SYNTHETIC_CONTROL_TOKEN".into(),
        max_store_bytes: 64 * 1024 * 1024,
    });
    config.validate().unwrap();
    let secrets = fixture_secrets();
    secrets.validate(&config).unwrap();
    let manifest = config.manifest().unwrap();
    assert_eq!(
        serde_json::to_value(&manifest).unwrap()["schema"],
        "gateway-embedded-manifest/v10"
    );
    let mut route = manifest.configuration()["routes"][0].clone();
    route.as_object_mut().unwrap().remove("api_key_env");
    let origin = crate::continuation::Origin {
        route,
        realm: "synthetic-realm".into(),
        generation: "1".into(),
    };
    let state = Arc::new(crate::http::GatewayState {
        configuration_sha256: manifest.configuration_sha256().into(),
        slots: Arc::new(tokio::sync::Semaphore::new(config.limits.max_in_flight)),
        config,
        secrets,
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .unwrap(),
        continuation: Some(runtime.clone()),
        usage,
    });
    // Reuse production routing/authentication with an explicitly injected test
    // protector, avoiding process-global environment mutations in parallel tests.
    let app = crate::http::state_router(state, None, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (
        ManagedHost {
            url,
            stop: Some(stop),
            task: Some(task),
        },
        runtime,
        origin,
    )
}
async fn managed_request(host: &ManagedHost, session: &str, input: Value) -> (StatusCode, Vec<u8>) {
    let response = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .post(format!("{}/v1/responses", host.url))
        .bearer_auth("L".repeat(40))
        .header("x-gateway-session", session)
        .json(&json!({"model":"demo","input":input}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.bytes().await.unwrap().to_vec())
}
fn user_input(text: &str) -> Value {
    json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})
}

#[tokio::test]
async fn managed_provider_restarts_resumes_and_rejects_changed_package_and_corruption() {
    use crate::continuation::ReplayRecord;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let package = package(false);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let server = upstream(Router::new().route(
        "/vendor/generate",
        post(move |axum::Json(value): axum::Json<Value>| {
            let calls = observed.clone();
            async move {
                let turn = calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(value["cursor"], turn);
                assert!(!value.to_string().contains("arg-continuation"));
                axum::Json(native(json!([{"text":format!("turn {turn}")}])))
            }
        }),
    ))
    .await;
    let ledger = package._directory.path().join("ledger");
    let (host, runtime, origin) = managed_host(&package, &server.url, &ledger, true).await;
    let session = runtime
        .access(move |store, _| store.create(&origin))
        .await
        .unwrap();
    let session_id = session.id.clone();
    let (status, bytes) = managed_request(&host, &session_id, json!([user_input("first")])).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let first: Value = serde_json::from_slice(&bytes).unwrap();
    let token = first["output"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item["encrypted_content"].as_str())
        .unwrap();
    assert!(token.starts_with("arg-continuation-v3."));
    let mut input = vec![user_input("first")];
    input.extend(first["output"].as_array().unwrap().clone());
    input.push(user_input("second"));
    drop(runtime);
    host.close().await;

    let changed_package = self::package(true);
    let (changed, changed_runtime, _) =
        managed_host(&changed_package, &server.url, &ledger, false).await;
    let (status, _) = managed_request(&changed, &session_id, json!(input)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(changed_runtime);
    changed.close().await;

    let (host, runtime, _) = managed_host(&package, &server.url, &ledger, false).await;
    let mut corrupt = input.clone();
    let carrier = corrupt
        .iter_mut()
        .find(|item| item.get("encrypted_content").is_some())
        .unwrap();
    carrier["encrypted_content"] = json!(format!(
        "{}x",
        carrier["encrypted_content"].as_str().unwrap()
    ));
    let (status, _) = managed_request(&host, &session_id, json!(corrupt)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (status, bytes) = managed_request(&host, &session_id, json!(input)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let second: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(second["output"][0]["content"][0]["text"], "turn 1");
    let mut third_input = input.clone();
    third_input.extend(second["output"].as_array().unwrap().clone());
    third_input.push(user_input("third"));
    let (status, bytes) = managed_request(&host, &session_id, json!(third_input)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let third: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(third["output"][0]["content"][0]["text"], "turn 2");
    let token = third["output"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item["encrypted_content"].as_str())
        .unwrap()
        .to_owned();
    let lookup = session_id.clone();
    let current = runtime
        .access(move |store, _| store.session(&lookup))
        .await
        .unwrap();
    let record = runtime.restore_record(current, token).await.unwrap();
    assert!(matches!(record, ReplayRecord::V3(_)));
    drop(runtime);
    host.close().await;
}

#[tokio::test]
async fn managed_provider_sse_tool_checkpoint_restarts_and_accepts_exact_tool_result() {
    use crate::continuation::{Outcome, ReplayRecord};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let package = package(false);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let server = upstream(Router::new().route(
        "/vendor/generate",
        post(move |headers: axum::http::HeaderMap, axum::Json(value): axum::Json<Value>| {
            let calls = observed.clone();
            async move {
                assert_eq!(headers[header::AUTHORIZATION], "Bearer host-selected-key");
                assert_eq!(headers[header::CONTENT_TYPE], "application/json");
                assert!(!value.to_string().contains("arg-continuation"));
                let turn = calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(value["cursor"], turn);
                if turn == 0 {
                    assert_eq!(value["query"]["stream"], true);
                    assert_eq!(value["query"]["tools"][0]["name"], "lookup");
                    assert_eq!(headers[header::ACCEPT], "text/event-stream");
                    let data = native(json!([{"call":"call_managed","name":"lookup","arguments":"{}"}]));
                    ([(header::CONTENT_TYPE, "text/event-stream")],
                        Body::from(format!("event: end\ndata: {data}\n\n"))).into_response()
                } else {
                    assert_eq!(turn, 1);
                    assert_eq!(headers[header::ACCEPT], "application/json");
                    assert_ne!(value["query"]["stream"], true);
                    let input = value["query"]["input"].as_array().unwrap();
                    let results: Vec<_> = input.iter().filter(|item|item["type"]=="function_call_output").collect();
                    assert_eq!(results.len(), 1);
                    assert_eq!(results[0], &json!({"type":"function_call_output","call_id":"call_managed","output":"synthetic-result"}));
                    assert!(input.iter().any(|item|item["type"]=="function_call" && item["call_id"]=="call_managed" && item["name"]=="lookup"));
                    axum::Json(native(json!([{"text":"tool result resumed"}]))).into_response()
                }
            }
        }),
    )).await;
    let ledger = package._directory.path().join("sse-ledger");
    let (host, runtime, origin) = managed_host(&package, &server.url, &ledger, true).await;
    let session = runtime
        .access(move |store, _| store.create(&origin))
        .await
        .unwrap();
    let session_id = session.id.clone();
    let first_input = user_input("lookup a synthetic value");
    let response = reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
        .post(format!("{}/v1/responses", host.url))
        .bearer_auth("L".repeat(40))
        .header("x-gateway-session", &session_id)
        .json(&json!({"model":"demo","input":[first_input],"stream":true,
            "tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let bytes = response.bytes().await.unwrap();
    assert!(bytes.len() < 65536);
    let mut decoder = crate::adapters::sse::SseDecoder::new(65536).unwrap();
    let mut remaining = bytes.as_ref();
    let mut events = Vec::new();
    while let Some(event) = decoder.next_event(&mut remaining).unwrap() {
        let value: Value = serde_json::from_str(&event.data).unwrap();
        assert_eq!(event.event, value["type"].as_str().unwrap());
        assert_eq!(value["sequence_number"].as_u64(), Some(events.len() as u64));
        events.push(value);
    }
    assert!(remaining.is_empty());
    assert_eq!(events.last().unwrap()["type"], "response.completed");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "response.completed")
            .count(),
        1
    );
    let first = events.last().unwrap()["response"].clone();
    let output = first["output"].as_array().unwrap();
    let tool = output
        .iter()
        .find(|item| item["type"] == "function_call")
        .unwrap();
    assert_eq!(tool["call_id"], "call_managed");
    assert_eq!(tool["name"], "lookup");
    assert_eq!(tool["arguments"], "{}");
    assert!(events.iter().any(
        |event| event["type"] == "response.function_call_arguments.done"
            && event["item_id"] == tool["id"]
            && event["arguments"] == "{}"
    ));
    let carrier = output
        .iter()
        .find(|item| item["encrypted_content"].is_string())
        .unwrap();
    let token = carrier["encrypted_content"].as_str().unwrap().to_owned();
    assert!(token.starts_with("arg-continuation-v3."));
    let done = events
        .iter()
        .find(|event| {
            event["type"] == "response.output_item.done" && event["item"]["id"] == carrier["id"]
        })
        .unwrap();
    assert_eq!(&done["item"], carrier);
    let lookup = session_id.clone();
    let checkpoint = runtime
        .access(move |store, _| store.session(&lookup))
        .await
        .unwrap();
    assert!(checkpoint.pending_tools);
    assert_eq!(checkpoint.head.as_deref(), first["id"].as_str());
    let record = runtime.restore_record(checkpoint, token).await.unwrap();
    let ReplayRecord::V3(record) = record else {
        panic!("provider V3 checkpoint required");
    };
    assert_eq!(record.outcome, Outcome::AwaitingTools);
    let mut resumed = vec![first_input];
    resumed.extend(output.clone());
    resumed.push(
        json!({"type":"function_call_output","call_id":"call_managed","output":"synthetic-result"}),
    );
    drop(runtime);
    host.close().await;

    let (host, runtime, _) = managed_host(&package, &server.url, &ledger, false).await;
    let lookup = session_id.clone();
    assert!(
        runtime
            .access(move |store, _| store.session(&lookup))
            .await
            .unwrap()
            .pending_tools
    );
    let (status, bytes) = managed_request(&host, &session_id, json!(resumed)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let second: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        second["output"][0]["content"][0]["text"],
        "tool result resumed"
    );
    let token = second["output"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item["encrypted_content"].as_str())
        .unwrap()
        .to_owned();
    let lookup = session_id.clone();
    let checkpoint = runtime
        .access(move |store, _| store.session(&lookup))
        .await
        .unwrap();
    assert!(!checkpoint.pending_tools);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let record = runtime.restore_record(checkpoint, token).await.unwrap();
    let ReplayRecord::V3(record) = record else {
        panic!("provider V3 checkpoint required");
    };
    assert_eq!(record.outcome, Outcome::Completed);
    drop(runtime);
    host.close().await;
}

fn usage_sink(
    accept_start: bool,
    accept_final: bool,
    v2: bool,
) -> (
    crate::usage::UsageSink,
    Arc<std::sync::Mutex<Vec<crate::usage::RecordedEvent>>>,
    tokio::task::JoinHandle<()>,
) {
    use crate::usage::*;
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Delivery>(256);
    let records = Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = records.clone();
    let worker = tokio::spawn(async move {
        while let Some(d) = receiver.recv().await {
            let kind = d.event.view().kind;
            let valid = d.event.bytes().is_ok();
            output.lock().unwrap().push(d.event);
            let _ = d.ack.send(
                valid
                    && (kind != EventKind::AttemptStarted || accept_start)
                    && (kind != EventKind::AttemptFinished || accept_final),
            );
        }
    });
    (
        UsageSink {
            sender,
            mode: Mode::DurableLocal,
            timeout: Duration::from_millis(200),
            producer: "host-recorder".into(),
            supports_v2: v2,
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        },
        records,
        worker,
    )
}
#[tokio::test]
async fn provider_usage_v2_keeps_package_identity_and_numeric_failure_evidence() {
    use crate::usage::*;
    for invalid in [false, true] {
        let package = package(false);
        let server=upstream(Router::new().route("/vendor/generate",post(move||async move{
            axum::Json(json!({"answer":[{"text":"synthetic"}],"meter":{"input_tokens":if invalid{json!(-1)}else{json!(7)},"output_tokens":2,"total_tokens":9}}))
        }))).await;
        let (sink, records, worker) = usage_sink(true, true, true);
        let app = gateway_with_usage(&package, &server.url, Some(sink)).unwrap();
        let (_gateway, response) = request(app, json!({"model":"demo","input":"test"})).await;
        assert_eq!(
            response.status(),
            if invalid {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::OK
            }
        );
        let _ = body(response).await;
        let captured = records.lock().unwrap();
        assert!(!captured.is_empty());
        assert!(captured.iter().all(RecordedEvent::is_v2));
        let final_event = captured
            .iter()
            .find(|e| e.view().kind == EventKind::AttemptFinished)
            .unwrap();
        let RecordedEvent::V2(event) = final_event else {
            unreachable!()
        };
        assert_eq!(
            event.interpretation.provider_protocol,
            "synthetic-provider/v1"
        );
        assert_eq!(event.producer_id, "host-recorder");
        assert!(event.usage.reported.is_empty());
        assert_eq!(event.usage.value("output_tokens"), Some(2));
        if invalid {
            assert_eq!(event.usage.counters["input_tokens"].source, Source::Invalid);
            assert!(event.observation_incomplete);
            assert_eq!(event.gateway, Outcome::ConversionFailed);
        } else {
            assert_eq!(event.finality, Finality::Final);
            assert_eq!(event.usage.value("input_tokens"), Some(7));
        }
        worker.abort();
    }
}
#[tokio::test]
async fn provider_recording_contract_and_durable_barriers_are_enforced() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for (start, finish, v2) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        let package = package(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let server = upstream(Router::new().route(
            "/vendor/generate",
            post(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    axum::Json(native(json!([{"text":"synthetic"}])))
                }
            }),
        ))
        .await;
        let (sink, _, worker) = usage_sink(start, finish, v2);
        let app = gateway_with_usage(&package, &server.url, Some(sink));
        if !v2 {
            assert!(app.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            worker.abort();
            continue;
        }
        let (_gateway, response) =
            request(app.unwrap(), json!({"model":"demo","input":"test"})).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let _ = body(response).await;
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(start));
        worker.abort();
    }
}

#[tokio::test]
async fn provider_sse_tool_terminal_waits_for_v2_durable_final_ack() {
    use crate::usage::*;
    for accept_final in [true, false] {
        let package = package(false);
        let server = upstream(Router::new().route(
            "/vendor/generate",
            post(|| async {
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    format!(
                        "event: end\ndata: {}\n\n",
                        native(json!([{"call":"synthetic_call","name":"lookup","arguments":"{}"}]))
                    ),
                )
            }),
        ))
        .await;
        let (sink, records, worker) = usage_sink(true, accept_final, true);
        let app = gateway_with_usage(&package, &server.url, Some(sink)).unwrap();
        let(_gateway,response)=request(app,json!({"model":"demo","input":"tool","stream":true,"tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{},"additionalProperties":false}}]})).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.bytes_stream();
        let mut data = Vec::new();
        let mut failed = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => data.extend_from_slice(&bytes),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let text = String::from_utf8(data).unwrap();
        assert_eq!(text.contains("response.completed"), accept_final);
        assert_eq!(text.contains("function_call"), accept_final);
        assert_eq!(failed, !accept_final);
        let captured = records.lock().unwrap();
        let event = captured
            .iter()
            .find(|e| e.view().kind == EventKind::AttemptFinished)
            .unwrap();
        assert!(event.is_v2());
        assert_eq!(event.usage().value("input_tokens"), Some(7));
        worker.abort();
    }
}

fn gateway_with_usage(
    package: &Package,
    url: &str,
    usage: Option<crate::usage::UsageSink>,
) -> Result<Router, crate::ConfigError> {
    crate::router_with_usage(configuration(package, url), fixture_secrets(), None, usage)
}

#[tokio::test]
async fn managed_provider_v2_captures_each_failed_exchange_before_drop() {
    use crate::usage::*;
    let invalid_completed = (
        "observation = usage(native)",
        "observation = usage({**native, 'meter': {**native.get('meter', {}), 'input_tokens': -1}})",
    );
    let invalid_start = (
        "return {'result': 'progress', 'events': [], 'complete': False, 'usage': {'kind': 'unobserved'}}",
        "return {'result': 'progress', 'events': [], 'complete': False, 'usage': usage({'meter': {'input_tokens': -1, 'output_tokens': 2, 'total_tokens': 9}})}",
    );
    for scenario in ["json", "event", "finish", "start"] {
        let patch = match scenario {
            "finish" => Some(invalid_completed),
            "start" => Some(invalid_start),
            _ => None,
        };
        let package = package_patched(false, patch);
        let streaming = scenario != "json";
        let invalid = scenario == "json" || scenario == "event";
        let server=upstream(Router::new().route("/vendor/generate",post(move||async move{
            let value=json!({"answer":[{"text":"synthetic"}],"meter":{"input_tokens":if invalid{json!(-1)}else{json!(7)},"output_tokens":2,"total_tokens":9}});
            if streaming{([(header::CONTENT_TYPE,"text/event-stream")],format!("event: end\ndata: {value}\n\n")).into_response()}else{axum::Json(value).into_response()}
        }))).await;
        let directory =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join(".local")).unwrap();
        let (sink, records, worker) = usage_sink(true, true, true);
        let (host, runtime, origin) = managed_host_with_usage(
            &package,
            &server.url,
            &directory.path().join("continuation"),
            true,
            Some(sink),
        )
        .await;
        let session = runtime
            .access(move |store, _| store.create(&origin))
            .await
            .unwrap();
        let response = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap()
            .post(format!("{}/v1/responses", host.url))
            .bearer_auth("L".repeat(40))
            .header("x-gateway-session", &session.id)
            .json(&json!({"model":"demo","input":[user_input("synthetic")],"stream":streaming}))
            .send()
            .await
            .unwrap();
        if streaming {
            assert_eq!(response.status(), StatusCode::OK);
            let mut chunks = response.bytes_stream();
            while let Some(chunk) = chunks.next().await {
                if chunk.is_err() {
                    break;
                }
            }
        } else {
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            let _ = response.bytes().await;
        }
        {
            let captured = records.lock().unwrap();
            let last = captured
                .iter()
                .find(|e| e.view().kind == EventKind::AttemptFinished)
                .unwrap_or_else(|| panic!("missing final {scenario}"));
            assert!(last.is_v2());
            assert_eq!(
                last.usage().counters["input_tokens"].source,
                Source::Invalid,
                "{scenario}"
            );
            assert_eq!(last.usage().value("output_tokens"), Some(2), "{scenario}");
            assert_eq!(last.view().gateway, Outcome::ConversionFailed, "{scenario}");
        }
        let id = session.id;
        let current = runtime
            .access(move |store, _| store.session(&id))
            .await
            .unwrap();
        assert!(current.head.is_none(), "{scenario}");
        drop(runtime);
        host.close().await;
        worker.abort();
    }
}
#[tokio::test]
async fn managed_provider_v2_start_and_final_ack_keep_independent_state_barriers() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for accept_start in [false, true] {
        let package = package(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let server = upstream(Router::new().route(
            "/vendor/generate",
            post(move || {
                let seen = seen.clone();
                async move {
                    seen.fetch_add(1, Ordering::SeqCst);
                    axum::Json(native(json!([{"text":"synthetic"}])))
                }
            }),
        ))
        .await;
        let directory =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join(".local")).unwrap();
        let (sink, records, worker) = usage_sink(accept_start, false, true);
        let (host, runtime, origin) = managed_host_with_usage(
            &package,
            &server.url,
            &directory.path().join("continuation"),
            true,
            Some(sink),
        )
        .await;
        let session = runtime
            .access(move |store, _| store.create(&origin))
            .await
            .unwrap();
        let (status, bytes) =
            managed_request(&host, &session.id, json!([user_input("synthetic")])).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!String::from_utf8_lossy(&bytes).contains("encrypted_content"));
        assert_eq!(calls.load(Ordering::SeqCst), usize::from(accept_start));
        let id = session.id;
        let current = runtime
            .access(move |store, _| store.session(&id))
            .await
            .unwrap();
        assert_eq!(current.head.is_some(), accept_start);
        if accept_start {
            assert!(records.lock().unwrap().iter().any(|e|e.is_v2()&&e.view().kind==crate::usage::EventKind::AttemptFinished));
        }
        drop(runtime);
        host.close().await;
        worker.abort();
    }
}

#[tokio::test]
async fn managed_provider_v2_resume_keeps_exact_interpretation_and_checkpoint_attempt_ids() {
    use crate::usage::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let package = package(false);
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let server = upstream(Router::new().route(
        "/vendor/generate",
        post(move |axum::Json(value): axum::Json<Value>| {
            let seen = seen.clone();
            async move {
                let turn = seen.fetch_add(1, Ordering::SeqCst);
                assert_eq!(value["cursor"], turn);
                axum::Json(native(json!([{"text":format!("turn {turn}")}])))
            }
        }),
    ))
    .await;
    let ledger = package._directory.path().join("ledger-v2");
    let (sink, records, worker) = usage_sink(true, true, true);
    let (host, runtime, origin) =
        managed_host_with_usage(&package, &server.url, &ledger, true, Some(sink.clone())).await;
    let session = runtime
        .access(move |store, _| store.create(&origin))
        .await
        .unwrap();
    let id = session.id.clone();
    let first_input = user_input("first");
    let (status, bytes) = managed_request(&host, &id, json!([first_input.clone()])).await;
    assert_eq!(status, StatusCode::OK);
    let first: Value = serde_json::from_slice(&bytes).unwrap();
    let mut history = vec![first_input];
    history.extend(first["output"].as_array().unwrap().iter().cloned());
    history.push(user_input("second"));
    drop(runtime);
    host.close().await;
    let (host, runtime, _) =
        managed_host_with_usage(&package, &server.url, &ledger, false, Some(sink)).await;
    let (status, bytes) = managed_request(&host, &id, json!(history)).await;
    assert_eq!(status, StatusCode::OK);
    let second: Value = serde_json::from_slice(&bytes).unwrap();
    {
        let captured = records.lock().unwrap();
        let final_events: Vec<_> = captured
            .iter()
            .filter(|e| e.view().kind == EventKind::AttemptFinished)
            .collect();
        assert_eq!(final_events.len(), 2);
        assert_eq!(
            final_events[0].view().attempt_id,
            first["id"].as_str().unwrap()
        );
        assert_eq!(
            final_events[1].view().attempt_id,
            second["id"].as_str().unwrap()
        );
        assert_eq!(
            final_events[0].interpretation(),
            final_events[1].interpretation()
        );
        assert!(
            final_events
                .iter()
                .all(|e| e.is_v2() && e.view().finality == Finality::Final)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    drop(runtime);
    host.close().await;
    worker.abort();
}

#[tokio::test]
async fn managed_provider_v2_sse_tool_output_waits_for_final_ack_after_checkpoint() {
    for accept_final in [true, false] {
        let package = package(false);
        let server = upstream(Router::new().route(
            "/vendor/generate",
            post(|| async {
                (
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    format!(
                        "event: end\ndata: {}\n\n",
                        native(json!([{"call":"synthetic_call","name":"lookup","arguments":"{}"}]))
                    ),
                )
            }),
        ))
        .await;
        let (sink, records, worker) = usage_sink(true, accept_final, true);
        let (host, runtime, origin) = managed_host_with_usage(
            &package,
            &server.url,
            &package._directory.path().join("ledger-ack"),
            true,
            Some(sink),
        )
        .await;
        let session = runtime
            .access(move |store, _| store.create(&origin))
            .await
            .unwrap();
        let response=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build().unwrap().post(format!("{}/v1/responses",host.url)).bearer_auth("L".repeat(40)).header("x-gateway-session",&session.id).json(&json!({"model":"demo","stream":true,"input":[user_input("tool")],"tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{},"additionalProperties":false}}]})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut data = Vec::new();
        let mut failed = false;
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            match chunk {
                Ok(bytes) => data.extend_from_slice(&bytes),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let text = String::from_utf8(data).unwrap();
        assert_eq!(failed, !accept_final);
        assert_eq!(text.contains("response.completed"), accept_final);
        assert_eq!(text.contains("function_call"), accept_final);
        assert_eq!(text.contains("encrypted_content"), accept_final);
        let id = session.id;
        let current = runtime
            .access(move |store, _| store.session(&id))
            .await
            .unwrap();
        assert!(current.head.is_some());
        assert!(current.pending_tools);
        assert!(
            records
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.is_v2() && e.view().kind == crate::usage::EventKind::AttemptFinished)
        );
        drop(runtime);
        host.close().await;
        worker.abort();
    }
}
