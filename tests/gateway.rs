use std::{
    collections::BTreeMap,
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use agent_response_gateway::{Config, Secrets, router};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    routing::post,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle, time::timeout};

const LOCAL_TOKEN: &str = "local-token-0123456789-abcdefghijklmnop";
const UPSTREAM_TOKEN: &str = "upstream-secret-never-return-this";

struct Server {
    base_url: String,
    task: JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(app: Router) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { base_url, task }
}

#[derive(Clone, Debug)]
struct Recorded {
    headers: HeaderMap,
    body: Value,
}

#[derive(Clone)]
enum Mode {
    Json,
    Error,
    Redirect,
    WrongMediaType,
    EncodedSse,
    LargeJson,
    SlowHeaders,
    StalledJson,
    StalledSse,
    SplitSse,
    EndlessSse(Arc<AtomicBool>),
}

#[derive(Clone)]
struct Mock {
    requests: Arc<Mutex<Vec<Recorded>>>,
    mode: Mode,
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn upstream(
    State(mock): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response<Body> {
    mock.requests
        .lock()
        .unwrap()
        .push(Recorded { headers, body });
    let (status, media, body) = match mock.mode {
        Mode::Json => (
            StatusCode::OK,
            "application/json",
            Body::from(json!({"id":"resp_mock","object":"response","output":[]}).to_string()),
        ),
        Mode::Error => (
            StatusCode::TOO_MANY_REQUESTS,
            "application/json",
            Body::from(format!("{{\"error\":\"{UPSTREAM_TOKEN} secret prompt\"}}")),
        ),
        Mode::Redirect => {
            return Response::builder()
                .status(StatusCode::TEMPORARY_REDIRECT)
                .header("location", "/v1/responses")
                .body(Body::from(UPSTREAM_TOKEN))
                .unwrap();
        }
        Mode::WrongMediaType => (StatusCode::OK, "text/html", Body::from(UPSTREAM_TOKEN)),
        Mode::EncodedSse => {
            return Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .header("content-encoding", "gzip")
                .body(Body::from(UPSTREAM_TOKEN))
                .unwrap();
        }
        Mode::LargeJson => (
            StatusCode::OK,
            "application/json",
            Body::from(json!({"output":"x".repeat(4096)}).to_string()),
        ),
        Mode::SlowHeaders => {
            tokio::time::sleep(Duration::from_secs(2)).await;
            (StatusCode::OK, "application/json", Body::from("{}"))
        }
        Mode::StalledJson => {
            let stream = async_stream::stream! {
                yield Ok::<_, Infallible>(Bytes::from_static(b"{\"output\":"));
                std::future::pending::<()>().await;
            };
            (
                StatusCode::OK,
                "application/json",
                Body::from_stream(stream),
            )
        }
        Mode::StalledSse => {
            let stream = async_stream::stream! {
                yield Ok::<_, Infallible>(Bytes::from_static(b"event: response.created\ndata: {}\n\n"));
                std::future::pending::<()>().await;
            };
            (
                StatusCode::OK,
                "text/event-stream",
                Body::from_stream(stream),
            )
        }
        Mode::SplitSse => {
            let wire = split_sse_wire();
            let stream = async_stream::stream! {
                // Deliberately split every byte, including Korean UTF-8 and JSON escapes.
                for byte in wire {
                    yield Ok::<_, Infallible>(Bytes::from(vec![byte]));
                    tokio::task::yield_now().await;
                }
            };
            (
                StatusCode::OK,
                "text/event-stream",
                Body::from_stream(stream),
            )
        }
        Mode::EndlessSse(dropped) => {
            let stream = async_stream::stream! {
                let _guard = DropFlag(dropped);
                yield Ok::<_, Infallible>(Bytes::from_static(b"event: response.created\ndata: {}\n\n"));
                // Delay the next event so observing the first proves streaming, not collection.
                tokio::time::sleep(Duration::from_millis(350)).await;
                loop {
                    yield Ok::<_, Infallible>(Bytes::from(format!(":{}\n\n", "x".repeat(32 * 1024))));
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            };
            (
                StatusCode::OK,
                "text/event-stream",
                Body::from_stream(stream),
            )
        }
    };
    Response::builder()
        .status(status)
        .header("content-type", media)
        .header("set-cookie", "provider-session=private")
        .header("x-provider-private", UPSTREAM_TOKEN)
        .body(body)
        .unwrap()
}

fn split_sse_wire() -> Vec<u8> {
    concat!(
        "event: response.output_text.delta\ndata: {\"delta\":\"한글 테스트\"}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"delta\":\"{\\\"input\\\":\\\"a\\\\nb\\\"}\"}\n\n",
        "event: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
    )
    .as_bytes()
    .to_vec()
}

fn config_for(base_url: &str) -> Config {
    Config::parse(&format!(
        r#"
source_url = "https://example.org/source/v0.1.0"
[providers.mock]
base_url = "{base_url}/v1"
api_key_env = "UNUSED_UPSTREAM_KEY"
[models.writer]
provider = "mock"
upstream_model = "actual-model"
"#
    ))
    .unwrap()
}

fn secrets() -> Secrets {
    Secrets {
        local_token: LOCAL_TOKEN.to_string(),
        upstream_keys: BTreeMap::from([("mock".to_string(), UPSTREAM_TOKEN.to_string())]),
    }
}

struct Harness {
    gateway: Server,
    _upstream: Server,
    mock: Mock,
    client: reqwest::Client,
}

impl Harness {
    async fn new(mode: Mode, configure: impl FnOnce(&mut Config)) -> Self {
        let mock = Mock {
            requests: Arc::new(Mutex::new(Vec::new())),
            mode,
        };
        let upstream = serve(
            Router::new()
                .route("/v1/responses", post(upstream))
                .with_state(mock.clone()),
        )
        .await;
        let mut config = config_for(&upstream.base_url);
        configure(&mut config);
        let gateway = serve(router(config, secrets()).unwrap()).await;
        Self {
            gateway,
            _upstream: upstream,
            mock,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }

    fn post(&self) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}/v1/responses", self.gateway.base_url))
            .bearer_auth(LOCAL_TOKEN)
    }

    fn calls(&self) -> usize {
        self.mock.requests.lock().unwrap().len()
    }
}

#[test]
fn config_rejects_unsafe_endpoints_unknown_fields_and_invalid_limits() {
    let valid = config_for("http://127.0.0.1:12345");
    assert_eq!(
        valid.providers["mock"].responses_url().unwrap().as_str(),
        "http://127.0.0.1:12345/v1/responses"
    );
    for endpoint in [
        "http://example.org/v1",
        "http://localhost/v1",
        "https://user:password@example.org/v1",
        "https://example.org/v1?token=secret",
        "https://example.org/v1#fragment",
        "file:///tmp/provider",
    ] {
        let mut config = valid.clone();
        config.providers.get_mut("mock").unwrap().base_url = endpoint.into();
        assert!(config.validate().is_err(), "accepted {endpoint}");
    }
    let mut config = valid.clone();
    config.listen = "0.0.0.0:9000".parse().unwrap();
    assert!(config.validate().is_err());
    let mut config = valid.clone();
    config.models.get_mut("writer").unwrap().provider = "absent".into();
    assert!(config.validate().is_err());
    let mut config = valid.clone();
    config.limits.max_in_flight = 0;
    assert!(config.validate().is_err());
    let mut config = valid;
    config.limits.stream_idle_timeout_ms = 0;
    assert!(config.validate().is_err());
    assert!(Config::parse("not_a_setting = 'do-not-echo-secret'").is_err());
    assert!(
        !Config::parse("not_a_setting = 'do-not-echo-secret'")
            .unwrap_err()
            .to_string()
            .contains("do-not-echo-secret")
    );
}

#[test]
fn secrets_require_distinct_valid_credentials() {
    let config = config_for("http://127.0.0.1:12345");
    let mut values = secrets();
    values.local_token = "too-short".into();
    assert!(values.validate(&config).is_err());
    let mut values = secrets();
    values
        .upstream_keys
        .insert("mock".into(), LOCAL_TOKEN.into());
    assert!(values.validate(&config).is_err());
    let mut values = secrets();
    values.upstream_keys.clear();
    assert!(values.validate(&config).is_err());
    assert!(secrets().validate(&config).is_ok());
}

#[tokio::test]
async fn health_and_models_do_not_disclose_upstream_configuration() {
    let harness = Harness::new(Mode::Json, |_| {}).await;
    for path in ["/healthz", "/readyz", "/"] {
        let response = harness
            .client
            .get(format!("{}{path}", harness.gateway.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.unwrap();
        assert!(!body.contains(UPSTREAM_TOKEN));
        assert!(!body.contains("actual-model"));
    }
    let url = format!("{}/v1/models", harness.gateway.base_url);
    assert_eq!(
        harness.client.get(&url).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let response = harness
        .client
        .get(url)
        .bearer_auth(LOCAL_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["data"][0]["id"], "writer");
    assert!(!body.to_string().contains("actual-model"));
    assert!(!body.to_string().contains(UPSTREAM_TOKEN));
    assert_eq!(harness.calls(), 0);
}

#[tokio::test]
async fn authenticates_locally_and_preserves_responses_semantics() {
    let harness = Harness::new(Mode::Json, |_| {}).await;
    let request = json!({
        "model":"writer",
        "input":[
            {"role":"user","content":[{"type":"input_text","text":"합성 시험 원고"}]},
            {"type":"custom_tool_call","call_id":"call_one","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"},
            {"type":"custom_tool_call_output","call_id":"call_one","output":"done"}
        ],
        "tools":[{"type":"custom","name":"apply_patch","format":{"type":"text"}}],
        "text":{"format":{"type":"json_schema","name":"result","strict":true,"schema":{"type":"object","properties":{},"additionalProperties":false}}},
        "reasoning":{"effort":"high"},
        "metadata":{"synthetic":true},
        "stream":false
    });
    let response = harness
        .post()
        .header("cookie", "local-session=private")
        .header("x-client-only", "not-for-the-provider")
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    assert!(response.headers().get("x-provider-private").is_none());
    assert_eq!(response.json::<Value>().await.unwrap()["id"], "resp_mock");
    let records = harness.mock.requests.lock().unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(
        record.headers["authorization"],
        format!("Bearer {UPSTREAM_TOKEN}")
    );
    assert!(record.headers.get("cookie").is_none());
    assert!(record.headers.get("x-client-only").is_none());
    assert_eq!(record.headers["accept-encoding"], "identity");
    let mut expected = request;
    expected["model"] = json!("actual-model");
    expected["store"] = json!(false);
    assert_eq!(record.body, expected);
}

#[tokio::test]
async fn rejects_authentication_and_unsupported_requests_before_dispatch() {
    let harness = Harness::new(Mode::Json, |_| {}).await;
    for credential in [None, Some("incorrect-local-token")] {
        let mut request = harness
            .client
            .post(format!("{}/v1/responses", harness.gateway.base_url));
        if let Some(credential) = credential {
            request = request.bearer_auth(credential);
        }
        assert_eq!(
            request
                .json(&json!({"model":"writer"}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        harness
            .post()
            .json(&json!({"model":"unknown"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let unsupported = [
        json!({"model":"writer","store":true}),
        json!({"model":"writer","store":null}),
        json!({"model":"writer","store":"false"}),
        json!({"model":"writer","background":true}),
        json!({"model":"writer","background":"false"}),
        json!({"model":"writer","previous_response_id":"resp_other"}),
        json!({"model":"writer","conversation":"conv_other"}),
        json!({"model":"writer","context_management":[{"type":"compaction"}]}),
        json!({"model":"writer","input":[{"type":"item_reference","id":"item_stored"}]}),
        json!({"model":"writer","input":[{"id":"item_stored"}]}),
        json!({"model":"writer","input":[{"id":"item_stored","type":null}]}),
        json!({"model":"writer","input":[{"type":"compaction","encrypted_content":"synthetic-state"}]}),
        json!({"model":"writer","input":[{"type":"compaction_trigger"}]}),
        json!({"model":"writer","stream":"true"}),
        json!({"model":null}),
        json!({"input":"missing model"}),
        json!([]),
    ];
    for body in unsupported {
        let response = harness.post().json(&body).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    assert_eq!(harness.calls(), 0);
}

#[tokio::test]
async fn preserves_large_integers_and_decimal_precision_in_raw_json() {
    let harness = Harness::new(Mode::Json, |_| {}).await;
    let raw = r#"{
        "model":"writer",
        "input":"synthetic numeric precision fixture",
        "metadata":{
            "large_integer":184467440737095516161234567890123456789,
            "precise_decimal":0.123456789012345678901234567890123456789
        }
    }"#;
    let response = harness
        .post()
        .header("content-type", "application/json")
        .body(raw)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let records = harness.mock.requests.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].body["metadata"]["large_integer"].to_string(),
        "184467440737095516161234567890123456789"
    );
    assert_eq!(
        records[0].body["metadata"]["precise_decimal"].to_string(),
        "0.123456789012345678901234567890123456789"
    );
}

#[tokio::test]
async fn enforces_request_size_and_body_deadline_without_upstream_calls() {
    let harness = Harness::new(Mode::Json, |config| {
        config.limits.max_request_bytes = 128;
        config.limits.request_body_timeout_ms = 120;
    })
    .await;
    let response = harness
        .post()
        .json(&json!({"model":"writer","input":"x".repeat(1024)}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let incomplete = async_stream::stream! {
        yield Ok::<_, Infallible>(Bytes::from_static(b"{\"model\":"));
        std::future::pending::<()>().await;
    };
    let response = timeout(
        Duration::from_secs(3),
        harness
            .post()
            .header("content-type", "application/json")
            .body(reqwest::Body::wrap_stream(incomplete))
            .send(),
    )
    .await
    .expect("incomplete request must hit the body deadline")
    .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert_eq!(harness.calls(), 0);
}

#[tokio::test]
async fn sse_preserves_split_utf8_tool_json_and_event_order() {
    let harness = Harness::new(Mode::SplitSse, |config| {
        // A total JSON body limit must not truncate a longer streaming response.
        config.limits.max_response_bytes = 128;
    })
    .await;
    let response = harness
        .post()
        .json(&json!({"model":"writer","stream":true,"input":"synthetic"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    assert_eq!(response.bytes().await.unwrap().as_ref(), split_sse_wire());
    assert_eq!(harness.calls(), 1);
}

#[tokio::test]
async fn stalled_sse_terminates_without_a_fabricated_completion_and_releases_capacity() {
    let harness = Harness::new(Mode::StalledSse, |config| {
        config.limits.stream_idle_timeout_ms = 120;
        config.limits.max_in_flight = 1;
    })
    .await;
    let response = harness
        .post()
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream();
    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(first.as_ref(), b"event: response.created\ndata: {}\n\n");
    let end = timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("idle stream must terminate");
    assert!(
        end.is_none() || end.unwrap().is_err(),
        "must not fabricate a success event"
    );
    drop(stream);
    let next = harness
        .post()
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(next.status(), StatusCode::OK);
    assert_eq!(harness.calls(), 2);
}

#[tokio::test]
async fn stored_response_and_compaction_endpoints_are_explicitly_unsupported() {
    let harness = Harness::new(Mode::Json, |_| {}).await;
    let response = harness
        .client
        .post(format!("{}/v1/responses/compact", harness.gateway.base_url))
        .bearer_auth(LOCAL_TOKEN)
        .json(&json!({"model":"writer"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let response = harness
            .client
            .request(
                method,
                format!("{}/v1/responses/resp_old", harness.gateway.base_url),
            )
            .bearer_auth(LOCAL_TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }
    assert_eq!(harness.calls(), 0);
}

#[tokio::test]
async fn streams_promptly_holds_capacity_and_cancels_on_disconnect() {
    let dropped = Arc::new(AtomicBool::new(false));
    let harness = Harness::new(Mode::EndlessSse(dropped.clone()), |config| {
        config.limits.max_in_flight = 1;
    })
    .await;
    let response = harness
        .post()
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream();
    let first = timeout(Duration::from_millis(250), stream.next())
        .await
        .expect("first event must arrive before subsequent upstream events")
        .unwrap()
        .unwrap();
    assert!(first.starts_with(b"event: response.created"));
    assert!(!dropped.load(Ordering::SeqCst));
    let overloaded = harness
        .post()
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(overloaded.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(harness.calls(), 1);
    drop(stream);
    timeout(Duration::from_secs(3), async {
        while !dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("disconnect must drop the upstream producer");
    let next = harness
        .post()
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        next.status(),
        StatusCode::OK,
        "permit must be released after disconnect"
    );
    drop(next);
}

#[tokio::test]
async fn sanitizes_upstream_errors_and_never_replays_redirects() {
    for (mode, expected) in [
        (Mode::Error, StatusCode::TOO_MANY_REQUESTS),
        (Mode::Redirect, StatusCode::BAD_GATEWAY),
        (Mode::WrongMediaType, StatusCode::BAD_GATEWAY),
    ] {
        let harness = Harness::new(mode, |_| {}).await;
        let response = harness
            .post()
            .json(&json!({"model":"writer"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert!(response.headers().get("location").is_none());
        assert!(response.headers().get("set-cookie").is_none());
        let body = response.text().await.unwrap();
        assert!(!body.contains(UPSTREAM_TOKEN));
        assert!(!body.contains("secret prompt"));
        assert_eq!(
            harness.calls(),
            1,
            "requests must not be retried or redirected"
        );
    }
}

#[tokio::test]
async fn limits_json_response_size() {
    let harness = Harness::new(Mode::LargeJson, |config| {
        config.limits.max_response_bytes = 128;
    })
    .await;
    let response = harness
        .post()
        .json(&json!({"model":"writer"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(harness.calls(), 1);
}

#[tokio::test]
async fn requests_identity_encoding_and_rejects_encoded_sse_without_exposing_body() {
    let harness = Harness::new(Mode::EncodedSse, |_| {}).await;
    let response = harness
        .post()
        .header("accept-encoding", "gzip")
        .json(&json!({"model":"writer","stream":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(response.headers().get("content-encoding").is_none());
    let body = response.text().await.unwrap();
    assert!(!body.contains(UPSTREAM_TOKEN));
    let records = harness.mock.requests.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].headers["accept-encoding"], "identity");
}

#[tokio::test]
async fn times_out_response_headers_and_stalled_json_without_retry() {
    for mode in [Mode::SlowHeaders, Mode::StalledJson] {
        let harness = Harness::new(mode, |config| {
            config.limits.response_header_timeout_ms = 120;
            config.limits.stream_idle_timeout_ms = 120;
        })
        .await;
        let response = timeout(
            Duration::from_secs(3),
            harness.post().json(&json!({"model":"writer"})).send(),
        )
        .await
        .expect("upstream request must time out")
        .unwrap();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(harness.calls(), 1);
    }
}

#[tokio::test]
async fn network_failure_is_a_sanitized_gateway_error() {
    // Reserve then close a loopback port; no external host is contacted.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let gateway = serve(router(config_for(&base_url), secrets()).unwrap()).await;
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
        .post(format!("{}/v1/responses", gateway.base_url))
        .bearer_auth(LOCAL_TOKEN)
        .json(&json!({"model":"writer"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = response.text().await.unwrap();
    assert!(!body.contains(&base_url));
    assert!(!body.contains(UPSTREAM_TOKEN));
}
