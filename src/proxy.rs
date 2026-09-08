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
    config::UpstreamAuth,
    error::ApiError,
    http::{GatewayState, RequestId},
    responses_policy::normalize_stateless,
    routing::AdmittedRequest,
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
    let admitted = route.admit(payload).map_err(|_| {
        bad(
            "unsupported_request",
            "Request is invalid or exceeds the declared route capabilities or limits",
        )
    })?;
    let payload = match admitted {
        AdmittedRequest::Native(payload) => payload,
        AdmittedRequest::Translated { .. } => {
            return Err(bad(
                "unsupported_api",
                "API adapter is not qualified for dispatch",
            ));
        }
    };
    let key = &state.secrets.upstream_keys[&route.snapshot.provider_id];
    let send = state.client.post(route.endpoint);
    let send = match route.auth {
        UpstreamAuth::Bearer => send.bearer_auth(key),
        UpstreamAuth::ApiKey => send.header("x-api-key", key),
    };
    let send = if let Some(version) = &route.messages_version {
        send.header("anthropic-version", version)
    } else {
        send
    };
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
    let upstream = tokio::time::timeout(
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
    })?
    .map_err(|error| {
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
    })?;
    let status = upstream.status();
    if !status.is_success() {
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
        // No producer task or application queue: downstream demand drives upstream polling.
        // Capturing the lease outside the generator retains capacity even before its first poll.
        let stream = async_stream::stream! {
            let _hold = &lease;
            loop {
                match tokio::time::timeout(idle, source.next()).await {
                    Ok(Some(Ok(chunk))) => yield Ok::<Bytes, io::Error>(chunk),
                    Ok(None) => { lease.mark("upstream_eof"); break; },
                    Ok(Some(Err(_))) => { lease.mark("upstream_read_error"); yield Err(io::Error::other("Upstream stream interrupted")); break; },
                    Err(_) => { lease.mark("upstream_idle_timeout"); yield Err(io::Error::new(io::ErrorKind::TimedOut, "Upstream stream timed out")); break; },
                }
            }
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
        if serde_json::from_slice::<Value>(&data).is_err() {
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "upstream_invalid_json",
                "Upstream returned invalid JSON",
            ));
        }
        lease.outcome = "json_complete";
        Ok((status, [(header::CONTENT_TYPE, "application/json")], data).into_response())
    }
}
