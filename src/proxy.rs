use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Extension,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;

use crate::{
    adapters::sse::SseDecoder,
    codecs::dispatch::Dispatch,
    config::UpstreamAuth,
    error::ApiError,
    http::{GatewayState, RequestId},
    responses_policy::normalize_stateless,
    routing::AdmittedRequest,
    usage::{self, Attempt, EventKind, Finality, Outcome, Profile, UsageEvent},
};

struct StreamLease {
    _permit: OwnedSemaphorePermit,
    request_id: String,
    route_id: String,
    started: Instant,
    outcome: &'static str,
}

impl StreamLease {
    fn mark(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }
}

impl Drop for StreamLease {
    fn drop(&mut self) {
        tracing::info!(request_id = %self.request_id, route_id = %self.route_id,
            elapsed_ms = self.started.elapsed().as_millis() as u64, outcome = self.outcome,
            "upstream_body_closed");
    }
}

fn bad(code: &'static str, message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, code, message)
}

async fn request_bytes(body: Body, maximum: usize) -> Result<Vec<u8>, ApiError> {
    let mut stream = body.into_data_stream();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| bad("invalid_body", "Cannot read request body"))?;
        if chunk.len() > maximum.saturating_sub(data.len()) {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "Request body exceeds the configured limit",
            ));
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

pub(crate) async fn responses(
    State(state): State<Arc<GatewayState>>,
    Extension(id): Extension<RequestId>,
    request: Request,
) -> Result<Response, ApiError> {
    let started = Instant::now();
    let mut session_headers = request.headers().get_all("x-gateway-session").iter();
    let session_id = match (session_headers.next(), session_headers.next()) {
        (Some(v), None) => v.to_str().ok().map(str::to_owned),
        _ => None,
    };
    let permit = state.slots.clone().try_acquire_owned().map_err(|_| {
        ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "capacity_exceeded",
            "Gateway request capacity is exhausted",
        )
    })?;
    let media_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("application/json")
    {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Content-Type must be application/json",
        ));
    }
    let raw = tokio::time::timeout(
        Duration::from_millis(state.config.limits.request_body_timeout_ms),
        request_bytes(request.into_body(), state.config.limits.max_request_bytes),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "Request body timed out",
        )
    })??;
    let value = serde_json::from_slice(&raw)
        .map_err(|_| bad("invalid_json", "Request body is not valid JSON"))?;
    let (payload, model_id, streaming) =
        normalize_stateless(value).map_err(|e| bad(e.code, e.message))?;
    if !state.config.models.contains_key(&model_id) {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "model_not_found",
            "Model is not registered",
        ));
    }
    let route = state
        .config
        .resolve_route(&model_id)
        .expect("configuration was validated");
    if route.compatibility.is_some() || state.config.models[&model_id].api_codec.is_some() {
        crate::adapters::json::decode(&raw).map_err(|_| {
            bad(
                "invalid_json",
                "Checked requests require unique JSON object keys",
            )
        })?;
    }
    if route.managed {
        return crate::proxy_managed::responses(
            state, payload, model_id, streaming, session_id, permit, id.0,
        )
        .await;
    }
    let admitted = route.admit(payload).map_err(|_| {
        bad(
            "unsupported_request",
            "Request is invalid or exceeds the declared route capabilities or limits",
        )
    })?;
    let (payload, mut prepared) = match admitted {
        AdmittedRequest::Native(payload) => (Value::Object(payload), None),
        AdmittedRequest::Translated { request, plan } => {
            let mut prepared = Dispatch::prepare(
                state.config.models[&model_id]
                    .api_codec
                    .as_ref()
                    .and_then(|id| state.config.codecs.get(id)),
                &request,
                &plan,
                state.config.limits.max_response_bytes,
                state
                    .config
                    .resolved_usage_profile(&state.config.models[&model_id]),
            )
            .await
            .map_err(|_| {
                bad(
                    "unsupported_request",
                    "Request cannot be represented by the declared API profile",
                )
            })?;
            (prepared.take_payload(), Some(prepared))
        }
    };
    let key = &state.secrets.upstream_keys[&route.snapshot.provider_id];
    let send = state.client.post(route.endpoint);
    let send = match route.auth {
        UpstreamAuth::Bearer => send.bearer_auth(key),
        UpstreamAuth::ApiKey => send.header("x-api-key", key),
        UpstreamAuth::GoogleApiKey => send.header("x-goog-api-key", key),
    };
    let send = if let Some(version) = &route.messages_version {
        send.header("anthropic-version", version)
    } else {
        send
    };
    let profile = match route.snapshot.api {
        crate::ir::ApiProtocol::Responses => Profile::ResponsesV1,
        crate::ir::ApiProtocol::Messages => Profile::MessagesV1,
        crate::ir::ApiProtocol::ChatCompletions => Profile::ChatV1,
        crate::ir::ApiProtocol::GeminiInteractions => Profile::GeminiInteractionsV1,
    };
    let timestamp = usage::now();
    let mut attempt = Attempt::start(
        state.usage.as_ref(),
        UsageEvent {
            schema: usage::SCHEMA.into(),
            producer_id: String::new(),
            request_id: id.0.clone(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            event_id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            kind: EventKind::AttemptStarted,
            started_at_ms: timestamp,
            observed_at_ms: timestamp,
            provider: route.snapshot.provider_id.clone(),
            model_alias: model_id.clone(),
            upstream_model: route.snapshot.model.clone(),
            reported_model: None,
            provider_request_id: None,
            provider_response_id: None,
            profile,
            configuration_sha256: state.configuration_sha256.clone(),
            upstream: Outcome::InProgress,
            gateway: Outcome::InProgress,
            finality: Finality::Unobserved,
            observation_incomplete: false,
            usage: Default::default(),
        },
    )
    .await
    .map_err(|_| accounting_error())?;
    let send = send
        .header(
            header::ACCEPT,
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .header("x-request-id", &id.0)
        .header(header::ACCEPT_ENCODING, "identity")
        .json(&payload)
        .send();
    let upstream_result = tokio::time::timeout(
        Duration::from_millis(state.config.limits.response_header_timeout_ms),
        send,
    )
    .await
    .map_err(|_| {
        ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "Upstream response headers timed out",
        )
    })
    .and_then(|result| {
        result.map_err(|error| {
            if error.is_timeout() {
                ApiError::new(
                    StatusCode::GATEWAY_TIMEOUT,
                    "upstream_timeout",
                    "Upstream connection timed out",
                )
            } else {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "upstream_unavailable",
                    "Cannot connect to the configured upstream",
                )
            }
        })
    });
    let upstream = match upstream_result {
        Ok(upstream) => upstream,
        Err(error) => {
            if let Some(a) = &mut attempt {
                a.event.upstream = Outcome::TransportLost;
            }
            let _ = usage::finish(&mut attempt, Outcome::Failed).await;
            return Err(error);
        }
    };
    if let Some(a) = &mut attempt {
        a.event.gateway = Outcome::Failed;
    }
    let status = upstream.status();
    if let Some(a) = &mut attempt {
        a.event.provider_request_id = upstream
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .filter(|v| usage::safe_label(v))
            .map(str::to_owned);
    }
    if !status.is_success() {
        if let Some(a) = &mut attempt {
            a.event.upstream = Outcome::Failed;
        }
        let _ = usage::finish(&mut attempt, Outcome::Failed).await;
        // Provider errors can echo credentials/prompts; expose only status and a local error.
        return Err(ApiError::new(
            if status.is_redirection() {
                StatusCode::BAD_GATEWAY
            } else {
                status
            },
            "upstream_error",
            "The upstream rejected or failed the request",
        ));
    }
    if upstream
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|value| {
            !value
                .to_str()
                .is_ok_and(|v| v.trim().eq_ignore_ascii_case("identity"))
        })
    {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "upstream_content_encoding",
            "Upstream content encoding is not supported",
        ));
    }
    let media_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let expected = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    if !media_type.eq_ignore_ascii_case(expected) {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "upstream_content_type",
            "Upstream returned an unexpected content type",
        ));
    }
    let idle = Duration::from_millis(state.config.limits.stream_idle_timeout_ms);
    let mut source = upstream.bytes_stream();
    let mut lease = StreamLease {
        _permit: permit,
        request_id: id.0,
        route_id: model_id,
        started,
        outcome: "downstream_closed",
    };
    if streaming {
        if let Some(a) = &mut attempt {
            a.event.gateway = Outcome::InProgress;
        }
        // No producer task or application queue: downstream demand drives upstream polling.
        // Capturing the lease outside the generator retains capacity even before its first poll.
        let maximum = state.config.limits.max_response_bytes;
        let stream = async_stream::stream! {
            let _hold = &lease;
            let mut framing = SseDecoder::new(maximum).expect("positive configured byte limit");
            let mut observation = state.usage.as_ref().map(|_| SseDecoder::new(maximum).expect("positive limit"));
            let mut converted = match prepared.as_mut() {
                Some(p)=>match p.stream(maximum).await {Ok(s)=>Some(s),Err(_)=>{let _=usage::finish(&mut attempt,Outcome::ConversionFailed).await;yield Err(io::Error::other("Codec stream initialization failed"));return;}},
                None=>None,
            };
            'upstream: loop {
                match tokio::time::timeout(idle, source.next()).await {
                    Ok(Some(Ok(chunk))) => {
                        if let Some(converted) = &mut converted {
                            let mut remaining = chunk.as_ref();
                            loop {
                                let event = match framing.next_event(&mut remaining) {
                                    Ok(Some(event)) => event,
                                    Ok(None) => break,
                                    Err(_) => {
                                        lease.mark("upstream_invalid_stream");
                                        yield Err(io::Error::other("Upstream stream is invalid"));
                                        break 'upstream;
                                    }
                                };
                                if let Ok(payload) = crate::adapters::json::decode(event.data.as_bytes()) {
                                    if usage::observe_payload(&mut attempt, &payload).await.is_err() { lease.mark("usage_record_failed"); yield Err(io::Error::other("Usage record failed")); break 'upstream; }
                                } else if event.data.trim() != "[DONE]" && let Some(a) = &mut attempt { a.incomplete(); }
                                let events = match converted.event(event).await {
                                    Ok(events) => events,
                                    Err(_) => {
                                        lease.mark("upstream_invalid_stream");
                                        let _ = usage::finish(&mut attempt, Outcome::ConversionFailed).await;
                                        yield Err(io::Error::other("Upstream stream cannot be converted"));
                                        break 'upstream;
                                    }
                                };
                                // Checked Responses releases executable items only with a fully
                                // validated terminal; commit accounting before the entire batch.
                                if converted.gates_tool_completion()
                                    && let Some(terminal) = events.iter().find(|e|matches!(e["type"].as_str(),Some("response.completed"|"response.incomplete"|"response.failed"))) {
                                    if let Some(a) = &mut attempt { a.event.upstream = match terminal["type"].as_str() {Some("response.completed")=>Outcome::Completed,Some("response.incomplete")=>Outcome::Incomplete,_=>Outcome::Failed}; }
                                    if usage::finish(&mut attempt,Outcome::Completed).await.is_err() {lease.mark("usage_record_failed");yield Err(io::Error::other("Usage record failed"));break 'upstream;}
                                }
                                for event in events {
                                    let kind = event["type"].as_str().expect("constructed Responses event type");
                                    if matches!(kind, "response.completed" | "response.incomplete" | "response.failed") {
                                        if let Some(a) = &mut attempt { a.event.upstream = match kind { "response.completed"=>Outcome::Completed,"response.incomplete"=>Outcome::Incomplete,_=>Outcome::Failed }; }
                                        if usage::finish(&mut attempt, Outcome::Completed).await.is_err() { lease.mark("usage_record_failed"); yield Err(io::Error::other("Usage record failed")); break 'upstream; }
                                    }
                                    yield Ok::<Bytes, io::Error>(Bytes::from(format!("event: {kind}\ndata: {event}\n\n")));
                                }
                                if converted.is_complete() { lease.mark("converted_complete"); break 'upstream; }
                            }
                        } else {
                            if let Some(parser) = &mut observation {
                                let mut remaining = chunk.as_ref();
                                loop {
                                    match parser.next_event(&mut remaining) {
                                        Ok(Some(event)) => {
                                            if let Ok(payload) = crate::adapters::json::decode(event.data.as_bytes()) {
                                                if usage::observe_payload(&mut attempt, &payload).await.is_err() { lease.mark("usage_record_failed"); yield Err(io::Error::other("Usage record failed")); break 'upstream; }
                                                if attempt.as_ref().is_some_and(|a|matches!(a.event.upstream,Outcome::Completed|Outcome::Incomplete|Outcome::Failed))
                                                    && usage::finish(&mut attempt,Outcome::Completed).await.is_err() {lease.mark("usage_record_failed");yield Err(io::Error::other("Usage record failed"));break 'upstream;}
                                            } else if event.data.trim() != "[DONE]"&& let Some(a)=&mut attempt {a.incomplete();}
                                        }
                                        Ok(None) => break,
                                        Err(_) => {if let Some(a)=&mut attempt {a.incomplete();} observation=None; break;}
                                    }
                                }
                            }
                            yield Ok::<Bytes, io::Error>(chunk);
                        }
                    },
                    Ok(None) => {
                        if observation.as_ref().is_some_and(|p|p.finish().is_err())&& let Some(a)=&mut attempt {a.incomplete();}
                        if converted.as_ref().is_some_and(|c| c.finish().is_err()) || (converted.is_some() && framing.finish().is_err()) {
                            lease.mark("upstream_incomplete_stream");
                            yield Err(io::Error::other("Upstream stream ended before completion"));
                        } else { lease.mark("upstream_eof"); }
                        break;
                    },
                    Ok(Some(Err(_))) => { lease.mark("upstream_read_error"); yield Err(io::Error::other("Upstream stream interrupted")); break; },
                    Err(_) => { lease.mark("upstream_idle_timeout"); yield Err(io::Error::new(io::ErrorKind::TimedOut, "Upstream stream timed out")); break; },
                }
            }
            let outcome = if matches!(lease.outcome,"converted_complete"|"upstream_eof") {Outcome::Completed} else {Outcome::TransportLost};
            if usage::finish(&mut attempt,outcome).await.is_err() {yield Err(io::Error::other("Usage record failed"));}
        };
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        response
            .headers_mut()
            .insert("x-accel-buffering", HeaderValue::from_static("no"));
        Ok(response)
    } else {
        let mut data = Vec::new();
        loop {
            let chunk = tokio::time::timeout(idle, source.next())
                .await
                .map_err(|_| {
                    ApiError::new(
                        StatusCode::GATEWAY_TIMEOUT,
                        "upstream_timeout",
                        "Upstream response body timed out",
                    )
                })?;
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "upstream_read_error",
                    "Upstream response body was interrupted",
                )
            })?;
            if chunk.len()
                > state
                    .config
                    .limits
                    .max_response_bytes
                    .saturating_sub(data.len())
            {
                return Err(ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "upstream_response_too_large",
                    "Upstream JSON response exceeds the configured limit",
                ));
            }
            data.extend_from_slice(&chunk);
        }
        if let Ok(value) = crate::adapters::json::decode(&data) {
            usage::observe_payload(&mut attempt, &value)
                .await
                .map_err(|_| accounting_error())?;
            if let Some(a) = &mut attempt {
                a.event.upstream = match value
                    .get("status")
                    .and_then(Value::as_str)
                    .or_else(|| value.get("stop_reason").and_then(Value::as_str))
                    .or_else(|| {
                        value
                            .pointer("/choices/0/finish_reason")
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("completed")
                {
                    "incomplete" | "max_tokens" | "length" => Outcome::Incomplete,
                    "failed" => Outcome::Failed,
                    _ => Outcome::Completed,
                };
            }
        } else if let Some(a) = &mut attempt {
            a.incomplete();
        }
        if let Some(mut prepared) = prepared {
            let decoded = prepared.decode_bytes(&data).await;
            if decoded.is_err() {
                let _ = usage::finish(&mut attempt, Outcome::ConversionFailed).await;
            }
            let output = decoded.map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "upstream_invalid_response",
                    "Upstream response cannot be converted",
                )
            })?;
            data = serde_json::to_vec(&output).expect("constructed JSON response");
            if route.compatibility.is_some() && data.len() > state.config.limits.max_response_bytes
            {
                let _ = usage::finish(&mut attempt, Outcome::ConversionFailed).await;
                return Err(ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "upstream_response_too_large",
                    "Converted response exceeds the configured limit",
                ));
            }
        } else if serde_json::from_slice::<Value>(&data).is_err() {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "upstream_invalid_json",
                "Upstream returned invalid JSON",
            ));
        }
        usage::finish(&mut attempt, Outcome::Completed)
            .await
            .map_err(|_| accounting_error())?;
        lease.outcome = "json_complete";
        Ok((status, [(header::CONTENT_TYPE, "application/json")], data).into_response())
    }
}

pub(crate) fn accounting_error() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "usage_recorder_unavailable",
        "Usage recorder did not confirm local commit",
    )
}
