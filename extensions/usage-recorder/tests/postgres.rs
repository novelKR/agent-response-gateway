//! Run explicitly against a disposable TLS PostgreSQL instance. Never a default live call.
use gateway_usage_contract::*;
use gateway_usage_recorder::{Store, export};
use serde_json::json;
#[test]
#[ignore = "requires disposable TLS PostgreSQL and explicit private connection/CA files"]
fn postgres_outbox_commits_and_replays_without_duplicate_events() {
    let connection_file =
        std::env::var("USAGE_TEST_PG_CONNECTION_FILE").expect("explicit test DSN file");
    let tls_ca_file = Some(std::env::var("USAGE_TEST_PG_CA_FILE").expect("explicit test CA file"));
    let d = Destination::Postgres {
        id: "postgres-test".into(),
        connection_file,
        tls_ca_file,
    };
    export::initialize_postgres(&d).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/usage-tests");
    std::fs::create_dir_all(&root).unwrap();
    let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut s = Store::open(temp.path(), true, true).unwrap();
    let c = RecorderConfig {
        schema: "gateway-usage-recorder-config/v1".into(),
        destinations: vec![d],
    };
    s.bind_destinations(&c).unwrap();
    let e = UsageEvent {
        schema: SCHEMA.into(),
        producer_id: "producer".into(),
        request_id: "request".into(),
        attempt_id: "attempt".into(),
        event_id: "event".into(),
        revision: 1,
        kind: EventKind::AttemptFinished,
        started_at_ms: 0,
        observed_at_ms: 1,
        provider: "synthetic".into(),
        model_alias: "model".into(),
        upstream_model: "model".into(),
        reported_model: None,
        provider_request_id: None,
        provider_response_id: None,
        profile: Profile::ResponsesV1,
        configuration_sha256: "a".repeat(64),
        upstream: Outcome::Completed,
        gateway: Outcome::Completed,
        finality: Finality::Final,
        observation_incomplete: false,
        usage: normalize(
            Profile::ResponsesV1,
            extract(
                Profile::ResponsesV1,
                &json!({"input_tokens":u64::MAX,"output_tokens":0}),
            ),
        ),
    };
    s.record(&e, &c).unwrap();
    assert_eq!(export::run_once(&mut s, &c).unwrap(), 1);
    s.connection
        .execute("UPDATE usage_outbox SET state='pending',next_at_ms=0", [])
        .unwrap();
    assert_eq!(export::run_once(&mut s, &c).unwrap(), 1);
}
