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
fn gateway(package: &Package, upstream: &str) -> Router {
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
    crate::router(
        config,
        Secrets {
            local_token: "L".repeat(40),
            upstream_keys: BTreeMap::from([("synthetic".into(), "host-selected-key".into())]),
        },
    )
    .unwrap()
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
