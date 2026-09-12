use gateway_usage_contract::*;
use gateway_usage_recorder::{Store, query};
use serde_json::json;
fn directory() -> tempfile::TempDir {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/usage-tests");
    std::fs::create_dir_all(&root).unwrap();
    let t = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    t
}
fn config() -> RecorderConfig {
    RecorderConfig {
        schema: "gateway-usage-recorder-config/v1".into(),
        destinations: vec![],
    }
}
fn event(revision: u64, kind: EventKind) -> UsageEvent {
    UsageEvent {
        schema: SCHEMA.into(),
        producer_id: "producer".into(),
        request_id: "request".into(),
        attempt_id: "attempt".into(),
        event_id: format!("event-{revision}"),
        revision,
        kind,
        started_at_ms: 0,
        observed_at_ms: revision,
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
                &json!({"input_tokens":u64::MAX,"output_tokens":0,"input_tokens_details":{"cached_tokens":4}}),
            ),
        ),
    }
}
#[test]
fn duplicates_conflicts_and_delayed_revisions_do_not_double_count() {
    let d = directory();
    let mut s = Store::open(d.path(), true, true).unwrap();
    let c = config();
    let end = event(3, EventKind::AttemptFinished);
    assert_eq!(s.record(&end, &c).unwrap(), ReceiptStatus::Committed);
    assert_eq!(s.record(&end, &c).unwrap(), ReceiptStatus::Duplicate);
    let mut conflict = end.clone();
    conflict.event_id = "different".into();
    assert_eq!(s.record(&conflict, &c).unwrap(), ReceiptStatus::Conflict);
    assert_eq!(
        s.record(&event(1, EventKind::UsageUpdated), &c).unwrap(),
        ReceiptStatus::Committed
    );
    assert_eq!(
        s.record(&event(4, EventKind::UsageUpdated), &c).unwrap(),
        ReceiptStatus::Conflict
    );
    let revision: i64 = s
        .connection
        .query_row("SELECT revision FROM usage_current", [], |r| r.get(0))
        .unwrap();
    assert_eq!(revision, 3);
    let report = query::aggregate(&s, 0, 100, "Asia/Seoul").unwrap();
    assert_eq!(report["groups"][0]["calls"], 1);
    assert_eq!(
        report["groups"][0]["token_sums"]["input_tokens"].as_u64(),
        Some(u64::MAX)
    );
}
#[test]
fn restart_and_backup_preserve_identity_and_unknown_attempts() {
    let d = directory();
    let mut s = Store::open(d.path(), true, true).unwrap();
    let producer = s.producer().unwrap();
    let mut e = event(0, EventKind::AttemptStarted);
    e.finality = Finality::Unobserved;
    e.usage = CanonicalUsage::default();
    s.record(&e, &config()).unwrap();
    let backup = d.path().join("backup.sqlite3");
    s.backup(&backup).unwrap();
    drop(s);
    let s = Store::open(d.path(), false, true).unwrap();
    assert_eq!(s.producer().unwrap(), producer);
    assert_eq!(query::status(&s).unwrap()["unfinished_calls"], 1);
    assert!(Store::open(d.path(), false, true).is_err());
    drop(s);
    let c = rusqlite::Connection::open(d.path().join("usage.sqlite3")).unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
    drop(c);
    assert!(Store::open(d.path(), false, true).is_err());
}
#[test]
fn export_destination_change_cannot_redirect_existing_events() {
    let d = directory();
    let mut s = Store::open(d.path(), true, true).unwrap();
    let mut c = config();
    c.destinations.push(Destination::Http {
        id: "collector".into(),
        url: "https://collector.example/events".into(),
        bearer_file: "/synthetic/token".into(),
    });
    s.bind_destinations(&c).unwrap();
    s.record(&event(1, EventKind::UsageUpdated), &c).unwrap();
    c.destinations[0] = Destination::Http {
        id: "collector".into(),
        url: "https://other.example/events".into(),
        bearer_file: "/synthetic/token".into(),
    };
    assert!(s.bind_destinations(&c).is_err());
    assert!(s.bind_destinations(&config()).is_err());
    let n: i64 = s
        .connection
        .query_row("SELECT COUNT(*) FROM usage_outbox", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn retention_keeps_deduplication_tombstones_and_monotonic_export_cursors() {
    let d = directory();
    let mut s = Store::open(d.path(), true, true).unwrap();
    let c = config();
    let first = event(1, EventKind::AttemptFinished);
    s.record(&first, &c).unwrap();
    drop(s);
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_gateway-usage-recorder"))
        .args(["prune", "--store"])
        .arg(d.path())
        .args(["--before-ms", "100"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let mut s = Store::open(d.path(), false, true).unwrap();
    assert_eq!(s.record(&first, &c).unwrap(), ReceiptStatus::Duplicate);
    let old = event(0, EventKind::AttemptStarted);
    assert_eq!(s.record(&old, &c).unwrap(), ReceiptStatus::Conflict);
    let mut next = event(1, EventKind::AttemptFinished);
    next.attempt_id = "second-attempt".into();
    next.event_id = "second-event".into();
    s.record(&next, &c).unwrap();
    let cursor: i64 = s
        .connection
        .query_row("SELECT rowid FROM usage_events", [], |r| r.get(0))
        .unwrap();
    assert!(cursor > 1);
}
