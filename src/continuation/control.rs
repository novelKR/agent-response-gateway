//! Host-only loopback control routes; never share this token with the model client.
use super::{Origin, Runtime};
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
#[derive(Clone)]
struct Control {
    runtime: Runtime,
    token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    origin: Origin,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    revision: i64,
    kind: String,
    portable_sha256: Option<String>,
    decision_reference: String,
    pending_tools: bool,
    pending_approvals: bool,
}
fn error() -> Response {
    (StatusCode::CONFLICT,Json(json!({"error":{"code":"continuation_rejected","message":"Host reconciliation or a compatible state is required","type":"gateway_error"}}))).into_response()
}
pub fn router(runtime: Runtime, token: String) -> Router {
    let state = Control { runtime, token };
    Router::new()
        .route("/__continuation/sessions", post(create))
        .route("/__continuation/sessions/{id}", get(status))
        .route(
            "/__continuation/sessions/{id}/transitions",
            post(transition),
        )
        .layer(axum::extract::DefaultBodyLimit::max(65536))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}
async fn authenticate(State(state): State<Control>, req: Request, next: Next) -> Response {
    let mut values = req.headers().get_all(header::AUTHORIZATION).iter();
    let ok = matches!((values.next(),values.next()),(Some(v),None) if v.to_str().ok().and_then(|v|v.strip_prefix("Bearer ")).is_some_and(|v|bool::from(v.as_bytes().ct_eq(state.token.as_bytes()))));
    if !ok {
        return (StatusCode::UNAUTHORIZED,Json(json!({"error":{"code":"unauthorized","message":"Host control token required","type":"gateway_error"}}))).into_response();
    }
    next.run(req).await
}
async fn create(State(state): State<Control>, Json(body): Json<Create>) -> Response {
    match state
        .runtime
        .access(move |s, _| s.create(&body.origin))
        .await
    {
        Ok(s) => Json(s).into_response(),
        Err(_) => error(),
    }
}
async fn status(State(state): State<Control>, Path(id): Path<String>) -> Response {
    match state.runtime.access(move |s, _| s.session(&id)).await {
        Ok(s) => Json(s).into_response(),
        Err(_) => error(),
    }
}
async fn transition(
    State(state): State<Control>,
    Path(id): Path<String>,
    Json(body): Json<Transition>,
) -> Response {
    if body.pending_tools
        || body.pending_approvals
        || body.portable_sha256.as_ref().is_some_and(|v| {
            v.len() != 64
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return error();
    }
    match state
        .runtime
        .access(move |s, _| {
            s.transition(
                &id,
                body.revision,
                &body.kind,
                body.portable_sha256.as_deref(),
                &body.decision_reference,
            )
        })
        .await
    {
        Ok(s) => Json(s).into_response(),
        Err(_) => error(),
    }
}
/// Fixed envelope without underlying database or cryptographic diagnostics.
pub fn api_error() -> (StatusCode, Json<Value>) {
    (
        StatusCode::CONFLICT,
        Json(
            json!({"error":{"code":"continuation_rejected","message":"Continuation could not be verified; host reconciliation is required","type":"gateway_error"}}),
        ),
    )
}
