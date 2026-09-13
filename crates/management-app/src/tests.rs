#![cfg(feature = "team")]
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use gateway_management::{Action, Actor, Digest, Grant, Identity};
use gateway_management_api::{Authenticator, CredentialKind, LocalAuthenticator, LocalCredential};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
use tower::ServiceExt;
const TOKEN: &str = "synthetic-management-key-01234567890123456789";
fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn private(path: &Path) {
    std::fs::create_dir(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    journal: PathBuf,
    team: PathBuf,
    service: Arc<Service>,
    auth: Arc<gateway_team_access::Authenticator>,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/app-tests");
        std::fs::create_dir_all(&root).unwrap();
        let root = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let journal = root.path().join("journal");
        let team = root.path().join("team");
        let runtime = root.path().join("runtime");
        for p in [&journal, &team, &runtime] {
            private(p)
        }
        let writer = Journal::initialize(&journal, STORE_BYTES).unwrap();
        let manager =
            gateway_team_access::Manager::initialize(&team, id("gateway"), STORE_BYTES).unwrap();
        let auth = Arc::new(
            gateway_team_access::Authenticator::open(
                &team,
                id("gateway"),
                Reader::open(&journal).unwrap(),
            )
            .unwrap(),
        );
        let identity = Identity {
            subject: id("local:operator"),
            credential: id("local:credential"),
        };
        let actions = [
            Action::TeamSubjectRegister,
            Action::TeamCredentialIssue,
            Action::TeamCredentialRotate,
            Action::TeamCredentialRevoke,
            Action::TeamPermissionsChange,
            Action::Reconcile,
            Action::ReadOperations,
            Action::ReadState,
        ];
        let local = LocalAuthenticator::new(vec![LocalCredential {
            token: TOKEN.into(),
            identity: identity.clone(),
            kind: CredentialKind::Management,
            grants: actions
                .into_iter()
                .map(|action| Grant {
                    action,
                    target: id("gateway"),
                })
                .collect(),
        }])
        .unwrap();
        let owner = Runtime::initialize(gateway_management_runtime::Registration {
            target: id("gateway"),
            directory: runtime,
            executable: root.path().join("never-started"),
            executable_sha256: Digest::of(b"no execution"),
            credential_generation: id("test"),
            sources: BTreeMap::new(),
            environment: BTreeMap::new(),
            startup_timeout: std::time::Duration::from_secs(1),
            stop_timeout: std::time::Duration::from_secs(1),
        })
        .unwrap();
        let adapters = dispatch::Adapters {
            web_enabled: false,
            target: id("gateway"),
            runtime: Arc::new(Mutex::new(owner)),
            native: None,
            profiles: None,
            usage: None,
            control: None,
            local_identities: [(identity.subject, identity.credential)].into(),
            team: Some(manager),
            team_auth: Some(auth.clone()),
            team_http: None,
            secret: None,
        };
        let service = Service::new(
            id("gateway"),
            "127.0.0.1:46391".parse().unwrap(),
            Arc::new(team::Authority {
                local,
                team: Some(auth.clone()),
            }),
            writer,
            Reader::open(&journal).unwrap(),
            Box::new(adapters),
            false,
        )
        .unwrap();
        Self {
            _root: root,
            journal,
            team,
            service,
            auth,
        }
    }
    async fn call(&self, path: &str, body: Value, origin: bool) -> (u16, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/management/v1{path}"))
            .header("host", "127.0.0.1:46391")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("content-type", "application/json");
        if origin {
            request = request.header("origin", "http://127.0.0.1:46391")
        }
        let result = self
            .service
            .router()
            .oneshot(
                request
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let code = result.status().as_u16();
        let body = to_bytes(result.into_body(), 65536).await.unwrap();
        (code, serde_json::from_slice(&body).unwrap())
    }
    async fn prepare(&self, command: Value, key: &str) -> Value {
        let(code,data)=self.call("/preflight",json!({"schema":gateway_management_api::SCHEMA,"target":"gateway","idempotency_key":key,"command":command}),false).await;
        assert_eq!(code, 200);
        data["data"]["submission"].clone()
    }
    async fn register(&self) {
        let p=self.prepare(json!({"kind":"team_subject_register","subject":"member","permissions":{"enabled":true,"routes":["a"],"management":[],"read_all_usage":false}}),"register").await;
        assert_eq!(self.call("/operations", p, false).await.0, 202);
        for _ in 0..100 {
            if self.count("subjects") == 1 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("registration timeout")
    }
    fn count(&self, table: &str) -> usize {
        assert!(matches!(table, "subjects" | "credentials"));
        let db = rusqlite::Connection::open(self.team.join("team.sqlite3")).unwrap();
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        .try_into()
        .unwrap()
    }
    fn trigger(&self, phase: &str) {
        let db = rusqlite::Connection::open(self.journal.join("management.sqlite3")).unwrap();
        assert!(matches!(phase, "accepted" | "finished"));
        db.execute_batch(&format!("CREATE TRIGGER test_failure BEFORE INSERT ON events WHEN json_extract(NEW.event,'$.phase')='{phase}' BEGIN SELECT RAISE(FAIL,'synthetic audit failure'); END;")).unwrap();
    }
}
#[tokio::test]
async fn one_time_delivery_uses_exact_wire_receipts_and_never_returns_keys_from_retries() {
    let f = Fixture::new();
    f.register().await;
    let p=f.prepare(json!({"kind":"team_credential_issue","subject":"member","credential":"model-key","purpose":"model"}),"issue").await;
    assert_eq!(f.call("/operations", p.clone(), false).await.0, 400);
    assert_eq!(f.call("/credential-delivery", p.clone(), true).await.0, 403);
    assert_eq!(f.count("credentials"), 0);
    let (code, value) = f.call("/credential-delivery", p.clone(), false).await;
    assert_eq!(code, 200);
    let secret = value["data"]["credential"].as_str().unwrap();
    assert!(f.auth.authenticate_model(secret).is_some());
    assert!(f.auth.authenticate(secret).is_none());
    let again = f.call("/credential-delivery", p.clone(), false).await;
    assert_eq!(again.0, 200);
    assert!(again.1["data"]["credential"].is_null());
    assert_eq!(f.count("credentials"), 1);
    let reader = Reader::open(&f.journal).unwrap();
    let actor = Actor::new(
        Identity {
            subject: id("reader"),
            credential: id("reader"),
        },
        [Grant {
            target: id("gateway"),
            action: Action::ReadOperations,
        }],
    )
    .unwrap();
    let op = reader
        .get(
            &actor,
            &id("gateway"),
            &id(value["data"]["operation_id"].as_str().unwrap()),
        )
        .unwrap();
    let submission: gateway_management_api::Submission = serde_json::from_value(p).unwrap();
    assert_eq!(
        op.request.fingerprint(),
        submission.request().unwrap().fingerprint()
    );
    assert!(!serde_json::to_string(&op).unwrap().contains(secret));
    let mut altered = serde_json::to_value(submission).unwrap();
    altered["command"]["credential"] = json!("different");
    assert_eq!(f.call("/credential-delivery", altered, false).await.0, 409);
}
#[tokio::test]
async fn audit_failure_before_issuance_prevents_effect_and_result_failure_consumes_output() {
    for phase in ["accepted", "finished"] {
        let f = Fixture::new();
        f.register().await;
        let p=f.prepare(json!({"kind":"team_credential_issue","subject":"member","credential":"model-key","purpose":"model"}),"issue").await;
        f.trigger(phase);
        let (code, value) = f.call("/credential-delivery", p.clone(), false).await;
        assert_eq!(code, 503);
        assert!(value.get("data").is_none());
        assert_eq!(f.count("credentials"), usize::from(phase == "finished"));
        let db = rusqlite::Connection::open(f.journal.join("management.sqlite3")).unwrap();
        db.execute_batch("DROP TRIGGER test_failure").unwrap();
        if phase == "finished" {
            let operation = value["operation_id"].as_str().unwrap();
            let (code, _) = f
                .call(
                    &format!("/operations/{operation}/reconcile"),
                    json!({"schema":gateway_management_api::SCHEMA,"target":"gateway"}),
                    false,
                )
                .await;
            assert_eq!(code, 200);
            let (code, retry) = f.call("/credential-delivery", p, false).await;
            assert_eq!(code, 200);
            assert!(retry["data"]["credential"].is_null());
            assert_eq!(f.count("credentials"), 1);
        }
    }
}
#[tokio::test]
async fn reserved_local_namespace_and_stale_generation_reject_before_team_mutation() {
    let f = Fixture::new();
    let(code,_)=f.call("/preflight",json!({"schema":gateway_management_api::SCHEMA,"target":"gateway","idempotency_key":"collision","command":{"kind":"team_subject_register","subject":"local:operator","permissions":{"enabled":true,"routes":[],"management":[],"read_all_usage":false}}}),false).await;
    assert_eq!(code, 403);
    assert_eq!(f.count("subjects"), 0);
    f.register().await;
    let mut p=f.prepare(json!({"kind":"team_credential_issue","subject":"member","credential":"key","purpose":"model"}),"stale").await;
    p["expected"]["revision"] = json!(0);
    assert_eq!(f.call("/credential-delivery", p, false).await.0, 409);
    assert_eq!(f.count("credentials"), 0);
}

#[tokio::test]
async fn shutdown_refuses_prepared_or_queued_new_effects_before_audit_admission() {
    let f = Fixture::new();
    f.register().await;
    let prepared=f.prepare(json!({"kind":"team_credential_issue","subject":"member","credential":"closing-key","purpose":"model"}),"closing").await;
    f.service.close_admission();
    assert_eq!(f.call("/credential-delivery", prepared, false).await.0, 503);
    assert_eq!(f.count("credentials"), 0);
}
