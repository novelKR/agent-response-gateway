use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

use crate::{Config, ConfigError, Secrets, error::ApiError, extensions::ObserverSink, proxy};

pub(crate) struct GatewayState {
    pub config: Config,
    pub secrets: Secrets,
    pub client: reqwest::Client,
    pub slots: Arc<Semaphore>,
    pub continuation: Option<crate::continuation::Runtime>,
    pub usage: Option<crate::usage::UsageSink>,
    pub configuration_sha256: String,
}

#[derive(Clone)]
pub(crate) struct RequestId(pub String);

pub fn router(config: Config, secrets: Secrets) -> Result<Router, ConfigError> {
    router_with_observers(config, secrets, None)
}

/// Optional numeric metadata delivery; observers cannot change admission or transport.
pub fn router_with_observers(
    config: Config,
    secrets: Secrets,
    observers: Option<ObserverSink>,
) -> Result<Router, ConfigError> {
    router_with_usage(config, secrets, observers, None)
}

/// Opt-in accounting; durable mode may gate upstream admission and final completion.
pub fn router_with_usage(
    config: Config,
    secrets: Secrets,
    observers: Option<ObserverSink>,
    usage: Option<crate::usage::UsageSink>,
) -> Result<Router, ConfigError> {
    config.validate()?;
    secrets.validate(&config)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .connect_timeout(Duration::from_millis(config.limits.connect_timeout_ms))
        .pool_max_idle_per_host(config.limits.max_in_flight)
        .build()
        .map_err(|_| ConfigError("Cannot construct upstream HTTP client".into()))?;
    let continuation = config
        .continuation
        .as_ref()
        .map(|c| c.start(&secrets))
        .transpose()?;
    let control = continuation
        .as_ref()
        .map(|(r, t)| crate::continuation::control::router(r.clone(), t.clone()));
    let state = Arc::new(GatewayState {
        continuation: continuation.map(|(r, _)| r),
        slots: Arc::new(Semaphore::new(config.limits.max_in_flight)),
        configuration_sha256: config.manifest()?.configuration_sha256().into(),
        config,
        secrets,
        client,
        usage,
    });
    let protected = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/responses", post(proxy::responses))
        .route("/v1/responses/compact", post(unsupported))
        .route("/v1/responses/{id}", get(unsupported).delete(unsupported))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate));
    Ok(Router::new()
        .route("/", get(about))
        .route("/healthz", get(|| async { Json(json!({"status": "ok"})) }))
        .route(
            "/readyz",
            get(|| async { Json(json!({"status": "ready", "provider_probe": false})) }),
        )
        .merge(protected)
        .fallback(|| async {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "Endpoint is not supported",
            )
        })
        .with_state(state)
        .merge(control.unwrap_or_default())
        .layer(middleware::from_fn_with_state(observers, audit)))
}

async fn authenticate(
    State(state): State<Arc<GatewayState>>,
    request: Request,
    next: Next,
) -> Response {
    let mut values = request.headers().get_all(header::AUTHORIZATION).iter();
    let authenticated = match (values.next(), values.next()) {
        (Some(value), None) => value
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| {
                bool::from(value.as_bytes().ct_eq(state.secrets.local_token.as_bytes()))
            }),
        _ => false,
    };
    if !authenticated {
        let mut response = ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "A valid local bearer token is required",
        )
        .into_response();
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }
    next.run(request).await
}

async fn audit(
    State(observers): State<Option<ObserverSink>>,
    mut request: Request,
    next: Next,
) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let started = Instant::now();
    // Do not log URI, headers, body, errors, or user-supplied request IDs.
    request.extensions_mut().insert(RequestId(id.clone()));
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&id).expect("UUID is a valid header"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let headers_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    tracing::info!(request_id = %id, status = response.status().as_u16(), headers_ms, "response_headers");
    if let Some(observers) = observers {
        // Header timing is not proof of successful inference or completed streaming.
        observers.observe_headers(response.status().as_u16(), headers_ms);
    }
    response
}

async fn models(State(state): State<Arc<GatewayState>>) -> Json<serde_json::Value> {
    let data: Vec<_> = state
        .config
        .models
        .keys()
        .map(|id| json!({"id": id, "object": "model", "created": 0, "owned_by": "gateway"}))
        .collect();
    Json(json!({"object": "list", "data": data}))
}

async fn about(
    State(state): State<Arc<GatewayState>>,
    Extension(_id): Extension<RequestId>,
) -> Json<serde_json::Value> {
    Json(
        json!({"name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"), "license": "AGPL-3.0-only",
        "source_url": state.config.source_url, "source_status": if state.config.source_url.is_some() { "configured" } else { "not_configured" }}),
    )
}

async fn unsupported() -> ApiError {
    ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        "unsupported_endpoint",
        "Response storage, retrieval and compaction are not supported",
    )
}
