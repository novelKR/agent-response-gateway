use crate::ModelAuthority;
use crate::{contract::*, ledger::Ledger, usage::UsageReader};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{Stream, StreamExt};
use gateway_management::{Error, Id, Result};
use gateway_team_access::{Principal, Purpose};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use zeroize::Zeroizing;
fn error(status: StatusCode, code: &'static str) -> Response {
    let mut response=(status,Json(json!({"schema":SCHEMA,"error":{"code":code,"message":"The team entry point could not authorize or confirm this request","type":"gateway_error"}}))).into_response();
    response
        .headers_mut()
        .insert("x-team-contract", HeaderValue::from_static(SCHEMA));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}
fn admitted_error(id: &Id, status: StatusCode, code: &'static str) -> Response {
    let mut response = error(status, code);
    response.headers_mut().insert(
        "x-team-request-id",
        HeaderValue::from_str(id.as_str()).expect("generated ID"),
    );
    response
}
fn control_session(value: &Value, expected: &gateway_management::Digest) -> Result<Id> {
    let fields = value.as_object().ok_or(Error::InvalidStore)?;
    if fields.len() != 8
        || ![
            "id",
            "epoch",
            "revision",
            "origin",
            "status",
            "head",
            "pending_tools",
            "portable_sha256",
        ]
        .iter()
        .all(|key| fields.contains_key(*key))
        || value["epoch"].as_i64().is_none_or(|v| v < 1)
        || value["revision"].as_i64().is_none_or(|v| v < 1)
        || !matches!(
            value["status"].as_str(),
            Some(
                "ready"
                    | "pending"
                    | "compacting"
                    | "compacting_pending"
                    | "awaiting_compaction"
                    | "unknown"
            )
        )
        || !value["pending_tools"].is_boolean()
    {
        return Err(Error::InvalidStore);
    }
    if !value["head"].is_null() {
        Id::new(value["head"].as_str().ok_or(Error::InvalidStore)?)?;
    }
    if !value["portable_sha256"].is_null() {
        gateway_management::Digest::try_from(
            value["portable_sha256"]
                .as_str()
                .ok_or(Error::InvalidStore)?
                .to_owned(),
        )?;
    }
    let origin: ManagedOrigin =
        serde_json::from_value(value["origin"].clone()).map_err(|_| Error::InvalidStore)?;
    if origin.digest()?.as_str() != expected.as_str() {
        return Err(Error::InvalidStore);
    }
    let id = uuid::Uuid::parse_str(value["id"].as_str().ok_or(Error::InvalidStore)?)
        .map_err(|_| Error::InvalidStore)?;
    Id::new(id.to_string())
}
struct Rejection(StatusCode, &'static str);
impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        error(self.0, self.1)
    }
}
fn management(error_value: Error) -> Response {
    match error_value {
        Error::Forbidden => error(StatusCode::FORBIDDEN, "team_forbidden"),
        Error::NotFound => error(StatusCode::NOT_FOUND, "team_not_found"),
        Error::InvalidInput => error(StatusCode::BAD_REQUEST, "invalid_request"),
        Error::Conflict => error(StatusCode::CONFLICT, "team_conflict"),
        Error::Unsupported => error(StatusCode::NOT_IMPLEMENTED, "team_unsupported"),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, "team_evidence_unavailable"),
    }
}
fn json_response(value: Value) -> Result<Response> {
    let bytes = serde_json::to_vec(&value).map_err(|_| Error::Storage)?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(Error::Conflict);
    }
    Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response())
}
/// An explicit router only; the host owns its listener, peer lifecycle and all credentials.
pub struct Service {
    target: Id,
    authority: String,
    auth: Arc<dyn ModelAuthority>,
    peers: Arc<dyn PeerSource>,
    ledger: Arc<Mutex<Ledger>>,
    usage: Mutex<Option<Box<dyn UsageReader>>>,
    client: reqwest::Client,
    limits: Limits,
    slots: Arc<Semaphore>,
    jobs: Arc<Semaphore>,
    live: Arc<Mutex<BTreeSet<Id>>>,
}
impl Service {
    pub fn new(
        bound: SocketAddr,
        auth: Arc<dyn ModelAuthority>,
        peers: Arc<dyn PeerSource>,
        ledger: Ledger,
        usage: Option<Box<dyn UsageReader>>,
        limits: Limits,
    ) -> Result<Arc<Self>> {
        limits.validate(bound)?;
        if auth.target() != ledger.target() {
            return Err(Error::InvalidInput);
        }
        let target = ledger.target().clone();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| Error::InvalidInput)?;
        Ok(Arc::new(Self {
            target,
            authority: bound.to_string(),
            auth,
            peers,
            ledger: Arc::new(Mutex::new(ledger)),
            usage: Mutex::new(usage),
            client,
            slots: Arc::new(Semaphore::new(limits.max_in_flight)),
            jobs: Arc::new(Semaphore::new(64)),
            limits,
            live: Arc::new(Mutex::new(BTreeSet::new())),
        }))
    }
    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/v1/models", get(models))
            .route("/v1/responses", post(responses))
            .route("/team/v1/usage", get(usage))
            .route("/team/v1/sessions", post(create_session))
            .route("/team/v1/sessions/{id}", get(session_status))
            .fallback(|| async { error(StatusCode::NOT_FOUND, "unsupported_endpoint") })
            .layer(middleware::from_fn_with_state(self.clone(), boundary))
            .with_state(self.clone())
    }
    fn token(&self, headers: &HeaderMap) -> Result<Zeroizing<String>> {
        let mut values = headers.get_all(header::AUTHORIZATION).iter();
        match (values.next(), values.next()) {
            (Some(value), None) => value
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("Bearer "))
                .filter(|s| s.len() <= 4096)
                .map(|s| Zeroizing::new(s.to_owned()))
                .ok_or(Error::Forbidden),
            _ => Err(Error::Forbidden),
        }
    }
    fn peer(&self) -> Result<Peer> {
        let peer = self.peers.current()?;
        if peer.identity.target != self.target {
            return Err(Error::InvalidStore);
        }
        Ok(peer)
    }
    fn principal(&self, token: &str) -> Result<Principal> {
        self.auth
            .authenticate_model(token)
            .filter(|p| {
                p.purpose == Purpose::Model
                    && p.permissions.enabled
                    && p.permissions.validate().is_ok()
            })
            .ok_or(Error::Forbidden)
    }
    async fn authenticate(
        self: &Arc<Self>,
        headers: &HeaderMap,
    ) -> std::result::Result<Principal, Rejection> {
        let token = self
            .token(headers)
            .map_err(|_| Rejection(StatusCode::UNAUTHORIZED, "team_unauthorized"))?;
        self.blocking(move |s| s.principal(&token))
            .await
            .map_err(|_| Rejection(StatusCode::UNAUTHORIZED, "team_unauthorized"))
    }
    fn refresh(&self, principal: &Principal) -> Result<Principal> {
        self.auth
            .refresh_model(&principal.identity, &principal.authorization_version)
            .filter(|p| {
                p.purpose == Purpose::Model
                    && p.permissions.enabled
                    && p.identity == principal.identity
                    && p.authorization_version == principal.authorization_version
                    && p.permissions.validate().is_ok()
            })
            .ok_or(Error::Forbidden)
    }
    async fn blocking<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnOnce(&Self) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let job = self
            .jobs
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict)?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let _job = job;
            work(&this)
        })
        .await
        .map_err(|_| Error::Storage)?
    }
    fn permit(&self) -> std::result::Result<OwnedSemaphorePermit, Rejection> {
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Rejection(StatusCode::TOO_MANY_REQUESTS, "team_capacity_exceeded"))
    }
}
async fn boundary(State(service): State<Arc<Service>>, request: Request, next: Next) -> Response {
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    if !matches!((hosts.next(),hosts.next()),(Some(value),None) if value.to_str().ok()==Some(&service.authority))
    {
        return error(StatusCode::BAD_REQUEST, "host_rejected");
    }
    if origins.next().is_some_and(|o| {
        o.to_str().ok() != Some(format!("http://{}", service.authority).as_str())
            || origins.next().is_some()
    }) {
        return error(StatusCode::FORBIDDEN, "origin_rejected");
    }
    if request
        .headers()
        .keys()
        .any(|h| h.as_str().starts_with("x-gateway-"))
    {
        return error(StatusCode::BAD_REQUEST, "internal_header_forbidden");
    }
    let mut response = next.run(request).await;
    // Axum path/query/media rejections must not reflect arbitrary user input.
    if (response.status().is_client_error() || response.status().is_server_error())
        && !response.headers().contains_key("x-team-contract")
        && !response.headers().contains_key("x-team-request-id")
    {
        response = error(response.status(), "invalid_request");
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}
async fn bytes(body: Body, maximum: usize, timeout: Duration) -> Result<Vec<u8>> {
    tokio::time::timeout(timeout, async move {
        let mut source = body.into_data_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = source.next().await {
            let chunk = chunk.map_err(|_| Error::InvalidInput)?;
            if chunk.len() > maximum.saturating_sub(bytes.len()) {
                return Err(Error::InvalidInput);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| Error::InvalidInput)?
}
async fn bounded(
    response: reqwest::Response,
    maximum: usize,
    timeout: Duration,
) -> Result<Vec<u8>> {
    tokio::time::timeout(timeout, async move {
        let mut body = response.bytes_stream();
        let mut result = Vec::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|_| Error::Storage)?;
            if chunk.len() > maximum.saturating_sub(result.len()) {
                return Err(Error::Conflict);
            }
            result.extend_from_slice(&chunk);
        }
        Ok(result)
    })
    .await
    .map_err(|_| Error::Storage)?
}
fn media(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            h.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
}
async fn models(State(service): State<Arc<Service>>, request: Request) -> Response {
    let _permit = match service.permit() {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if request.uri().query().is_some() {
        return management(Error::InvalidInput);
    }
    let principal = match service.authenticate(request.headers()).await {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let result = async {
        let (peer, principal) = service
            .blocking(move |s| Ok((s.peer()?, s.refresh(&principal)?)))
            .await?;
        let response = tokio::time::timeout(
            service.limits.header_timeout,
            service
                .client
                .get(peer.url("/v1/models")?)
                .bearer_auth(peer.local_token.as_str())
                .header(header::ACCEPT_ENCODING, "identity")
                .send(),
        )
        .await
        .map_err(|_| Error::Storage)?
        .map_err(|_| Error::Storage)?;
        if !response.status().is_success() {
            return Err(Error::Storage);
        }
        let data: Value =
            serde_json::from_slice(&bounded(response, 65536, service.limits.body_timeout).await?)
                .map_err(|_| Error::InvalidStore)?;
        if data["object"] != "list" {
            return Err(Error::InvalidStore);
        }
        let rows = data["data"].as_array().ok_or(Error::InvalidStore)?;
        if rows.len() > 1024 {
            return Err(Error::InvalidStore);
        }
        let mut filtered = Vec::new();
        for row in rows {
            let alias = row["id"].as_str().ok_or(Error::InvalidStore)?;
            if peer.routes.contains_key(alias) && principal.permissions.permits_route(alias) {
                filtered.push(row.clone());
            }
        }
        json_response(json!({"object":"list","data":filtered}))
    }
    .await;
    result.unwrap_or_else(management)
}
struct Guard {
    ledger: Arc<Mutex<Ledger>>,
    live: Arc<Mutex<BTreeSet<Id>>>,
    id: Id,
    ended: bool,
    _permit: OwnedSemaphorePermit,
}
impl Guard {
    fn finish(&mut self, end: End) {
        if self.ended {
            return;
        }
        self.ended = true;
        if let Ok(mut live) = self.live.lock() {
            live.remove(&self.id);
        }
        if let Ok(mut ledger) = self.ledger.lock() {
            let _ = ledger.finish(&self.id, end);
        }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.finish(End::ClientDisconnected);
    }
}
type GatewayBody = Pin<Box<dyn Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send>>;
struct Forward {
    body: Option<GatewayBody>,
    guard: Guard,
    idle: Pin<Box<tokio::time::Sleep>>,
    timeout: Duration,
    remaining: usize,
}
impl Forward {
    fn stop(&mut self, end: End) {
        self.body.take();
        self.guard.finish(end);
    }
}
impl Drop for Forward {
    fn drop(&mut self) {
        self.stop(End::ClientDisconnected);
    }
}
impl Stream for Forward {
    type Item = std::result::Result<Bytes, std::io::Error>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.body.is_none() {
            return Poll::Ready(None);
        }
        if std::future::Future::poll(this.idle.as_mut(), cx).is_ready() {
            this.stop(End::IdleTimeout);
            return Poll::Ready(Some(Err(std::io::Error::other(
                "team_gateway_body_unconfirmed",
            ))));
        }
        match this
            .body
            .as_mut()
            .expect("checked body")
            .as_mut()
            .poll_next(cx)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                this.stop(End::Eof);
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(_))) => {
                this.stop(End::GatewayBodyLost);
                Poll::Ready(Some(Err(std::io::Error::other(
                    "team_gateway_body_unconfirmed",
                ))))
            }
            Poll::Ready(Some(Ok(bytes))) => {
                if bytes.len() > this.remaining {
                    this.stop(End::ResponseLimit);
                    return Poll::Ready(Some(Err(std::io::Error::other("team_response_limit"))));
                }
                this.remaining -= bytes.len();
                this.idle
                    .as_mut()
                    .reset(tokio::time::Instant::now() + this.timeout);
                Poll::Ready(Some(Ok(bytes)))
            }
        }
    }
}
async fn responses(State(service): State<Arc<Service>>, request: Request) -> Response {
    let permit = match service.permit() {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if request.uri().query().is_some() || !media(request.headers()) {
        return management(Error::InvalidInput);
    }
    let principal = match service.authenticate(request.headers()).await {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let mut sessions = request.headers().get_all("x-team-session").iter();
    let session = match (sessions.next(), sessions.next()) {
        (None, None) => None,
        (Some(v), None) => match v.to_str().ok().and_then(|s| Id::new(s).ok()) {
            Some(v) => Some(v),
            None => return management(Error::InvalidInput),
        },
        _ => return management(Error::InvalidInput),
    };
    let raw = match bytes(
        request.into_body(),
        service.limits.max_request_bytes,
        service.limits.body_timeout,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return management(e),
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return management(Error::InvalidInput),
    };
    let route = match value
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256)
    {
        Some(route) => route.to_owned(),
        None => return management(Error::InvalidInput),
    };
    let admission = service
        .blocking(move |s| {
            let mut ledger = s.ledger.lock().map_err(|_| Error::Storage)?;
            let peer = s.peer()?;
            let principal = s.refresh(&principal)?;
            if !principal.permissions.permits_route(&route) {
                return Err(Error::Forbidden);
            }
            let registered = peer.routes.get(&route).ok_or(Error::Forbidden)?;
            let internal = match (&registered.managed, &session) {
                (None, None) => None,
                (Some(origin), Some(id)) => {
                    let (intent, binding) = ledger.session(&principal, id)?;
                    if intent.route != route || intent.origin_sha256 != origin.digest()? {
                        return Err(Error::Conflict);
                    }
                    Some(binding.ok_or(Error::Conflict)?.internal)
                }
                _ => return Err(Error::Conflict),
            };
            let admission = ledger.admit(&principal, &peer, route, session)?;
            s.live
                .lock()
                .map_err(|_| Error::Storage)?
                .insert(admission.id.clone());
            let guard = Guard {
                ledger: s.ledger.clone(),
                live: s.live.clone(),
                id: admission.id.clone(),
                ended: false,
                _permit: permit,
            };
            Ok((peer, admission, internal, guard))
        })
        .await;
    let (peer, admission, internal, mut guard) = match admission {
        Ok(v) => v,
        Err(e) => return management(e),
    };
    let url = match peer.url("/v1/responses") {
        Ok(v) => v,
        Err(e) => return management(e),
    };
    let mut send = service
        .client
        .post(url)
        .bearer_auth(peer.local_token.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT_ENCODING, "identity")
        .body(raw);
    if let Some(session) = internal {
        send = send.header("x-gateway-session", session.as_str());
    }
    let received = tokio::time::timeout(service.limits.header_timeout, send.send()).await;
    let response = match received {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => {
            guard.finish(End::GatewayConnectionFailed);
            return admitted_error(
                &admission.id,
                StatusCode::BAD_GATEWAY,
                "gateway_connection_unconfirmed",
            );
        }
        Err(_) => {
            guard.finish(End::HeaderTimeout);
            return admitted_error(
                &admission.id,
                StatusCode::GATEWAY_TIMEOUT,
                "gateway_headers_unconfirmed",
            );
        }
    };
    let mut ids = response.headers().get_all("x-request-id").iter();
    let gateway_request = match (ids.next(), ids.next()) {
        (Some(v), None) => v
            .to_str()
            .ok()
            .and_then(|v| uuid::Uuid::parse_str(v).ok())
            .map(|v| Id::new(v.to_string()).expect("UUID")),
        _ => None,
    };
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
    // Missing clock or correlation evidence must not replay or discard the model response.
    if let (Ok(at_ms), Ok(mut ledger)) = (now(), service.ledger.lock()) {
        let _ = ledger.headers(
            &admission,
            &Headers {
                at_ms,
                status: status.as_u16(),
                gateway_request: gateway_request.clone(),
            },
        );
    }
    let forward = Forward {
        body: Some(Box::pin(response.bytes_stream())),
        guard,
        idle: Box::pin(tokio::time::sleep(service.limits.idle_timeout)),
        timeout: service.limits.idle_timeout,
        remaining: service.limits.max_response_bytes,
    };
    let mut response = Response::new(Body::from_stream(forward));
    *response.status_mut() = status;
    response.headers_mut().insert(
        "x-team-request-id",
        HeaderValue::from_str(admission.id.as_str()).expect("UUID"),
    );
    if let Some(id) = gateway_request {
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(id.as_str()).expect("UUID"),
        );
    }
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}
async fn usage(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    let _permit = match service.permit() {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let principal = match service.authenticate(&headers).await {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let result=service.blocking(move|s|{
  let principal=s.refresh(&principal)?;let records=s.ledger.lock().map_err(|_|Error::Storage)?.records(&principal,&query)?;let mut reader=s.usage.lock().map_err(|_|Error::Storage)?;let mut rows=Vec::new();let mut response_bytes=0usize;let mut recorder_failed=false;
  for record in records{
   let active=s.live.lock().map_err(|_|Error::Storage)?.contains(&record.admission.id);
   let usage=match(&record.admission.producer,record.headers.as_ref().and_then(|h|h.gateway_request.as_ref()),if recorder_failed {None} else {reader.as_mut()}){
    (Some(producer),Some(request),Some(reader))=>match reader.lookup(producer,request){
     Ok(events) if !events.is_empty()&&events.len()<=16&&events.iter().all(|e|e.validate()&&e.producer_id==producer.as_str()&&e.request_id==request.as_str()&&e.model_alias==record.admission.route&&e.configuration_sha256==record.admission.configuration_sha256.as_str())=>{
      let identities:BTreeSet<_>=events.iter().map(|e|(&e.producer_id,&e.attempt_id)).collect();if identities.len()!=events.len(){json!({"state":"unobserved","reason":"correlation_mismatch"})}else{json!({"state":"observed","attempts":events})}
     },
     Ok(events) if events.is_empty()=>json!({"state":"unobserved","reason":"recorder_has_no_observation"}),
     Ok(_)=>json!({"state":"unobserved","reason":"correlation_mismatch"}),
     Err(_)=>{recorder_failed=true;json!({"state":"unobserved","reason":"recorder_unavailable"})},
    },
    (None,_,_)=>json!({"state":"unattributed","reason":"producer_not_observed"}),
    (_,None,_)=>json!({"state":"unattributed","reason":"gateway_request_not_observed"}),
    (_,_,None)=>json!({"state":"unobserved","reason":if recorder_failed {"recorder_unavailable"}else{"recorder_not_connected"}}),
   };
   let row=json!({"record":record,"transport_observation":if record.finished.is_some(){"recorded"}else if active{"active"}else{"unconfirmed"},"usage":usage});
   response_bytes += serde_json::to_vec(&row).map_err(|_|Error::Storage)?.len();if response_bytes>2*1024*1024-4096{return Err(Error::Conflict);}rows.push(row);
  }
  json_response(json!({"schema":SCHEMA,"observed_at_ms":now()?,"window":"team_admitted_at","scope":if query.all{"all_team_subjects"}else{"own_subject"},"from_ms":query.from_ms,"to_ms":query.to_ms,"requests":rows}))
 }).await;
    result.unwrap_or_else(management)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    model: String,
    idempotency_key: Id,
}
async fn create_session(State(service): State<Arc<Service>>, request: Request) -> Response {
    let _permit = match service.permit() {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if !media(request.headers()) || request.uri().query().is_some() {
        return management(Error::InvalidInput);
    }
    let principal = match service.authenticate(request.headers()).await {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let raw = match bytes(request.into_body(), 65536, service.limits.body_timeout).await {
        Ok(v) => v,
        Err(e) => return management(e),
    };
    let body: Create = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(_) => return management(Error::InvalidInput),
    };
    let prepared = service
        .blocking(move |s| {
            let mut ledger = s.ledger.lock().map_err(|_| Error::Storage)?;
            let peer = s.peer()?;
            let principal = s.refresh(&principal)?;
            if !principal.permissions.permits_route(&body.model) {
                return Err(Error::Forbidden);
            }
            let origin = peer
                .routes
                .get(&body.model)
                .and_then(|r| r.managed.clone())
                .ok_or(Error::Unsupported)?;
            let (intent, fresh) = ledger.session_intent(
                &principal,
                &body.model,
                &origin.digest()?,
                body.idempotency_key,
            )?;
            Ok((peer, origin, intent, fresh, principal))
        })
        .await;
    let (peer, origin, intent, fresh, principal) = match prepared {
        Ok(v) => v,
        Err(e) => return management(e),
    };
    if !fresh {
        return service
            .ledger
            .lock()
            .map_err(|_| Error::Storage)
            .and_then(|l| l.session_view(&principal, &intent.id))
            .and_then(json_response)
            .unwrap_or_else(management);
    }
    let result = async {
        let response = tokio::time::timeout(
            service.limits.header_timeout,
            service
                .client
                .post(peer.url("/__continuation/sessions")?)
                .bearer_auth(
                    peer.control_token
                        .as_ref()
                        .ok_or(Error::Unsupported)?
                        .as_str(),
                )
                .json(&json!({"origin":origin}))
                .send(),
        )
        .await
        .map_err(|_| Error::Storage)?
        .map_err(|_| Error::Storage)?;
        if !response.status().is_success() {
            return Err(Error::Conflict);
        }
        let value: Value =
            serde_json::from_slice(&bounded(response, 65536, service.limits.body_timeout).await?)
                .map_err(|_| Error::InvalidStore)?;
        let internal = control_session(&value, &intent.origin_sha256)?;
        let mut ledger = service.ledger.lock().map_err(|_| Error::Storage)?;
        ledger.bind_session(&intent, internal)?;
        json_response(ledger.session_view(&principal, &intent.id)?)
    }
    .await;
    match result{Ok(response)=>response,Err(_)=>(StatusCode::ACCEPTED,Json(json!({"schema":SCHEMA,"session":intent.id,"model":intent.route,"state":"unconfirmed"}))).into_response()}
}
async fn session_status(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    Path(id): Path<Id>,
) -> Response {
    let _permit = match service.permit() {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let principal = match service.authenticate(&headers).await {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };
    let prepared = service
        .blocking(move |s| {
            let peer = s.peer()?;
            let principal = s.refresh(&principal)?;
            let (intent, binding) = s
                .ledger
                .lock()
                .map_err(|_| Error::Storage)?
                .session(&principal, &id)?;
            let origin = peer
                .routes
                .get(&intent.route)
                .and_then(|r| r.managed.as_ref())
                .ok_or(Error::Unsupported)?;
            if origin.digest() != Ok(intent.origin_sha256.clone()) {
                return Err(Error::Conflict);
            }
            Ok((peer, intent, binding))
        })
        .await;
    let (peer, intent, binding) = match prepared {
        Ok(v) => v,
        Err(e) => return management(e),
    };
    let Some(binding) = binding else {
        return Json(
            json!({"schema":SCHEMA,"session":intent.id,"model":intent.route,"state":"unconfirmed"}),
        )
        .into_response();
    };
    let result=async{
  let response=tokio::time::timeout(service.limits.header_timeout,service.client.get(peer.url(&format!("/__continuation/sessions/{}",binding.internal))?).bearer_auth(peer.control_token.as_ref().ok_or(Error::Unsupported)?.as_str()).send()).await.map_err(|_|Error::Storage)?.map_err(|_|Error::Storage)?;
  if !response.status().is_success(){return Err(Error::Conflict);}let value:Value=serde_json::from_slice(&bounded(response,65536,service.limits.body_timeout).await?).map_err(|_|Error::InvalidStore)?;
  if control_session(&value,&intent.origin_sha256)?!=binding.internal{return Err(Error::InvalidStore);}
  json_response(json!({"schema":SCHEMA,"observed_at_ms":now()?,"session":intent.id,"model":intent.route,"state":"observed","status":value["status"],"revision":value["revision"],"epoch":value["epoch"],"has_head":!value["head"].is_null(),"pending_tools":value["pending_tools"]}))
 }.await;
    result.unwrap_or_else(management)
}
