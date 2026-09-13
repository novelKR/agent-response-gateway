use crate::{
    Authenticator, Command, CredentialKind, Dispatcher, Preflight, Principal, Query, SCHEMA,
    Submission, UsageRange,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query as HttpQuery, Request as HttpRequest, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use gateway_management::{
    Action, Backend, Digest, Effect, Id, Identity, Journal, Operation, PreparedOperation, Reader,
    Request, Snapshot,
};
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, oneshot};

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    operation: Option<Id>,
}
type Result<T> = std::result::Result<T, ApiError>;
fn error(status: StatusCode, code: &'static str) -> ApiError {
    ApiError {
        status,
        code,
        operation: None,
    }
}
fn unavailable() -> ApiError {
    error(StatusCode::SERVICE_UNAVAILABLE, "management_unavailable")
}
fn invalid() -> ApiError {
    error(StatusCode::BAD_REQUEST, "invalid_request")
}
fn unauthorized() -> ApiError {
    error(StatusCode::UNAUTHORIZED, "unauthorized")
}
impl From<gateway_management::Error> for ApiError {
    fn from(value: gateway_management::Error) -> Self {
        use gateway_management::Error as E;
        match value {
            E::Forbidden => error(StatusCode::FORBIDDEN, "forbidden"),
            E::NotFound => error(StatusCode::NOT_FOUND, "not_found"),
            E::Conflict => error(StatusCode::CONFLICT, "conflict"),
            E::InvalidInput => invalid(),
            E::Unsupported => error(StatusCode::NOT_IMPLEMENTED, "unsupported_operation"),
            E::Uncertain(id) => ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "outcome_uncertain",
                operation: Some(id),
            },
            _ => unavailable(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(json!({"schema":SCHEMA,"error":{"code":self.code},"operation_id":self.operation})),
        )
            .into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
            .headers_mut()
            .insert("x-management-contract", HeaderValue::from_static(SCHEMA));
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}
fn now() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| unavailable())?
            .as_millis(),
    )
    .map_err(|_| unavailable())
}
fn response(data: impl Serialize, status: StatusCode) -> Result<Response> {
    let value = json!({"schema":SCHEMA,"observed_at_ms":now()?,"data":data});
    let bytes = serde_json::to_vec(&value).map_err(|_| unavailable())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(error(StatusCode::SERVICE_UNAVAILABLE, "response_too_large"));
    }
    Ok((
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}
fn decode<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T> {
    if body.len() > 65536 {
        return Err(invalid());
    }
    serde_json::from_slice(body).map_err(|_| invalid())
}
struct Work {
    journal: Journal,
    dispatcher: Box<dyn Dispatcher>,
}
#[derive(Default)]
struct ExecutionObservation {
    active: Option<Id>,
    unrecorded: Option<Id>,
}
struct ExecutionGuard<'a> {
    observation: &'a Mutex<ExecutionObservation>,
    complete: bool,
}
impl ExecutionGuard<'_> {
    fn completed(&mut self) {
        if let Ok(mut observed) = self.observation.lock() {
            observed.active = None;
            observed.unrecorded = None;
        }
        self.complete = true;
    }
}
impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        if !self.complete
            && let Ok(mut observed) = self.observation.lock()
            && let Some(id) = observed.active.take()
        {
            observed.unrecorded = Some(id);
        }
    }
}
struct Session {
    identity: Identity,
    version: Digest,
    created: u64,
    expires: u64,
}
#[derive(Clone)]
enum Presented {
    Bearer(String),
    Session(String),
}
/// No listener is created here. The host provides an explicitly bound numeric loopback address,
/// initialized journal/reader, authentication adapter and one-target dispatcher.
pub struct Service {
    target: Id,
    authority: String,
    origin: String,
    cookie_name: String,
    auth: Arc<dyn Authenticator>,
    work: Mutex<Work>,
    reader: Mutex<Reader>,
    sessions: Mutex<BTreeMap<String, Session>>,
    execution: Mutex<ExecutionObservation>,
    web_sessions: bool,
    session_ttl: Duration,
    jobs: Arc<Semaphore>,
}
impl Service {
    pub fn new(
        target: Id,
        bound: SocketAddr,
        auth: Arc<dyn Authenticator>,
        journal: Journal,
        reader: Reader,
        dispatcher: Box<dyn Dispatcher>,
        web_sessions: bool,
    ) -> Result<Arc<Self>> {
        if !bound.ip().is_loopback() || bound.port() == 0 {
            return Err(invalid());
        }
        let authority = bound.to_string();
        let origin = format!("http://{authority}");
        let cookie_name = format!(
            "gateway_view_{}",
            &Digest::of(origin.as_bytes()).as_str()[..16]
        );
        Ok(Arc::new(Self {
            target,
            authority,
            origin,
            cookie_name,
            auth,
            work: Mutex::new(Work {
                journal,
                dispatcher,
            }),
            reader: Mutex::new(reader),
            sessions: Mutex::new(BTreeMap::new()),
            execution: Mutex::new(ExecutionObservation::default()),
            web_sessions,
            session_ttl: Duration::from_secs(900),
            jobs: Arc::new(Semaphore::new(64)),
        }))
    }
    pub fn router(self: &Arc<Self>) -> Router {
        let mut router = Router::new()
            .route("/management/v1/capabilities", get(capabilities))
            .route("/management/v1/state", get(state_view))
            .route("/management/v1/usage", get(usage))
            .route("/management/v1/continuations/{id}", get(continuation))
            .route("/management/v1/preflight", post(preflight))
            .route("/management/v1/operations", get(operations).post(submit))
            .route("/management/v1/operations/{id}", get(operation))
            .route("/management/v1/operations/{id}/reconcile", post(reconcile));
        if self.web_sessions {
            router = router.route("/management/v1/session", post(login).delete(logout));
        }
        router
            .layer(axum::extract::DefaultBodyLimit::max(65536))
            .layer(middleware::from_fn_with_state(self.clone(), boundary))
            .with_state(self.clone())
    }
    fn target(&self, target: &Id) -> Result<()> {
        if target != &self.target {
            Err(error(StatusCode::NOT_FOUND, "not_found"))
        } else {
            Ok(())
        }
    }
    fn operation_view(&self, operation: Operation) -> serde_json::Value {
        let missing = self
            .execution
            .lock()
            .map(|state| state.unrecorded.as_ref() == Some(&operation.id))
            .unwrap_or(true)
            && matches!(
                operation.state,
                gateway_management::State::Queued | gateway_management::State::Running
            );
        let observed = if missing {
            gateway_management::State::Uncertain
        } else {
            operation.state
        };
        json!({"operation":operation,"observed_state":observed,"uncertainty":if missing{Some("result_record_missing")}else{None::<&str>}})
    }
    fn presented(&self, headers: &HeaderMap, mutation: bool) -> Result<Presented> {
        let mut auth = headers.get_all(header::AUTHORIZATION).iter();
        if let Some(value) = auth.next() {
            if auth.next().is_some() {
                return Err(unauthorized());
            }
            let value = value
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .filter(|v| {
                    (32..=4096).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_graphic())
                })
                .ok_or_else(unauthorized)?;
            return Ok(Presented::Bearer(value.into()));
        }
        if mutation || !self.web_sessions {
            return Err(unauthorized());
        }
        let mut cookies = headers.get_all(header::COOKIE).iter();
        let cookie = cookies
            .next()
            .ok_or_else(unauthorized)?
            .to_str()
            .map_err(|_| unauthorized())?;
        if cookies.next().is_some() || cookie.len() > 8192 {
            return Err(unauthorized());
        }
        let mut selected = None;
        for field in cookie.split(';') {
            if let Some((name, value)) = field.trim().split_once('=')
                && name == self.cookie_name
            {
                if selected.is_some()
                    || value.len() != 64
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(unauthorized());
                }
                selected = Some(Digest::of(value.as_bytes()).as_str().to_owned());
            }
        }
        selected.map(Presented::Session).ok_or_else(unauthorized)
    }
    fn principal(&self, presented: &Presented, mutation: bool) -> Result<Principal> {
        let principal = match presented {
            Presented::Bearer(value) => self.auth.authenticate(value).ok_or_else(unauthorized)?,
            Presented::Session(key) => {
                if mutation {
                    return Err(unauthorized());
                }
                let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
                let timestamp = now()?;
                let session = sessions.get(key).ok_or_else(unauthorized)?;
                if timestamp < session.created || timestamp >= session.expires {
                    sessions.remove(key);
                    return Err(unauthorized());
                }
                self.auth
                    .refresh(&session.identity, &session.version)
                    .filter(|p| {
                        p.kind == CredentialKind::ReadOnly
                            && p.actor.identity() == &session.identity
                            && p.authorization_version == session.version
                    })
                    .ok_or_else(unauthorized)?
            }
        };
        if mutation && principal.kind != CredentialKind::Management {
            return Err(error(StatusCode::FORBIDDEN, "read_only_credential"));
        }
        Ok(principal)
    }
    async fn blocking<T: Send + 'static>(
        self: &Arc<Self>,
        f: impl FnOnce(&Self) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let permit = self
            .jobs
            .clone()
            .try_acquire_owned()
            .map_err(|_| error(StatusCode::TOO_MANY_REQUESTS, "management_busy"))?;
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&this)
        })
        .await
        .map_err(|_| unavailable())?
    }
    fn supported(dispatcher: &dyn Dispatcher, action: Action) -> Result<()> {
        if dispatcher.supported().contains(&action) {
            Ok(())
        } else {
            Err(error(StatusCode::NOT_IMPLEMENTED, "unsupported_operation"))
        }
    }
}
async fn boundary(
    State(service): State<Arc<Service>>,
    request: HttpRequest,
    next: Next,
) -> Response {
    let headers = request.headers();
    let mut host = headers.get_all(header::HOST).iter();
    if !matches!((host.next(),host.next()),(Some(h),None) if h.to_str().ok()==Some(service.authority.as_str()))
    {
        return error(StatusCode::BAD_REQUEST, "host_rejected").into_response();
    }
    let mut origin = headers.get_all(header::ORIGIN).iter();
    if let Some(value) = origin.next()
        && (origin.next().is_some() || value.to_str().ok() != Some(service.origin.as_str()))
    {
        return error(StatusCode::FORBIDDEN, "origin_rejected").into_response();
    }
    let mut response = next.run(request).await;
    if (response.status().is_client_error() || response.status().is_server_error())
        && !response.headers().contains_key("x-management-contract")
    {
        let status = response.status();
        let code = match status {
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
            StatusCode::PAYLOAD_TOO_LARGE => "request_too_large",
            StatusCode::BAD_REQUEST
            | StatusCode::UNPROCESSABLE_ENTITY
            | StatusCode::UNSUPPORTED_MEDIA_TYPE => "invalid_request",
            _ => "management_unavailable",
        };
        response = error(status, code).into_response();
    }
    response
        .headers_mut()
        .insert("x-management-contract", HeaderValue::from_static(SCHEMA));
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    target: Id,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    target: Id,
    #[serde(default)]
    after: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    20
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageQuery {
    target: Id,
    from_ms: u64,
    to_ms: u64,
    timezone: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reconcile {
    schema: String,
    target: Id,
}
async fn capabilities(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    HttpQuery(query): HttpQuery<Target>,
) -> Result<Response> {
    service.target(&query.target)?;
    let presented = service.presented(&headers, false)?;
    let value=service.blocking(move|service|{
        let work=service.work.lock().map_err(|_|unavailable())?;let p=service.principal(&presented,false)?;
        p.actor.authorize(Action::ReadState,&service.target)?;
        let supported=work.dispatcher.supported();
        let allowed:Vec<_>=p.actor.capabilities(&service.target,&supported).into_iter().filter(|a|p.kind==CredentialKind::Management||matches!(a,Action::ReadState|Action::ReadUsage|Action::ReadOperations)).collect();
        Ok(json!({"target":service.target,"features":work.dispatcher.features(),"supported_operations":supported,"allowed_operations":allowed,"read_sessions":service.web_sessions,"unsupported_operations":["package_remove","data_delete","remote_package_download"]}))
    }).await?;
    response(value, StatusCode::OK)
}
async fn read_view(
    service: Arc<Service>,
    headers: HeaderMap,
    target: Id,
    query: Query,
) -> Result<Response> {
    service.target(&target)?;
    let presented = service.presented(&headers, false)?;
    let value = service
        .blocking(move |service| {
            let mut work = service.work.lock().map_err(|_| unavailable())?;
            let p = service.principal(&presented, false)?;
            p.actor.authorize(query.action(), &service.target)?;
            Service::supported(work.dispatcher.as_ref(), query.action())?;
            let value = work.dispatcher.read(&p.actor, &query)?;
            if matches!(query, Query::State) {
                let state: crate::StateView =
                    serde_json::from_value(value.clone()).map_err(|_| unavailable())?;
                state.validate().map_err(|_| unavailable())?;
            }
            Ok(value)
        })
        .await?;
    response(value, StatusCode::OK)
}
async fn state_view(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    HttpQuery(query): HttpQuery<Target>,
) -> Result<Response> {
    read_view(service, headers, query.target, Query::State).await
}
async fn usage(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    HttpQuery(query): HttpQuery<UsageQuery>,
) -> Result<Response> {
    let range = UsageRange {
        from_ms: query.from_ms,
        to_ms: query.to_ms,
        timezone: query.timezone,
    };
    range.validate()?;
    read_view(service, headers, query.target, Query::Usage(range)).await
}
async fn continuation(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    Path(session): Path<Id>,
    HttpQuery(query): HttpQuery<Target>,
) -> Result<Response> {
    read_view(service, headers, query.target, Query::Continuation(session)).await
}
async fn operations(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    HttpQuery(query): HttpQuery<List>,
) -> Result<Response> {
    service.target(&query.target)?;
    let presented = service.presented(&headers, false)?;
    let data=service.blocking(move|service|{
        let reader=service.reader.lock().map_err(|_|unavailable())?;let p=service.principal(&presented,false)?;
        let rows=reader.list(&p.actor,&service.target,query.after,query.limit)?;
        Ok(json!({"items":rows.into_iter().map(|(cursor,operation)|{let mut view=service.operation_view(operation);view["cursor"]=json!(cursor);view}).collect::<Vec<_>>() }))
    }).await?;
    response(data, StatusCode::OK)
}
async fn operation(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    Path(id): Path<Id>,
    HttpQuery(query): HttpQuery<Target>,
) -> Result<Response> {
    service.target(&query.target)?;
    let presented = service.presented(&headers, false)?;
    let data = service
        .blocking(move |service| {
            let reader = service.reader.lock().map_err(|_| unavailable())?;
            let p = service.principal(&presented, false)?;
            reader
                .get(&p.actor, &service.target, &id)
                .map(|operation| service.operation_view(operation))
                .map_err(Into::into)
        })
        .await?;
    response(data, StatusCode::OK)
}
struct Bound<'a> {
    dispatcher: &'a mut dyn Dispatcher,
    command: Option<&'a Command>,
    accepted: Option<oneshot::Sender<Id>>,
    execution: &'a Mutex<ExecutionObservation>,
}
struct Prepared<'a> {
    inner: Box<dyn PreparedOperation + 'a>,
    accepted: &'a mut Option<oneshot::Sender<Id>>,
    execution: &'a Mutex<ExecutionObservation>,
}
impl PreparedOperation for Prepared<'_> {
    fn before(&self) -> &Snapshot {
        self.inner.before()
    }
    fn accepted(&mut self, id: &Id) {
        if let Ok(mut observed) = self.execution.lock() {
            observed.active = Some(id.clone());
            observed.unrecorded = None;
        }
        self.inner.accepted(id);
        if let Some(sender) = self.accepted.take() {
            let _ = sender.send(id.clone());
        }
    }
    fn apply(&mut self) -> Effect {
        self.inner.apply()
    }
}
impl Backend for Bound<'_> {
    fn prepare<'a>(
        &'a mut self,
        request: &'a Request,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        let command = self
            .command
            .ok_or(gateway_management::Error::InvalidInput)?;
        let inner = self.dispatcher.prepare(request, command)?;
        Ok(Box::new(Prepared {
            inner,
            accepted: &mut self.accepted,
            execution: self.execution,
        }))
    }
    fn reconcile(&mut self, operation: &Operation) -> gateway_management::Result<Effect> {
        self.dispatcher.reconcile(operation)
    }
}
async fn preflight(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let input: Preflight = decode(&body)?;
    if input.schema != SCHEMA {
        return Err(invalid());
    }
    input.command.validate()?;
    service.target(&input.target)?;
    let presented = service.presented(&headers, true)?;
    let submission = service
        .blocking(move |service| {
            let mut work = service.work.lock().map_err(|_| unavailable())?;
            let p = service.principal(&presented, true)?;
            p.actor.authorize(input.command.action(), &service.target)?;
            Service::supported(work.dispatcher.as_ref(), input.command.action())?;
            let expected = work.dispatcher.snapshot(&input.command)?;
            let submission = Submission {
                schema: SCHEMA.into(),
                target: input.target,
                expected,
                idempotency_key: input.idempotency_key,
                command: input.command,
            };
            let request = submission.request()?;
            let prepared = work.dispatcher.prepare(&request, &submission.command)?;
            if prepared.before() != &submission.expected {
                return Err(error(StatusCode::CONFLICT, "conflict"));
            }
            drop(prepared);
            Ok(submission)
        })
        .await?;
    response(
        json!({"submission":submission,"authorized_execution":false,"applied":false}),
        StatusCode::OK,
    )
}
async fn submit(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let submission: Submission = decode(&body)?;
    let request = submission.request()?;
    service.target(&submission.target)?;
    let presented = service.presented(&headers, true)?;
    let permit = service
        .jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| error(StatusCode::TOO_MANY_REQUESTS, "management_busy"))?;
    let (accepted_tx, mut accepted_rx) = oneshot::channel();
    let (completed_tx, mut completed_rx) = oneshot::channel();
    let worker = service.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let result = (|| {
            let mut work = worker.work.lock().map_err(|_| unavailable())?;
            let mut observation = ExecutionGuard {
                observation: &worker.execution,
                complete: false,
            };
            let p = worker.principal(&presented, true)?;
            p.actor.authorize(request.action, &worker.target)?;
            Service::supported(work.dispatcher.as_ref(), request.action)?;
            let Work {
                journal,
                dispatcher,
            } = &mut *work;
            let result = journal
                .execute(
                    &p.actor,
                    &request,
                    &mut Bound {
                        dispatcher: dispatcher.as_mut(),
                        command: Some(&submission.command),
                        accepted: Some(accepted_tx),
                        execution: &worker.execution,
                    },
                )
                .map_err(ApiError::from);
            if result.is_ok() {
                observation.completed();
            }
            result
        })();
        let _ = completed_tx.send(result);
    });
    let id = tokio::select! {
        accepted=&mut accepted_rx=>match accepted{Ok(id)=>id,Err(_)=>completed_rx.await.map_err(|_|unavailable())??.id},
        completed=&mut completed_rx=>completed.map_err(|_|unavailable())??.id,
    };
    response(json!({"operation_id":id}), StatusCode::ACCEPTED)
}
async fn reconcile(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    Path(id): Path<Id>,
    body: Bytes,
) -> Result<Response> {
    let input: Reconcile = decode(&body)?;
    if input.schema != SCHEMA {
        return Err(invalid());
    }
    service.target(&input.target)?;
    let presented = service.presented(&headers, true)?;
    let data = service
        .blocking(move |service| {
            let mut work = service.work.lock().map_err(|_| unavailable())?;
            let p = service.principal(&presented, true)?;
            p.actor.authorize(Action::Reconcile, &service.target)?;
            Service::supported(work.dispatcher.as_ref(), Action::Reconcile)?;
            let Work {
                journal,
                dispatcher,
            } = &mut *work;
            journal
                .reconcile(
                    &p.actor,
                    &service.target,
                    &id,
                    &mut Bound {
                        dispatcher: dispatcher.as_mut(),
                        command: None,
                        accepted: None,
                        execution: &service.execution,
                    },
                )
                .map_err(Into::into)
        })
        .await?;
    response(data, StatusCode::OK)
}
async fn login(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    if !body.is_empty()
        || headers.get(header::ORIGIN).and_then(|h| h.to_str().ok())
            != Some(service.origin.as_str())
    {
        return Err(invalid());
    }
    let presented = service.presented(&headers, true)?;
    let (secret, expires) = service
        .blocking(move |service| {
            let principal = service.principal(&presented, false)?;
            if principal.kind != CredentialKind::ReadOnly {
                return Err(error(StatusCode::FORBIDDEN, "read_credential_required"));
            }
            principal
                .actor
                .authorize(Action::ReadState, &service.target)?;
            let created = now()?;
            let expires = created
                .checked_add(service.session_ttl.as_millis() as u64)
                .ok_or_else(unavailable)?;
            let mut sessions = service.sessions.lock().map_err(|_| unavailable())?;
            sessions.retain(|_, s| s.expires > created && s.created <= created);
            if sessions.len() >= 256 {
                return Err(error(StatusCode::TOO_MANY_REQUESTS, "session_capacity"));
            }
            let mut bytes = [0u8; 32];
            ring::rand::SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| unavailable())?;
            let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            sessions.insert(
                Digest::of(secret.as_bytes()).as_str().into(),
                Session {
                    identity: principal.actor.identity().clone(),
                    version: principal.authorization_version,
                    created,
                    expires,
                },
            );
            Ok((secret, expires))
        })
        .await?;
    let mut result = response(
        json!({"scope":"read_only","expires_at_ms":expires}),
        StatusCode::OK,
    )?;
    result.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}={secret}; Path=/management/v1; HttpOnly; SameSite=Strict; Max-Age=900",
            service.cookie_name
        ))
        .map_err(|_| unavailable())?,
    );
    Ok(result)
}
async fn logout(
    State(service): State<Arc<Service>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    if !body.is_empty()
        || headers.get(header::ORIGIN).and_then(|h| h.to_str().ok())
            != Some(service.origin.as_str())
    {
        return Err(invalid());
    }
    let presented = service.presented(&headers, false)?;
    let Presented::Session(key) = presented else {
        return Err(invalid());
    };
    service
        .sessions
        .lock()
        .map_err(|_| unavailable())?
        .remove(&key);
    let mut result = response(json!({"closed":true}), StatusCode::OK)?;
    result.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}=; Path=/management/v1; HttpOnly; SameSite=Strict; Max-Age=0",
            service.cookie_name
        ))
        .map_err(|_| unavailable())?,
    );
    Ok(result)
}
