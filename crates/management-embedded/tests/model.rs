#![cfg(feature = "team")]
use gateway_management::{Digest, Id, Identity};
use gateway_management_embedded::{
    HostModelAuthority, MODEL_IDENTITY_SCHEMA, ModelIdentityVerifier, VerifiedModelIdentity,
};
use gateway_team_access::{Permissions, Principal, Purpose};
use gateway_team_http::ModelAuthority;
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
struct Verifier {
    version: AtomicU64,
    schema: &'static str,
    purpose: Purpose,
}
impl Verifier {
    fn evidence(&self) -> VerifiedModelIdentity {
        VerifiedModelIdentity {
            schema: self.schema.into(),
            principal: Principal {
                identity: Identity {
                    subject: id("verified-host-member"),
                    credential: id("host-session"),
                },
                purpose: self.purpose,
                permissions: Permissions {
                    enabled: true,
                    routes: BTreeSet::from(["allowed".into(), "withheld".into()]),
                    management: BTreeSet::new(),
                    read_all_usage: true,
                },
                authorization_version: Digest::of(
                    &self.version.load(Ordering::SeqCst).to_be_bytes(),
                ),
            },
        }
    }
}
impl ModelIdentityVerifier for Verifier {
    fn authenticate(&self, value: &str) -> Option<VerifiedModelIdentity> {
        (value == "synthetic-host-credential").then(|| self.evidence())
    }
    fn refresh(&self, _: &Identity, _: &Digest) -> Option<VerifiedModelIdentity> {
        Some(self.evidence())
    }
}
#[test]
fn verified_host_model_authority_needs_no_local_team_store_and_cannot_expand_permissions() {
    let verifier = Arc::new(Verifier {
        version: AtomicU64::new(1),
        schema: MODEL_IDENTITY_SCHEMA,
        purpose: Purpose::Model,
    });
    let authority = HostModelAuthority::new(
        id("gateway"),
        BTreeSet::from(["allowed".into()]),
        false,
        verifier.clone(),
    )
    .unwrap();
    let principal = authority
        .authenticate_model("synthetic-host-credential")
        .unwrap();
    assert_eq!(authority.target(), &id("gateway"));
    assert!(principal.permissions.permits_route("allowed"));
    assert!(!principal.permissions.permits_route("withheld"));
    assert!(!principal.permissions.read_all_usage);
    assert!(principal.permissions.management.is_empty());
    assert!(authority.authenticate_model("subject:admin").is_none());
    assert!(
        authority
            .refresh_model(&principal.identity, &principal.authorization_version)
            .is_some()
    );
    verifier.version.store(2, Ordering::SeqCst);
    assert!(
        authority
            .refresh_model(&principal.identity, &principal.authorization_version)
            .is_none()
    );
    for (schema, purpose) in [
        ("unsupported/v2", Purpose::Model),
        (MODEL_IDENTITY_SCHEMA, Purpose::Management),
    ] {
        let authority = HostModelAuthority::new(
            id("gateway"),
            BTreeSet::from(["allowed".into()]),
            true,
            Arc::new(Verifier {
                version: AtomicU64::new(1),
                schema,
                purpose,
            }),
        )
        .unwrap();
        assert!(
            authority
                .authenticate_model("synthetic-host-credential")
                .is_none()
        );
    }
}
#[tokio::test]
async fn host_identity_drives_real_team_transport_without_a_local_credential_database() {
    use axum::{
        Json, Router,
        body::{Body, to_bytes},
        http::{HeaderMap, Request, StatusCode},
        routing::{get, post},
    };
    use gateway_team_http::{Ledger, Limits, Peer, PeerIdentity, Route, Service};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use tower::ServiceExt;
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/embedded-model-tests");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let local = "synthetic-host-gateway-key-01234567890123456789";
    let gateway = Router::new()
        .route(
            "/v1/models",
            get(move |headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], format!("Bearer {local}"));
                Json(json!({"object":"list","data":[{"id":"allowed"},{"id":"withheld"}]}))
            }),
        )
        .route(
            "/v1/responses",
            post(move |headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], format!("Bearer {local}"));
                (
                    [("x-request-id", "21b6bdfa-5db8-4ce1-a8d6-a0943b516731")],
                    Json(json!({"object":"response","output":[]})),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, gateway).await.unwrap();
    });
    let authority = HostModelAuthority::new(
        id("gateway"),
        BTreeSet::from(["allowed".into()]),
        false,
        Arc::new(Verifier {
            version: AtomicU64::new(1),
            schema: MODEL_IDENTITY_SCHEMA,
            purpose: Purpose::Model,
        }),
    )
    .unwrap();
    let peer = Peer::new(
        PeerIdentity {
            target: id("gateway"),
            instance: id("synthetic-host-instance"),
            configuration_sha256: Digest::of(b"synthetic"),
            producer: None,
        },
        &endpoint,
        BTreeMap::from([
            ("allowed".into(), Route { managed: None }),
            ("withheld".into(), Route { managed: None }),
        ]),
        local.into(),
        None,
    )
    .unwrap();
    let ledger = Ledger::initialize(directory.path(), id("gateway"), 8 * 1024 * 1024).unwrap();
    let bound = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = Service::new(
        bound.local_addr().unwrap(),
        Arc::new(authority),
        Arc::new(peer),
        ledger,
        None,
        Limits::default(),
    )
    .unwrap();
    let request = |method: &str, path: &str, body: Value| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", bound.local_addr().unwrap().to_string())
            .header("authorization", "Bearer synthetic-host-credential")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let models = service
        .router()
        .oneshot(request("GET", "/v1/models", Value::Null))
        .await
        .unwrap();
    assert_eq!(models.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(models.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(value["data"], json!([{"id":"allowed"}]));
    let denied = service
        .router()
        .oneshot(request(
            "POST",
            "/v1/responses",
            json!({"model":"withheld","input":"synthetic"}),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let allowed = service
        .router()
        .oneshot(request(
            "POST",
            "/v1/responses",
            json!({"model":"allowed","input":"synthetic"}),
        ))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    to_bytes(allowed.into_body(), 65536).await.unwrap();
    assert!(!directory.path().join("team.sqlite3").exists());
    assert!(!directory.path().join("management.sqlite3").exists());
    task.abort();
}
