use crate::settings::{Web, bytes};
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use gateway_management::{Digest, Error, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    api_contract: String,
    state_contract: String,
    source_commit: String,
    source_dirty: bool,
    source_url: String,
    read_only: bool,
    files: BTreeMap<String, Digest>,
}
pub struct Assets {
    files: BTreeMap<String, (Vec<u8>, &'static str)>,
    authority: String,
}
impl Assets {
    pub fn load(settings: &Web, bound: SocketAddr) -> Result<Self> {
        let raw = bytes(&settings.directory.join("web-manifest.json"), 65536, false)?;
        if Digest::of(&raw) != settings.manifest_sha256 {
            return Err(Error::Conflict);
        }
        let manifest: Manifest = serde_json::from_slice(&raw).map_err(|_| Error::InvalidInput)?;
        if manifest.schema != "gateway-management-web/v1"
            || manifest.api_contract != gateway_management_api::SCHEMA
            || manifest.state_contract != gateway_management_api::STATE_SCHEMA
            || manifest.source_dirty
            || !manifest.read_only
            || manifest.source_commit.len() != 40
            || !manifest
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || manifest.source_url != "https://github.com/novelKR/agent-response-gateway"
            || manifest.files.len() > 64
        {
            return Err(Error::InvalidInput);
        }
        let mut files = BTreeMap::new();
        let mut total = 0;
        for (path, digest) in manifest.files {
            if !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
                || path
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == "..")
                || path == "web-manifest.json"
            {
                return Err(Error::InvalidInput);
            }
            let data = bytes(&settings.directory.join(&path), 4 * 1024 * 1024, false)?;
            total += data.len();
            if total > 16 * 1024 * 1024 || Digest::of(&data) != digest {
                return Err(Error::Conflict);
            }
            let mime = if path.ends_with(".html") {
                "text/html; charset=utf-8"
            } else if path.ends_with(".js") {
                "text/javascript; charset=utf-8"
            } else if path.ends_with(".css") {
                "text/css; charset=utf-8"
            } else if path.ends_with(".json") {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            files.insert(path, (data, mime));
        }
        if [
            "index.html",
            "LICENSE.txt",
            "web-notices.txt",
            "web-dependencies.json",
        ]
        .iter()
        .any(|n| !files.contains_key(*n))
        {
            return Err(Error::InvalidInput);
        }
        // Only allowlisted pinned bytes are served; new/unlisted files can never become routes.
        files.insert("web-manifest.json".into(), (raw, "application/json"));
        Ok(Self {
            files,
            authority: bound.to_string(),
        })
    }
    pub fn router(self) -> Router {
        Router::new()
            .route("/", get(serve))
            .route("/{*path}", get(serve))
            .with_state(Arc::new(self))
    }
}
async fn serve(State(assets): State<Arc<Assets>>, request: Request) -> Response {
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    if !matches!((hosts.next(),hosts.next()),(Some(h),None) if h.to_str().ok()==Some(&assets.authority))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if origins.next().is_some_and(|o| {
        o.to_str().ok() != Some(format!("http://{}", assets.authority).as_str())
            || origins.next().is_some()
    }) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some((bytes, mime)) = assets.files.get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (StatusCode::OK,[(header::CONTENT_TYPE,*mime),(header::CACHE_CONTROL,"no-store"),
        (header::CONTENT_SECURITY_POLICY,"default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"),
        (header::X_CONTENT_TYPE_OPTIONS,"nosniff"),(header::REFERRER_POLICY,"no-referrer")],bytes.clone()).into_response()
}
