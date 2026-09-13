use gateway_management::{Action, Grant, Id, Identity};
use gateway_management_api::{
    Authenticator, Command, CredentialKind, LocalAuthenticator, LocalCredential,
};
#[test]
fn wire_commands_reject_host_paths_roles_and_unsupported_removal() {
    for raw in [
        r#"{"kind":"runtime_start","executable":"untrusted"}"#,
        r#"{"kind":"runtime_stop","role":"owner"}"#,
        r#"{"kind":"package_install","family":"native","source":"registered","path":"untrusted"}"#,
        r#"{"kind":"package_remove","family":"native","package":"synthetic"}"#,
        r#"{"kind":"runtime_start","kind":"runtime_stop"}"#,
    ] {
        assert!(
            serde_json::from_str::<Command>(raw).is_err(),
            "wire command must reject ambiguous or unsupported fields"
        );
    }
}
#[test]
fn local_auth_uses_credentials_and_does_not_grant_mutation_to_read_keys() {
    let identity = Identity {
        subject: Id::new("operator").unwrap(),
        credential: Id::new("reader").unwrap(),
    };
    let token = "synthetic-read-credential-01234567890123456789";
    let auth = LocalAuthenticator::new(vec![LocalCredential {
        token: token.into(),
        identity: identity.clone(),
        kind: CredentialKind::ReadOnly,
        grants: vec![Grant {
            action: Action::ReadState,
            target: Id::new("gateway").unwrap(),
        }],
    }])
    .unwrap();
    assert!(auth.authenticate("untrusted").is_none());
    let principal = auth.authenticate(token).unwrap();
    assert_eq!(principal.kind, CredentialKind::ReadOnly);
    assert!(
        principal
            .actor
            .authorize(Action::RuntimeStart, &Id::new("gateway").unwrap())
            .is_err()
    );
    assert!(
        auth.refresh(&identity, &principal.authorization_version)
            .is_some()
    );
    let changed = Identity {
        subject: Id::new("other").unwrap(),
        ..identity
    };
    assert!(
        auth.refresh(&changed, &principal.authorization_version)
            .is_none()
    );
}

#[test]
fn module_views_preserve_unknown_and_reject_mislabelled_observations() {
    use gateway_management_api::{ModuleView, Observation, STATE_SCHEMA, StateView};
    let mut view = StateView {
        schema: STATE_SCHEMA.into(),
        modules: vec![ModuleView {
            id: Id::new("runtime").unwrap(),
            contract: "gateway-runtime-status/v1".into(),
            observation: Observation::Unobserved {
                reason: Id::new("host-not-connected").unwrap(),
            },
        }],
    };
    view.validate().unwrap();
    let value = serde_json::to_value(&view).unwrap();
    assert_eq!(value["modules"][0]["observation"]["state"], "unobserved");
    assert!(value["modules"][0]["observation"].get("data").is_none());
    view.modules[0].observation = Observation::Observed {
        observed_at_ms: 1,
        data: serde_json::json!({"schema":"wrong/v1"}),
    };
    assert!(view.validate().is_err());
}
