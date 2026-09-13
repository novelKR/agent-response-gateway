//! Synthetic browser verification host. Not a product runtime or deployment example.
use axum::{
    Router,
    body::Body,
    extract::{Request as HttpRequest, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use clap::Parser;
use gateway_management::{
    Action, Actor, Backend, Digest, Effect, Error, FailureCode, Grant, Id, Identity, Journal,
    Operation, PreparedOperation, Reader, Request, Snapshot,
};
use gateway_management_api::{
    Command, CredentialKind, Dispatcher, Feature, LocalAuthenticator, LocalCredential, Query,
    Service,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read, path::PathBuf, sync::Arc, time::SystemTime};
const READ: &str = "synthetic-browser-read-key-01234567890123456789";
#[derive(Parser)]
struct Options {
    #[arg(long)]
    assets: PathBuf,
    #[arg(long, default_value_t = 0)]
    port: u16,
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn snapshot() -> Snapshot {
    Snapshot {
        revision: 1,
        digest: Digest::of(b"synthetic browser fixture"),
    }
}
fn actions() -> Vec<Action> {
    vec![Action::ReadState, Action::ReadUsage, Action::ReadOperations]
}
struct Fixture;
impl Dispatcher for Fixture {
    fn features(&self) -> Vec<Feature> {
        vec![Feature {
            id: id("synthetic-browser-fixture"),
            version: "fixture/v1".into(),
            installed: true,
            enabled: true,
            operations: actions(),
        }]
    }
    fn supported(&self) -> Vec<Action> {
        actions()
    }
    fn snapshot(&mut self, _: &Command) -> gateway_management::Result<Snapshot> {
        Err(Error::Unsupported)
    }
    fn read(&mut self, _: &Actor, query: &Query) -> gateway_management::Result<Value> {
        let old = Digest::of(b"old synthetic package");
        let new = Digest::of(b"new synthetic package");
        let module = |name: &str, contract: &str, data: Value| json!({"id":name,"contract":contract,"observation":{"state":"observed","observed_at_ms":now()-60000,"data":data}});
        Ok(match query {
            Query::State => json!({"schema":gateway_management_api::STATE_SCHEMA,"modules":[
                module("runtime", "gateway-runtime-status/v1", json!({"schema":"gateway-runtime-status/v1","target":"gateway","revision":3,"selected":"candidate-next","candidates":["candidate-current","candidate-next"],"external_change":false,"ownership":"owned","running":{"instance_id":"synthetic-instance","gateway":{"configuration_sha256":Digest::of(b"current configuration"),"execution_sha256":Digest::of(b"current execution")}},"running_manifest":{"models":["synthetic-model"],"readiness_version":7},"desired":{"models":["synthetic-model","synthetic-next"]},"desired_valid":true,"restart_required":true})),
                module("native", "gateway-extension-status/v1", json!({"schema":"gateway-extension-status/v1","target":"gateway","store":{"inventory":{"installed":[{"id":"synthetic-codec","version":"1.0.0","package_sha256":old,"verified":true,"package":{"permissions":["network"]}},{"id":"synthetic-codec","version":"2.0.0","package_sha256":new,"verified":true,"package":{"permissions":["network"]}}],"activation":{"extensions":[{"id":"synthetic-codec","version":"2.0.0","package_sha256":new,"grants":["network"]}]}}},"effective":{"packages":[{"id":"synthetic-codec","version":"1.0.0","package_sha256":old}]},"removal_supported":false})),
                {"id":"profiles","contract":"gateway-extension-status/v1","observation":{"state":"unobserved","reason":"host_observation_unavailable"}}
            ]}),
            Query::Usage(_) => json!({"schema":"synthetic-usage/v1","groups":[
                {"date":"2026-01-02","model_alias":"synthetic-model","provider":"mock","calls":4,"final":2,"partial":1,"unobserved":1,"unfinished":1,"token_sums":{"input_tokens":1250,"output_tokens":null}},
                {"date":"2026-01-01","model_alias":"synthetic-large","provider":"mock","calls":1,"final":1,"partial":0,"unobserved":0,"unfinished":0,"token_sums":{"input_tokens":9007199254740993123u64,"output_tokens":0}}
            ]}),
            Query::Continuation(_) => return Err(Error::Unsupported),
        })
    }
    fn prepare<'a>(
        &'a mut self,
        _: &'a Request,
        _: &'a Command,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        Err(Error::Unsupported)
    }
    fn reconcile(&mut self, _: &Operation) -> gateway_management::Result<Effect> {
        Err(Error::Unsupported)
    }
}
struct Seed(Snapshot);
impl PreparedOperation for Seed {
    fn before(&self) -> &Snapshot {
        &self.0
    }
    fn apply(&mut self) -> Effect {
        Effect::Uncertain {
            code: FailureCode::Unverified,
        }
    }
}
impl Backend for Seed {
    fn prepare<'a>(
        &'a mut self,
        _: &'a Request,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        Ok(Box::new(Seed(self.0.clone())))
    }
    fn reconcile(&mut self, _: &Operation) -> gateway_management::Result<Effect> {
        Err(Error::Unsupported)
    }
}
struct Assets {
    authority: String,
    files: BTreeMap<String, Vec<u8>>,
}
async fn asset(State(state): State<Arc<Assets>>, request: HttpRequest) -> Response {
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    if !matches!((hosts.next(),hosts.next()),(Some(h),None) if h.to_str().ok()==Some(&state.authority))
        || origins.next().is_some_and(|o| {
            o.to_str().ok() != Some(format!("http://{}", state.authority).as_str())
                || origins.next().is_some()
        })
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = request.uri().path();
    let name = if path == "/dashboard/" {
        "index.html"
    } else {
        path.strip_prefix("/dashboard/").unwrap_or("")
    };
    let Some(bytes) = state.files.get(name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let kind = if name.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if name.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if name.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if name.ends_with(".json") {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    };
    Response::builder().header(header::CONTENT_TYPE,kind).header(header::CACHE_CONTROL,"no-store").header("x-content-type-options","nosniff").header("referrer-policy","no-referrer").header("content-security-policy","default-src 'self'; connect-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'").body(Body::from(bytes.clone())).unwrap()
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    let root = options.assets.canonicalize()?;
    let manifest_bytes = std::fs::read(root.join("web-manifest.json"))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    if manifest["schema"] != "gateway-management-web/v1" || manifest["read_only"] != true {
        return Err("invalid Web manifest".into());
    }
    let mut files = BTreeMap::new();
    for (name, digest) in manifest["files"]
        .as_object()
        .ok_or("missing file inventory")?
    {
        if name
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
            || name.contains('\\')
        {
            return Err("invalid asset path".into());
        }
        let path = root.join(name);
        if path.canonicalize()? != path || !path.is_file() {
            return Err("linked or invalid asset".into());
        }
        let bytes = std::fs::read(path)?;
        if Some(Digest::of(&bytes).as_str()) != digest.as_str() {
            return Err("asset digest differs".into());
        }
        files.insert(name.clone(), bytes);
    }
    files.insert("web-manifest.json".into(), manifest_bytes);
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/web-fixtures");
    std::fs::create_dir_all(&base)?;
    let directory = tempfile::tempdir_in(base.canonicalize()?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let mut journal = Journal::initialize(directory.path(), 8 * 1024 * 1024)?;
    let actor = Actor::new(
        Identity {
            subject: id("synthetic-operator"),
            credential: id("fixture-key"),
        },
        [Grant {
            action: Action::RuntimeStart,
            target: id("gateway"),
        }],
    )?;
    journal.execute(
        &actor,
        &Request {
            target: id("gateway"),
            action: Action::RuntimeStart,
            expected: snapshot(),
            idempotency_key: id("synthetic-operation"),
            parameters_sha256: Digest::of(b"fixture"),
        },
        &mut Seed(snapshot()),
    )?;
    let reader = Reader::open(directory.path())?;
    let auth = LocalAuthenticator::new(vec![LocalCredential {
        token: READ.into(),
        identity: Identity {
            subject: id("synthetic-reader"),
            credential: id("fixture-read"),
        },
        kind: CredentialKind::ReadOnly,
        grants: actions()
            .into_iter()
            .map(|action| Grant {
                action,
                target: id("gateway"),
            })
            .collect(),
    }])?;
    let listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, options.port)).await?;
    let bound = listener.local_addr()?;
    let service = Service::new(
        id("gateway"),
        bound,
        Arc::new(auth),
        journal,
        reader,
        Box::new(Fixture),
        true,
    )
    .map_err(|_| "fixture API setup failed")?;
    let assets = Arc::new(Assets {
        authority: bound.to_string(),
        files,
    });
    let static_routes = Router::new()
        .route("/dashboard/", get(asset))
        .route("/dashboard/{*path}", get(asset))
        .with_state(assets);
    let (send, receive) = tokio::sync::oneshot::channel::<()>();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1];
        let _ = std::io::stdin().read(&mut buffer);
        let _ = send.send(());
    });
    println!("http://{bound}/dashboard/");
    axum::serve(listener, service.router().merge(static_routes))
        .with_graceful_shutdown(async {
            let _ = receive.await;
        })
        .await?;
    Ok(())
}
