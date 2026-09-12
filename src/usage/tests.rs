use super::*;
use axum::{Router, response::IntoResponse, routing::post};
use serde_json::json;
use std::{collections::BTreeMap, sync::Mutex};
struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(app: Router) -> Server {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(l, app).await.unwrap();
    });
    Server { url, task }
}
async fn run(wire: &str, stream: bool, ack: bool) -> (u16, Vec<u8>, Vec<UsageEvent>, u64) {
    run_policy(wire, stream, ack, false).await
}
async fn run_policy(
    wire: &str,
    stream: bool,
    ack: bool,
    fail_final: bool,
) -> (u16, Vec<u8>, Vec<UsageEvent>, u64) {
    let bytes = wire.as_bytes().to_vec();
    let wire = bytes.clone();
    let calls = Arc::new(AtomicU64::new(0));
    let counted = calls.clone();
    let upstream = server(Router::new().route(
        "/v1/responses",
        post(move || {
            let wire = wire.clone();
            counted.fetch_add(1, Ordering::SeqCst);
            async move {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        if stream {
                            "text/event-stream"
                        } else {
                            "application/json"
                        },
                    )],
                    wire,
                )
                    .into_response()
            }
        }),
    ))
    .await;
    let config=crate::Config::parse(&format!("[providers.mock]\nbase_url=\"{}/v1\"\napi_key_env=\"UNUSED\"\n[models.writer]\nprovider=\"mock\"\nupstream_model=\"actual-model\"",upstream.url)).unwrap();
    let secrets = crate::Secrets {
        local_token: "L".repeat(40),
        upstream_keys: BTreeMap::from([("mock".into(), "U".repeat(40))]),
    };
    let (sender, mut receiver) = mpsc::channel::<Delivery>(256);
    let events = Arc::new(Mutex::new(vec![]));
    let observed = events.clone();
    let worker = tokio::spawn(async move {
        while let Some(d) = receiver.recv().await {
            let accepted = ack && !(fail_final && d.event.kind == EventKind::AttemptFinished);
            observed.lock().unwrap().push(d.event);
            let _ = d.ack.send(accepted);
        }
    });
    let sink = UsageSink {
        sender,
        mode: Mode::DurableLocal,
        timeout: Duration::from_millis(200),
        producer: "producer".into(),
        dropped: Arc::new(AtomicU64::new(0)),
    };
    let gateway =
        server(crate::router_with_usage(config, secrets, None, Some(sink)).unwrap()).await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", gateway.url))
        .bearer_auth("L".repeat(40))
        .json(&json!({"model":"writer","input":"synthetic","stream":stream}))
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let body = response.bytes().await.unwrap().to_vec();
    let result = events.lock().unwrap().clone();
    worker.abort();
    (status, body, result, calls.load(Ordering::SeqCst))
}
#[tokio::test]
async fn native_json_records_cache_counts_before_completion_without_rewriting() {
    let raw = r#"{ "status":"completed", "usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13,"input_tokens_details":{"cached_tokens":4,"cache_write_tokens":2}} }"#;
    let (status, body, e, calls) = run(raw, false, true).await;
    assert_eq!(status, 200);
    assert_eq!(body, raw.as_bytes());
    assert_eq!(calls, 1);
    assert_eq!(e[0].kind, EventKind::AttemptStarted);
    let last = e.last().unwrap();
    assert_eq!(last.kind, EventKind::AttemptFinished);
    assert_eq!(last.usage.value("cache_write_input_tokens"), Some(2));
    assert_eq!(last.finality, Finality::Final);
}
#[tokio::test]
async fn native_sse_invalid_observation_is_forwarded_and_marked_partial() {
    let raw = "data: not-json\n\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}}\n\n";
    let (status, body, e, _) = run(raw, true, true).await;
    assert_eq!(status, 200);
    assert_eq!(body, raw.as_bytes());
    let last = e.last().unwrap();
    assert!(last.observation_incomplete);
    assert_eq!(last.finality, Finality::Partial);
}
#[tokio::test]
async fn start_commit_failure_never_calls_provider() {
    let (status, _, _, calls) = run("{}", false, false).await;
    assert_eq!(status, 503);
    assert_eq!(calls, 0);
}
#[tokio::test]
async fn missing_usage_is_unobserved_not_zero() {
    let (_, _, e, _) = run("{\"status\":\"completed\"}", false, true).await;
    let last = e.last().unwrap();
    assert_eq!(last.finality, Finality::Unobserved);
    assert_eq!(last.usage.value("input_tokens"), None);
}

#[tokio::test]
async fn final_commit_failure_returns_error_without_replaying_provider() {
    let (status, _, events, calls) = run_policy(
        r#"{"status":"completed","usage":{"input_tokens":2,"output_tokens":1}}"#,
        false,
        true,
        true,
    )
    .await;
    assert_eq!(status, 503);
    assert_eq!(calls, 1);
    assert_eq!(events.last().unwrap().usage.value("input_tokens"), Some(2));
}

#[tokio::test]
async fn duplicate_usage_keys_are_not_adopted_by_native_accounting() {
    let raw =
        r#"{"status":"completed","usage":{"input_tokens":2,"input_tokens":999,"output_tokens":1}}"#;
    let (status, body, events, _) = run(raw, false, true).await;
    assert_eq!(status, 200);
    assert_eq!(body, raw.as_bytes());
    let e = events.last().unwrap();
    assert!(e.observation_incomplete);
    assert_eq!(e.usage.value("input_tokens"), None);
}
