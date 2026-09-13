use gateway_management::{
    Action, Actor, Digest, Effect, Error, Id, Identity, Journal, Operation, PreparedOperation,
    Reader, Request, Snapshot,
};
use gateway_management_api::{
    Authenticator, Command, CredentialKind, Dispatcher, Feature, Principal, Query,
};
use gateway_management_embedded::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tower::ServiceExt;
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn actor() -> Actor {
    Actor::new(
        Identity {
            subject: id("synthetic-host-operator"),
            credential: id("host-credential"),
        },
        [
            Action::ReadState,
            Action::ReadUsage,
            Action::ReadOperations,
            Action::RuntimeStart,
            Action::ConfigurationSelect,
        ]
        .into_iter()
        .map(|action| gateway_management::Grant {
            action,
            target: id("gateway"),
        }),
    )
    .unwrap()
}
fn contract() -> HostContract {
    HostContract {
        schema: HOST_SCHEMA.into(),
        target: id("gateway"),
        lifecycle: Lifecycle::HostOwned,
        operations: BTreeSet::from([
            Action::ReadState,
            Action::ReadOperations,
            Action::ConfigurationSelect,
        ]),
    }
}
struct Verifier {
    schema: &'static str,
    revision: AtomicU64,
    swap: bool,
}
impl Verifier {
    fn evidence(&self) -> VerifiedIdentity {
        let actor = if self.swap {
            Actor::new(
                Identity {
                    subject: id("different"),
                    credential: id("different-key"),
                },
                [],
            )
            .unwrap()
        } else {
            actor()
        };
        VerifiedIdentity {
            schema: self.schema.into(),
            principal: Principal {
                actor,
                kind: CredentialKind::Management,
                authorization_version: Digest::of(
                    &self.revision.load(Ordering::SeqCst).to_be_bytes(),
                ),
            },
        }
    }
}
impl IdentityVerifier for Verifier {
    fn authenticate(&self, token: &str) -> Option<VerifiedIdentity> {
        (token == "synthetic-host-token-01234567890123456789").then(|| self.evidence())
    }
    fn refresh(&self, _: &Identity, _: &Digest) -> Option<VerifiedIdentity> {
        Some(self.evidence())
    }
}
struct Backend {
    generation: Arc<AtomicU64>,
}
fn snapshot(generation: u64) -> Snapshot {
    Snapshot {
        revision: generation,
        digest: Digest::of(&generation.to_be_bytes()),
    }
}
struct Prepared {
    generation: Arc<AtomicU64>,
    before: Snapshot,
}
impl PreparedOperation for Prepared {
    fn before(&self) -> &Snapshot {
        &self.before
    }
    fn apply(&mut self) -> Effect {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        Effect::Applied {
            after: snapshot(generation),
            evidence_sha256: Digest::of(b"synthetic host observed change"),
        }
    }
}
impl Dispatcher for Backend {
    fn features(&self) -> Vec<Feature> {
        vec![Feature {
            id: id("synthetic-host"),
            version: "fixture/v1".into(),
            installed: true,
            enabled: true,
            operations: self.supported(),
        }]
    }
    fn supported(&self) -> Vec<Action> {
        vec![
            Action::ReadState,
            Action::ReadOperations,
            Action::ConfigurationSelect,
            Action::RuntimeStart,
        ]
    }
    fn snapshot(&mut self, _: &Command) -> gateway_management::Result<Snapshot> {
        Ok(snapshot(self.generation.load(Ordering::SeqCst)))
    }
    fn read(&mut self, _: &Actor, _: &Query) -> gateway_management::Result<Value> {
        Ok(
            json!({"schema":gateway_management_api::STATE_SCHEMA,"modules":[{"id":"synthetic","contract":"synthetic-host-state/v1","observation":{"state":"observed","observed_at_ms":1,"data":{"schema":"synthetic-host-state/v1","revision":self.generation.load(Ordering::SeqCst)}}}]}),
        )
    }
    fn prepare<'a>(
        &'a mut self,
        _: &'a Request,
        command: &'a Command,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        if !matches!(command, Command::RuntimeStart {})
            && !matches!(command,Command::ConfigurationSelect{candidate} if candidate==&id("synthetic-candidate"))
        {
            return Err(Error::Unsupported);
        }
        Ok(Box::new(Prepared {
            before: snapshot(self.generation.load(Ordering::SeqCst)),
            generation: self.generation.clone(),
        }))
    }
    fn reconcile(&mut self, _: &Operation) -> gateway_management::Result<Effect> {
        Err(Error::Unsupported)
    }
}
#[test]
fn host_contract_rejects_unsupported_versions_and_undelegated_lifecycle() {
    let generation = Arc::new(AtomicU64::new(0));
    let mut declaration = contract();
    let mut backend = HostDispatcher::new(
        declaration.clone(),
        Box::new(Backend {
            generation: generation.clone(),
        }),
    )
    .unwrap();
    assert!(!backend.supported().contains(&Action::RuntimeStart));
    assert!(
        !backend.features()[0]
            .operations
            .contains(&Action::RuntimeStart)
    );
    assert!(matches!(
        backend.snapshot(&Command::RuntimeStart {}),
        Err(Error::Unsupported)
    ));
    let command = Command::RuntimeStart {};
    let request = Request {
        target: id("gateway"),
        action: Action::RuntimeStart,
        expected: snapshot(0),
        idempotency_key: id("never-run"),
        parameters_sha256: command.digest().unwrap(),
    };
    assert!(matches!(
        backend.prepare(&request, &command),
        Err(Error::Unsupported)
    ));
    assert_eq!(generation.load(Ordering::SeqCst), 0);
    declaration.operations.insert(Action::RuntimeStart);
    assert!(matches!(declaration.validate(), Err(Error::InvalidInput)));
    declaration.lifecycle = Lifecycle::Delegated;
    assert!(declaration.validate().is_ok());
    declaration.schema = "gateway-embedded-management/v2".into();
    assert!(matches!(
        declaration.validate(),
        Err(Error::UnsupportedSchema)
    ));
    let mut missing = contract();
    missing.operations.insert(Action::PackageInstall);
    assert!(HostDispatcher::new(missing, Box::new(Backend { generation })).is_err());
}
#[test]
fn host_identity_is_verified_versioned_reduced_and_refreshed() {
    let verifier = Arc::new(Verifier {
        schema: IDENTITY_SCHEMA,
        revision: AtomicU64::new(1),
        swap: false,
    });
    let auth = HostAuthenticator::new(contract(), verifier.clone()).unwrap();
    assert!(auth.authenticate("untrusted subject:operator").is_none());
    let principal = auth
        .authenticate("synthetic-host-token-01234567890123456789")
        .unwrap();
    assert!(
        principal
            .actor
            .authorize(Action::ReadState, &id("gateway"))
            .is_ok()
    );
    assert!(
        principal
            .actor
            .authorize(Action::ReadUsage, &id("gateway"))
            .is_err()
    );
    assert!(
        principal
            .actor
            .authorize(Action::RuntimeStart, &id("gateway"))
            .is_err()
    );
    assert!(
        auth.refresh(principal.actor.identity(), &principal.authorization_version)
            .is_some()
    );
    verifier.revision.store(2, Ordering::SeqCst);
    assert!(
        auth.refresh(principal.actor.identity(), &principal.authorization_version)
            .is_none()
    );
    let invalid = HostAuthenticator::new(
        contract(),
        Arc::new(Verifier {
            schema: "unverified/v9",
            revision: AtomicU64::new(1),
            swap: false,
        }),
    )
    .unwrap();
    assert!(
        invalid
            .authenticate("synthetic-host-token-01234567890123456789")
            .is_none()
    );
    let swapped = HostAuthenticator::new(
        contract(),
        Arc::new(Verifier {
            schema: IDENTITY_SCHEMA,
            revision: AtomicU64::new(1),
            swap: true,
        }),
    )
    .unwrap();
    assert!(
        swapped
            .refresh(principal.actor.identity(), &principal.authorization_version)
            .is_none()
    );
}
#[tokio::test]
async fn real_api_enforces_host_limits_and_refuses_body_identity_injection() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request as HttpRequest, StatusCode},
    };
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/embedded-tests");
    std::fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let journal = Journal::initialize(directory.path(), 8 * 1024 * 1024).unwrap();
    let reader = Reader::open(directory.path()).unwrap();
    let generation = Arc::new(AtomicU64::new(0));
    let host = HostDispatcher::new(
        contract(),
        Box::new(Backend {
            generation: generation.clone(),
        }),
    )
    .unwrap();
    let verifier = Arc::new(Verifier {
        schema: IDENTITY_SCHEMA,
        revision: AtomicU64::new(1),
        swap: false,
    });
    let service = host
        .service(
            "127.0.0.1:47300".parse().unwrap(),
            verifier,
            journal,
            reader,
            false,
        )
        .unwrap();
    let request = |method: &str, path: &str, body: Value| {
        HttpRequest::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:47300")
            .header(
                "authorization",
                "Bearer synthetic-host-token-01234567890123456789",
            )
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    };
    let capabilities = service
        .router()
        .oneshot(request(
            "GET",
            "/management/v1/capabilities?target=gateway",
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(capabilities.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(capabilities.into_body(), 65536).await.unwrap()).unwrap();
    assert!(
        !value["data"]["supported_operations"]
            .as_array()
            .unwrap()
            .contains(&json!("runtime_start"))
    );
    let mut input = json!({"schema":gateway_management_api::SCHEMA,"target":"gateway","idempotency_key":"selected","command":{"kind":"configuration_select","candidate":"synthetic-candidate"},"subject":"root","role":"admin"});
    let response = service
        .router()
        .oneshot(request("POST", "/management/v1/preflight", input.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    input.as_object_mut().unwrap().remove("subject");
    input.as_object_mut().unwrap().remove("role");
    let response = service
        .router()
        .oneshot(request("POST", "/management/v1/preflight", input))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let submission = value["data"]["submission"].clone();
    let response = service
        .router()
        .oneshot(request("POST", "/management/v1/operations", submission))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let operation = value["data"]["operation_id"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while generation.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let response = service
        .router()
        .oneshot(request(
            "GET",
            &format!("/management/v1/operations/{operation}?target=gateway"),
            Value::Null,
        ))
        .await
        .unwrap();
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(
        value["data"]["operation"]["actor"]["subject"],
        "synthetic-host-operator"
    );
}
#[test]
fn generic_external_binding_requires_https_supported_paths_and_separate_keys() {
    let binding = ExternalAccess {
        schema: EXTERNAL_SCHEMA.into(),
        target: id("gateway"),
        public_base: "https://access.example.invalid/gateway/v1".into(),
        paths: BTreeSet::from([ClientPath::Models, ClientPath::Responses]),
        external_credential: id("access-key"),
        gateway_credential: id("gateway-key"),
    };
    binding.validate().unwrap();
    assert_eq!(
        binding.endpoint(ClientPath::Responses).unwrap(),
        "https://access.example.invalid/gateway/v1/responses"
    );
    assert_eq!(ClientPath::Responses.method(), "POST");
    assert!(
        binding
            .validate_credentials(&"a".repeat(40), &"b".repeat(40))
            .is_ok()
    );
    assert!(
        binding
            .validate_credentials(&"a".repeat(40), &"a".repeat(40))
            .is_err()
    );
    for base in [
        "http://access.example.invalid/v1",
        "https://user:pass@access.example.invalid/v1",
        "https://access.example.invalid/v1?token=synthetic",
        "https://access.example.invalid/v2",
    ] {
        let mut invalid = binding.clone();
        invalid.public_base = base.into();
        assert!(invalid.validate().is_err());
    }
    let mut read_only = binding.clone();
    read_only.paths = BTreeSet::from([ClientPath::Models]);
    assert!(matches!(
        read_only.endpoint(ClientPath::Responses),
        Err(Error::Unsupported)
    ));
    assert!(serde_json::from_value::<ClientPath>(json!("websocket")).is_err());
    let mut version = binding;
    version.schema = "gateway-external-access/v2".into();
    assert!(matches!(version.validate(), Err(Error::UnsupportedSchema)));
}
#[test]
fn expected_runtime_binds_current_manifest_readiness_versions_and_exact_digests() {
    let config=agent_response_gateway::Config::parse("listen='127.0.0.1:0'\n[providers.mock]\nbase_url='http://127.0.0.1:43200/v1'\napi_key_env='SYNTHETIC'\n[models.writer]\nprovider='mock'\nupstream_model='synthetic'\n").unwrap();
    let manifest = config.manifest().unwrap();
    let base = serde_json::to_value(&manifest).unwrap();
    let ready = manifest
        .readiness("127.0.0.1:43199".parse().unwrap(), None)
        .unwrap();
    let expected = ExpectedRuntime::from_manifest(&base).unwrap();
    expected.confirm(&ready).unwrap();
    let frame = json!({"schema":"gateway-managed-process/v1","instance_id":"owned-instance","gateway":ready});
    expected
        .confirm_managed(&id("owned-instance"), &frame)
        .unwrap();
    assert!(
        expected
            .confirm_managed(&id("other-instance"), &frame)
            .is_err()
    );
    let mut unsupported = frame;
    unsupported["schema"] = json!("gateway-managed-process/v2");
    assert!(
        expected
            .confirm_managed(&id("owned-instance"), &unsupported)
            .is_err()
    );
    for (key, value) in [
        ("schema", json!("gateway-ready/v9")),
        ("configuration_sha256", json!("a".repeat(64))),
        ("base_url", json!("https://foreign.example.invalid/v1")),
        ("address", json!("0.0.0.0:43199")),
    ] {
        let mut changed = ready.clone();
        changed[key] = value;
        assert!(expected.confirm(&changed).is_err());
    }
    let mut invalid = base.clone();
    invalid["schema"] = json!("gateway-embedded-manifest/v2");
    assert!(matches!(
        ExpectedRuntime::from_manifest(&invalid),
        Err(Error::UnsupportedSchema)
    ));
    // Synthetic projections test supported version matching; they are not configuration admission.
    for version in [1, 2, 3, 4, 5, 6, 7] {
        let mut embedded = base.clone();
        if version > 2 {
            embedded["schema"] = json!(format!("gateway-embedded-manifest/v{version}"));
        }
        let configuration = json!({"gateway":embedded,"extensions":{}});
        let digest = Digest::of(&serde_json::to_vec(&configuration).unwrap());
        let extended = json!({"schema":format!("gateway-extended-manifest/v{version}"),"configuration":configuration,"execution_sha256":digest});
        let expected = ExpectedRuntime::from_manifest(&extended).unwrap();
        let mut observed = ready.clone();
        observed["schema"] = json!(format!("gateway-extended-ready/v{version}"));
        observed["manifest_schema"] = extended["schema"].clone();
        observed["execution_sha256"] = json!(digest);
        expected.confirm(&observed).unwrap();
        observed["execution_sha256"] = json!("b".repeat(64));
        assert!(expected.confirm(&observed).is_err());
    }
}
