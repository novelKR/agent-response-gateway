//! Common managed dispatch. Durable attempts outlive HTTP connections.
use crate::{
    adapters::{managed::ManagedAdapter, sse::SseDecoder},
    continuation::{self, ReplayV2, Session},
    error::ApiError,
    http::GatewayState,
    ir::continuity::VerifiedProviderHistory as ProviderHistory,
};
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::OwnedSemaphorePermit;

fn rejected() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "continuation_rejected",
        "Continuation cannot be verified; host reconciliation is required",
    )
}
fn upstream() -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "upstream_invalid_response",
        "Managed provider response could not be validated",
    )
}
// Normalize only Responses item metadata. Never edit user/tool content recursively.
fn normalized(v: &Value) -> Value {
    match v {
        Value::Array(items) => Value::Array(items.iter().map(normalized).collect()),
        Value::Object(item) => {
            let mut item = item.clone();
            item.remove("id");
            item.remove("status");
            for key in ["phase", "internal_chat_message_metadata_passthrough"] {
                if item.get(key).is_some_and(Value::is_null) {
                    item.remove(key);
                }
            }
            if !item.contains_key("type") && item.contains_key("role") {
                item.insert("type".into(), json!("message"));
            }
            if let Some(Value::Array(parts)) = item.get_mut("content") {
                for part in parts {
                    if part.get("type").and_then(Value::as_str) == Some("output_text")
                        && part
                            .get("annotations")
                            .and_then(Value::as_array)
                            .is_some_and(Vec::is_empty)
                    {
                        part.as_object_mut()
                            .expect("text object")
                            .remove("annotations");
                    }
                }
            }
            Value::Object(item)
        }
        _ => v.clone(),
    }
}
struct Attempt {
    runtime: continuation::Runtime,
    session: String,
    id: String,
    finalized: bool,
}
impl Attempt {
    fn mark_finalized(&mut self) {
        self.finalized = true;
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.finalized {
            let r = self.runtime.clone();
            let s = self.session.clone();
            let a = self.id.clone();
            tokio::spawn(async move {
                let _ = r.access(move |store, _| store.uncertain(&s, &a)).await;
            });
        }
    }
}

async fn history(
    runtime: &continuation::Runtime,
    session: &Session,
    payload: &mut Map<String, Value>,
) -> Result<ProviderHistory, ApiError> {
    let Some(input) = payload.get("input").and_then(Value::as_array) else {
        if session.head.is_some() {
            return Err(rejected());
        }
        return Ok(ProviderHistory::default());
    };
    let mut clean = Vec::new();
    let mut tokens = Vec::new();
    for item in input {
        if item.get("type").and_then(Value::as_str) == Some("reasoning") {
            if item.as_object().is_none_or(|m| {
                m.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "type"
                            | "id"
                            | "summary"
                            | "encrypted_content"
                            | "content"
                            | "internal_chat_message_metadata_passthrough"
                    )
                })
            }) {
                return Err(rejected());
            }
            if item
                .get("content")
                .is_some_and(|v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty))
                || item
                    .get("internal_chat_message_metadata_passthrough")
                    .is_some_and(|v| !v.is_null())
            {
                return Err(rejected());
            }
            let summary = item
                .get("summary")
                .and_then(Value::as_array)
                .ok_or_else(rejected)?;
            if summary.iter().any(|part| {
                part.as_object().is_none_or(|m| {
                    m.len() != 2 || part["type"] != "summary_text" || !part["text"].is_string()
                })
            }) {
                return Err(rejected());
            }
            if let Some(token) = item.get("encrypted_content").filter(|v| !v.is_null()) {
                tokens.push(token.as_str().ok_or_else(rejected)?.to_owned());
            } else if summary.is_empty() {
                return Err(rejected());
            }
            if !summary.is_empty() {
                let mut public = item.clone();
                let fields = public.as_object_mut().expect("reasoning object");
                fields.remove("encrypted_content");
                fields.remove("content");
                fields.remove("internal_chat_message_metadata_passthrough");
                clean.push(public);
            }
        } else {
            clean.push(item.clone());
        }
    }
    let mut result = ProviderHistory::default();
    let mut seen = std::collections::BTreeSet::new();
    let mut previous = None;
    let mut last_end = 0;
    for token in tokens {
        let replay = runtime
            .restore_record(session.clone(), token)
            .await
            .map_err(|_| rejected())?
            .normalize();
        if !seen.insert(replay.response.clone()) || replay.parent != previous {
            return Err(rejected());
        }
        let output: Vec<_> = replay.output.iter().map(normalized).collect();
        let start = replay.input_len;
        if start < last_end
            || start + output.len() > clean.len()
            || continuation::digest(&normalized(&json!(&clean[..start]))).map_err(|_| rejected())?
                != replay.input_sha256
            || !clean[start..start + output.len()]
                .iter()
                .map(normalized)
                .eq(output.iter().cloned())
        {
            return Err(rejected());
        }
        let end = start + output.len();
        if result
            .segments
            .insert(start, (end, replay.native))
            .is_some()
        {
            return Err(rejected());
        }
        last_end = end;
        previous = Some(replay.response);
    }
    if previous != session.head {
        return Err(rejected());
    }
    for (position, item) in clean.iter().enumerate() {
        if item.get("type").and_then(Value::as_str) == Some("reasoning")
            && !result
                .segments
                .iter()
                .any(|(start, (end, _))| *start <= position && position < *end)
        {
            return Err(rejected());
        }
    }
    if session.head.is_none()
        && let Some(expected) = &session.portable_sha256
    {
        // A host-approved epoch starts a fresh Codex thread. Codex may prepend its
        // current instruction/environment messages; the portable user message
        // must occur exactly once and cannot import executable or provider state.
        if clean.iter().any(|item| {
            !matches!(
                item.get("role").and_then(Value::as_str),
                Some("system" | "developer" | "user")
            ) || item.get("type").is_some_and(|v| v != "message")
        }) {
            return Err(rejected());
        }
        let matches = clean
            .iter()
            .filter(|item| {
                item.get("role").and_then(Value::as_str) == Some("user")
                    && continuation::digest(&normalized(&json!([item])))
                        .is_ok_and(|hash| &hash == expected)
            })
            .count();
        if matches != 1 {
            return Err(rejected());
        }
    }
    payload.insert("input".into(), json!(clean));
    Ok(result)
}

pub(crate) async fn responses(
    state: Arc<GatewayState>,
    mut payload: Map<String, Value>,
    model: String,
    streaming: bool,
    session_id: Option<String>,
    permit: OwnedSemaphorePermit,
) -> Result<Response, ApiError> {
    let runtime = state.continuation.clone().ok_or_else(rejected)?;
    let config = state.config.continuation.as_ref().ok_or_else(rejected)?;
    let id = session_id.ok_or_else(rejected)?;
    let lookup = id.clone();
    let session = runtime
        .access(move |s, _| s.session(&lookup))
        .await
        .map_err(|_| rejected())?;
    if !matches!(session.status.as_str(), "ready" | "compacting") {
        return Err(rejected());
    }
    let route = state.config.resolve_route(&model).map_err(|_| rejected())?;
    let manifest = state.config.manifest().map_err(|_| rejected())?;
    let expected = manifest.configuration()["routes"]
        .as_array()
        .and_then(|routes| routes.iter().find(|r| r["alias"] == model))
        .ok_or_else(rejected)?;
    let mut expected = expected.clone();
    expected
        .as_object_mut()
        .expect("manifest route")
        .remove("api_key_env");
    if session.origin.route != expected
        || session.origin.realm != config.realm
        || session.origin.generation != config.generation
    {
        return Err(rejected());
    }
    if let Some(Value::String(input)) = payload.get("input") {
        payload.insert("input".into(), json!([{"type":"message","role":"user","content":[{"type":"input_text","text":input}]}]));
    }
    let replay = history(&runtime, &session, &mut payload).await?;
    let input = payload
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(rejected)?;
    let input_len = input.len();
    let input_sha256 = continuation::digest(&normalized(&json!(input))).map_err(|_| rejected())?;
    let input_digest = continuation::digest(&payload).map_err(|_| rejected())?;
    let admitted = route.admit_verified(payload, &replay).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_request",
            "Request exceeds the declared managed capabilities",
        )
    })?;
    let crate::routing::AdmittedRequest::Translated { request, plan } = admitted else {
        return Err(rejected());
    };
    let prepared = ManagedAdapter::encode(&request, &plan, &replay).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_request",
            "Request cannot be represented by the pinned managed contract",
        )
    })?;
    if session.pending_tools {
        prepared
            .validate_pending_controls(&replay)
            .map_err(|_| rejected())?;
    }
    let s = session.clone();
    let reserve = state.config.limits.max_response_bytes as u64;
    let attempt_id = runtime
        .access(move |store, _| {
            store.begin(&s.id, s.revision, s.head.as_deref(), &input_digest, reserve)
        })
        .await
        .map_err(|_| rejected())?;
    let mut attempt = Attempt {
        runtime: runtime.clone(),
        session: id,
        id: attempt_id.clone(),
        finalized: false,
    };
    let (auth_name, auth_value) = match route.auth {
        crate::config::UpstreamAuth::GoogleApiKey => (
            "x-goog-api-key",
            state.secrets.upstream_keys[&route.snapshot.provider_id].clone(),
        ),
        crate::config::UpstreamAuth::ApiKey => (
            "x-api-key",
            state.secrets.upstream_keys[&route.snapshot.provider_id].clone(),
        ),
        crate::config::UpstreamAuth::Bearer => (
            "authorization",
            format!(
                "Bearer {}",
                state.secrets.upstream_keys[&route.snapshot.provider_id]
            ),
        ),
    };
    let mut request = state
        .client
        .post(route.endpoint)
        .header(auth_name, auth_value);
    if let Some(version) = route.messages_version {
        request = request.header("anthropic-version", version);
    }
    if let Some(beta) = plan
        .route
        .capabilities
        .reasoning_contract
        .as_ref()
        .and_then(|c| c.beta_header())
    {
        request = request.header("anthropic-beta", beta);
    }
    let request = request
        .header(header::ACCEPT_ENCODING, "identity")
        .header(
            header::ACCEPT,
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(prepared.payload())
        .send();
    let response = tokio::time::timeout(
        Duration::from_millis(state.config.limits.response_header_timeout_ms),
        request,
    )
    .await
    .map_err(|_| upstream())?
    .map_err(|_| upstream())?;
    if !response.status().is_success() {
        return Err(ApiError::new(
            if response.status().is_redirection() {
                StatusCode::BAD_GATEWAY
            } else {
                response.status()
            },
            "upstream_error",
            "Upstream rejected or failed the request",
        ));
    }
    if response
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|v| {
            v.to_str()
                .map_or(true, |s| !s.eq_ignore_ascii_case("identity"))
        })
    {
        return Err(upstream());
    }
    let expected_type = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        != Some(expected_type)
    {
        return Err(upstream());
    }
    let max = state.config.limits.max_response_bytes;
    let idle = Duration::from_millis(state.config.limits.stream_idle_timeout_ms);
    let mut source = response.bytes_stream();
    if streaming {
        let stream = async_stream::stream! {
            let _capacity = permit;
            // Keep the durable attempt guard alive with the downstream stream.
            let _hold = &attempt;
            let mut decoder = SseDecoder::new(max).expect("validated limit");
            let mut adapter = prepared.stream(max, attempt_id.clone());
            let mut complete = false;
            let mut written = 0usize;
            let mut sequence = 0u64;
            'read: while let Ok(Some(Ok(chunk))) = tokio::time::timeout(idle, source.next()).await {
                let mut remaining = chunk.as_ref();
                loop {
                    let event = match decoder.next_event(&mut remaining) {
                        Ok(Some(event)) => event,
                        Ok(None) => break,
                        Err(_) => break 'read,
                    };
                    if adapter.event(event).is_err() { break 'read; }
                    for event in adapter.take_progress() {
                        let bytes = numbered(event, &mut sequence);
                        written = written.saturating_add(bytes.len());
                        if written > max { break 'read; }
                        yield Ok::<_, std::io::Error>(bytes);
                    }
                    if adapter.is_complete() { complete = true; break 'read; }
                }
            }
            if !complete {
                yield Err(std::io::Error::other("Managed stream interrupted"));
            } else {
                match adapter.finish() {
                    Ok(mut decoded) => {
                        decoded.project_usage();
                        let record = ReplayV2 {
                            schema: continuation::REPLAY_V2.into(), session: session.id.clone(),
                            epoch: session.epoch, origin: session.origin.clone(), response: attempt_id.clone(),
                            parent: session.head.clone(), input_len, input_sha256: input_sha256.clone(),
                            outcome: decoded.outcome, native: decoded.native,
                            output: public_output(&decoded.response).expect("validated output"),
                        };
                        let checked_response = decoded.response.clone();
                        let saved_sequence = sequence;
                        let result = runtime.finalize_checked(record, move |token| {
                            check_final_size(checked_response.clone(), token, max)?;
                            let mut next = saved_sequence;
                            let remaining: usize = finalized_events(checked_response, token).into_iter()
                                .map(|event| numbered(event, &mut next).len()).sum();
                            if written.saturating_add(remaining) > max { return Err(continuation::Error("stream limit")); }
                            Ok(())
                        }).await;
                        match result {
                            Ok(token) => {
                                attempt.mark_finalized();
                                for event in finalized_events(decoded.response, &token) {
                                    yield Ok(numbered(event, &mut sequence));
                                }
                            }
                            Err(_) => yield Err(std::io::Error::other("Continuation finalization failed")),
                        }
                    }
                    _ => yield Err(std::io::Error::other("Managed output incomplete or invalid")),
                }
            }
        };
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header("x-accel-buffering", "no")
            .body(Body::from_stream(stream))
            .map_err(|_| upstream())
    } else {
        let _capacity = permit;
        let mut bytes = Vec::new();
        while let Some(chunk) = tokio::time::timeout(idle, source.next())
            .await
            .map_err(|_| upstream())?
        {
            let chunk = chunk.map_err(|_| upstream())?;
            if bytes.len() + chunk.len() > max {
                return Err(upstream());
            }
            bytes.extend_from_slice(&chunk);
        }
        let mut decoded = prepared
            .decode_bytes(&bytes, &attempt_id)
            .map_err(|_| upstream())?;
        decoded.project_usage();
        let record = ReplayV2 {
            schema: continuation::REPLAY_V2.into(),
            session: session.id,
            epoch: session.epoch,
            origin: session.origin,
            response: attempt_id,
            parent: session.head,
            input_len,
            input_sha256,
            outcome: decoded.outcome,
            native: decoded.native,
            output: public_output(&decoded.response).ok_or_else(upstream)?,
        };
        let checked_response = decoded.response.clone();
        let token = runtime
            .finalize_checked(record, move |token| {
                check_final_size(checked_response, token, max)
            })
            .await
            .map_err(|_| rejected())?;
        attempt.mark_finalized();
        add_envelope(&mut decoded.response, &token);
        Response::builder()
            .status(200)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(decoded.response.to_string()))
            .map_err(|_| upstream())
    }
}
fn public_output(response: &Value) -> Option<Vec<Value>> {
    Some(
        response["output"]
            .as_array()?
            .iter()
            .filter(|item| {
                !(item["type"] == "reasoning"
                    && item["summary"].as_array().is_some_and(Vec::is_empty))
            })
            .cloned()
            .collect(),
    )
}
fn add_envelope(response: &mut Value, token: &str) {
    if let Some(item) = response["output"]
        .as_array_mut()
        .expect("validated output")
        .iter_mut()
        .find(|item| item["type"] == "reasoning")
    {
        item["encrypted_content"] = json!(token);
        return;
    }
    let id = response["id"].as_str().unwrap_or("response").to_owned();
    response["output"].as_array_mut().expect("validated output").push(json!({"type":"reasoning","id":format!("rs_{id}"),"summary":[],"encrypted_content":token}));
}
fn finalized_events(mut response: Value, token: &str) -> Vec<Value> {
    add_envelope(&mut response, token);
    let mut events = Vec::new();
    for (i, item) in response["output"]
        .as_array()
        .expect("output")
        .iter()
        .enumerate()
    {
        events.push(json!({"type":"response.output_item.done","output_index":i,"item":item}));
    }
    events.push(json!({"type":"response.completed","response":response}));
    events
}
fn numbered(mut value: Value, sequence: &mut u64) -> axum::body::Bytes {
    value["sequence_number"] = json!(*sequence);
    *sequence += 1;

    format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap_or("error")
    )
    .into()
}

fn check_final_size(mut response: Value, token: &str, max: usize) -> continuation::Result<()> {
    add_envelope(&mut response, token);
    if response.to_string().len() > max {
        return Err(continuation::Error("response limit"));
    }
    Ok(())
}
