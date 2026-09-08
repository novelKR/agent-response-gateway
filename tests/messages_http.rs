use agent_response_gateway::{Config, Secrets, ir::ApiProtocol, router};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    routing::post,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle, time::timeout};

const TOKEN: &str = "synthetic-local-token-0123456789-abcdef";
const KEY: &str = "synthetic-upstream-secret";
struct Server {
    url: String,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(app: Router) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { url, task }
}
#[derive(Clone)]
enum Mode {
    Json(String),
    Stream(Vec<Value>),
    Heartbeat,
    Error,
}
#[derive(Clone)]
struct Mock {
    mode: Mode,
    api: ApiProtocol,
    calls: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    release: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}
struct DropFlag(Arc<AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
fn frame(value: &Value, api: ApiProtocol) -> Bytes {
    if api == ApiProtocol::ChatCompletions {
        return Bytes::from(if value == "[DONE]" {
            "data: [DONE]\n\n".into()
        } else {
            format!("data: {value}\n\n")
        });
    }
    Bytes::from(format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap()
    ))
}
fn chat_chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"synthetic","object":"chat.completion.chunk","created":0,"model":"actual-model","choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}
fn start(api: ApiProtocol) -> Value {
    if api == ApiProtocol::ChatCompletions {
        return chat_chunk(json!({"role":"assistant","content":""}), Value::Null);
    }
    json!({"type":"message_start","message":{"id":"synthetic","type":"message","role":"assistant","model":"actual-model","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}})
}
fn text_frames(api: ApiProtocol) -> Vec<Value> {
    if api == ApiProtocol::ChatCompletions {
        return vec![
            chat_chunk(json!({"content":"합성 🧪"}), Value::Null),
            chat_chunk(json!({}), json!("stop")),
            json!("[DONE]"),
        ];
    }
    vec![
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"합성 🧪"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ]
}
fn response_json(api: ApiProtocol) -> String {
    if api == ApiProtocol::ChatCompletions {
        return json!({"id":"synthetic","object":"chat.completion","created":0,"model":"actual-model","choices":[{"index":0,"message":{"role":"assistant","content":"합성 🧪"},"finish_reason":"stop"}]}).to_string();
    }
    json!({"id":"synthetic","type":"message","role":"assistant","model":"actual-model","content":[{"type":"text","text":"합성 🧪"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":3}}).to_string()
}
async fn upstream(
    State(mock): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response<Body> {
    mock.calls.lock().unwrap().push((headers, body));
    let (status, content_type, body) = match mock.mode {
        Mode::Json(raw) => (StatusCode::OK, "application/json", Body::from(raw)),
        Mode::Error => (
            StatusCode::TOO_MANY_REQUESTS,
            "application/json",
            Body::from(format!("{KEY} synthetic body")),
        ),
        mode => {
            let stream = async_stream::stream! {
                let _drop = DropFlag(mock.dropped);
                yield Ok::<_, Infallible>(frame(&start(mock.api), mock.api));
                mock.release.notified().await;
                match mode {
                    Mode::Stream(events) => for event in events {
                        let bytes = frame(&event, mock.api);
                        for byte in bytes { yield Ok(Bytes::copy_from_slice(&[byte])); }
                    },
                    Mode::Heartbeat => loop {
                        yield Ok(Bytes::from(format!(":{}\n\n", "x".repeat(32 * 1024))));
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    },
                    _ => unreachable!(),
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
        .header("content-type", content_type)
        .header("x-private-upstream", KEY)
        .body(body)
        .unwrap()
}
fn config(url: &str, api: ApiProtocol) -> Config {
    let mut raw = format!(
        r#"
[providers.mock]
base_url="{url}/v1"
api_key_env="SYNTHETIC_KEY"
[models.writer]
provider="mock"
upstream_model="actual-model"
api="messages"
auth="api_key"
messages_version="2023-06-01"
capability_profile="mock"
[capability_profiles.mock]
version="1"
provider="mock"
upstream_model="actual-model"
api="messages"
context_window=32000
max_output_tokens=1024
tested_codex_version="0.154.0-alpha.6"
[capability_profiles.mock.support]
instructions="native"
instruction_hierarchy="bridged_instruction_envelope"
function_tools="native"
custom_tools="bridged_custom_tool_json"
custom_grammar="bridged_codex_patch_grammar"
namespaced_tools="bridged_tool_namespace"
tool_choice="native"
parallel_tool_control="native"
max_output_tokens="native"
"#
    );
    if api == ApiProtocol::ChatCompletions {
        raw = raw
            .replace("api=\"messages\"", "api=\"chat_completions\"")
            .replace("auth=\"api_key\"", "auth=\"bearer\"")
            .replace("messages_version=\"2023-06-01\"\n", "")
            .replace(
                "instruction_hierarchy=\"bridged_instruction_envelope\"",
                "instruction_hierarchy=\"native\"",
            );
    }
    Config::parse(&raw).unwrap()
}
struct Harness {
    gateway: Server,
    _upstream: Server,
    mock: Mock,
    client: reqwest::Client,
}
impl Harness {
    async fn new(api: ApiProtocol, mode: Mode, configure: impl FnOnce(&mut Config)) -> Self {
        let mock = Mock {
            mode,
            api,
            calls: Arc::new(Mutex::new(Vec::new())),
            release: Arc::new(Notify::new()),
            dropped: Arc::new(AtomicBool::new(false)),
        };
        let upstream = serve(
            Router::new()
                .route(
                    if api == ApiProtocol::Messages {
                        "/v1/messages"
                    } else {
                        "/v1/chat/completions"
                    },
                    post(upstream),
                )
                .with_state(mock.clone()),
        )
        .await;
        let mut config = config(&upstream.url, api);
        configure(&mut config);
        let gateway = serve(
            router(
                config,
                Secrets {
                    local_token: TOKEN.into(),
                    upstream_keys: BTreeMap::from([("mock".into(), KEY.into())]),
                },
            )
            .unwrap(),
        )
        .await;
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
            .post(format!("{}/v1/responses", self.gateway.url))
            .bearer_auth(TOKEN)
    }
    fn calls(&self) -> usize {
        self.mock.calls.lock().unwrap().len()
    }
}
#[tokio::test]
async fn converted_http_maps_json_and_keeps_credentials_and_provider_headers_private() {
    for api in [ApiProtocol::Messages, ApiProtocol::ChatCompletions] {
        let h = Harness::new(api, Mode::Json(response_json(api)), |_| {}).await;
        let response = h
            .post()
            .json(&json!({"model":"writer","input":"synthetic","max_output_tokens":900}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-private-upstream").is_none());
        let output: Value = response.json().await.unwrap();
        assert_eq!(output["status"], "completed");
        assert_eq!(output["output"][0]["content"][0]["text"], "합성 🧪");
        let calls = h.mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        if api == ApiProtocol::Messages {
            assert_eq!(calls[0].0["x-api-key"], KEY);
            assert_eq!(calls[0].0["anthropic-version"], "2023-06-01");
            assert!(calls[0].0.get("authorization").is_none());
            assert_eq!(calls[0].1["max_tokens"], 900);
        } else {
            assert_eq!(calls[0].0["authorization"], format!("Bearer {KEY}"));
            assert!(calls[0].0.get("x-api-key").is_none());
            assert!(calls[0].0.get("anthropic-version").is_none());
            assert_eq!(calls[0].1["max_completion_tokens"], 900);
        }
        assert_eq!(calls[0].1["model"], "actual-model");
        assert!(calls[0].1.get("store").is_none_or(|v| v == false));
    }
}
#[tokio::test]
async fn unsupported_requests_fail_before_any_converted_upstream_call() {
    for api in [ApiProtocol::Messages, ApiProtocol::ChatCompletions] {
        let h = Harness::new(api, Mode::Json(response_json(api)), |_| {}).await;
        for extra in [
            json!({"unknown_required":true}),
            json!({"max_output_tokens":1025}),
            json!({"reasoning":{"effort":"high"}}),
            json!({"tools":[{"type":"function","name":"echo","strict":true}]}),
            json!({"tools":[{"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":"unknown"}}]}),
            json!({"text":{"verbosity":"high"}}),
            json!({"tools":[{"type":"function","name":"echo"}],"input":[{"type":"function_call","name":"echo","call_id":"c","status":"in_progress","arguments":"{}"},{"type":"function_call_output","call_id":"c","output":"result"}]}),
        ] {
            let mut body = json!({"model":"writer","input":"synthetic"});
            body.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert_eq!(
                h.post().json(&body).send().await.unwrap().status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(h.calls(), 0);
    }
}
#[tokio::test]
async fn converted_stream_is_incremental_and_completes_after_split_bytes() {
    for api in [ApiProtocol::Messages, ApiProtocol::ChatCompletions] {
        let h = Harness::new(api, Mode::Stream(text_frames(api)), |_| {}).await;
        let response = h
            .post()
            .json(&json!({"model":"writer","input":"synthetic","stream":true}))
            .send()
            .await
            .unwrap();
        let mut stream = response.bytes_stream();
        let first = timeout(Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            std::str::from_utf8(&first)
                .unwrap()
                .contains("response.created")
        );
        h.mock.release.notify_one();
        let mut output = first.to_vec();
        while let Some(chunk) = stream.next().await {
            output.extend_from_slice(&chunk.unwrap());
        }
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("합성 🧪"));
        assert!(text.contains("response.output_text.delta"));
        assert!(text.contains("response.completed"));
        assert_eq!(h.calls(), 1);
    }
}
#[tokio::test]
async fn converted_eof_error_and_aggregate_limit_close_without_completion_and_release_capacity() {
    for api in [ApiProtocol::Messages, ApiProtocol::ChatCompletions] {
        for events in [
            vec![],
            vec![json!({"type":"error","error":{"message":"synthetic"}})],
            {
                let mut events = vec![text_frames(api)[0].clone()];
                events.extend((0..8).map(|_| if api == ApiProtocol::Messages {json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x".repeat(1000)}})} else {chat_chunk(json!({"content":"x".repeat(1000)}), Value::Null)}));
                events
            },
        ] {
            let h = Harness::new(api, Mode::Stream(events), |c| {
                c.limits.max_response_bytes = 4096;
                c.limits.max_in_flight = 1;
            })
            .await;
            let response = h
                .post()
                .json(&json!({"model":"writer","input":"synthetic","stream":true}))
                .send()
                .await
                .unwrap();
            let mut stream = response.bytes_stream();
            let mut output = stream.next().await.unwrap().unwrap().to_vec();
            h.mock.release.notify_one();
            let mut failed = false;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => output.extend_from_slice(&bytes),
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            assert!(failed);
            assert!(!String::from_utf8_lossy(&output).contains("response.completed"));
            drop(stream);
            let next = h
                .post()
                .json(&json!({"model":"writer","input":"synthetic","stream":true}))
                .send()
                .await
                .unwrap();
            assert_eq!(next.status(), StatusCode::OK);
        }
    }
}
#[tokio::test]
async fn converted_heartbeat_disconnect_closes_upstream_and_releases_held_capacity() {
    for api in [ApiProtocol::Messages, ApiProtocol::ChatCompletions] {
        let h = Harness::new(api, Mode::Heartbeat, |c| c.limits.max_in_flight = 1).await;
        let response = h
            .post()
            .json(&json!({"model":"writer","input":"synthetic","stream":true}))
            .send()
            .await
            .unwrap();
        let mut stream = response.bytes_stream();
        stream.next().await.unwrap().unwrap();
        h.mock.release.notify_one();
        let overloaded = h
            .post()
            .json(&json!({"model":"writer","input":"synthetic","stream":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(overloaded.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(h.calls(), 1);
        drop(stream);
        timeout(Duration::from_secs(3), async {
            while !h.mock.dropped.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("dropping client must cancel the upstream producer");
        let next = h
            .post()
            .json(&json!({"model":"writer","input":"synthetic","stream":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(next.status(), StatusCode::OK);
    }
}
#[tokio::test]
async fn invalid_json_and_provider_errors_remain_sanitized() {
    for api in [ApiProtocol::Messages, ApiProtocol::ChatCompletions] {
        for (mode, expected) in [
            (
                Mode::Json("{\"type\":\"message\",\"type\":\"message\"}".into()),
                StatusCode::BAD_GATEWAY,
            ),
            (Mode::Error, StatusCode::TOO_MANY_REQUESTS),
        ] {
            let h = Harness::new(api, mode, |_| {}).await;
            let response = h
                .post()
                .json(&json!({"model":"writer","input":"synthetic"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            let text = response.text().await.unwrap();
            assert!(!text.contains(KEY));
            assert!(!text.contains("synthetic body"));
            assert_eq!(h.calls(), 1);
        }
    }
}
