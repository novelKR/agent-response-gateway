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
    assert_eq!(
        s.connection
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        s.connection
            .query_row("SELECT COUNT(*) FROM usage_tombstones", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(s.record(&first, &c).unwrap(), ReceiptStatus::Duplicate);
    assert_eq!(
        s.connection
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
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

#[test]
fn contradictory_terminal_revision_cannot_leave_a_newer_unfinished_snapshot() {
    let d = directory();
    let mut s = Store::open(d.path(), true, true).unwrap();
    let c = config();
    s.record(&event(5, EventKind::UsageUpdated), &c).unwrap();
    assert_eq!(
        s.record(&event(3, EventKind::AttemptFinished), &c).unwrap(),
        ReceiptStatus::Conflict
    );
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_gateway-usage-recorder"))
        .args(["query", "--store"])
        .arg(d.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    let row: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(row["non_read_input_tokens"].as_u64(), Some(u64::MAX - 4));
}

#[test]
fn recorder_waits_for_export_writer_before_reading_ledger() {
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    // Synchronize on SQLite contention rather than a timing window or a production
    // hook. A deferred transaction cannot invoke the busy handler when upgrading
    // its read snapshot while another connection owns the write reservation.
    static CONTENDED: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
    fn busy(attempt: i32) -> bool {
        *CONTENDED.0.lock().unwrap() = true;
        CONTENDED.1.notify_one();
        std::thread::sleep(Duration::from_millis(1));
        attempt < 1000
    }

    let d = directory();
    let mut store = Store::open(d.path(), true, true).unwrap();
    store.connection.busy_handler(Some(busy)).unwrap();
    let mut exporter = rusqlite::Connection::open(d.path().join("usage.sqlite3")).unwrap();
    let tx = exporter
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    tx.execute("UPDATE usage_outbox SET attempts=attempts+1", [])
        .unwrap();
    std::thread::scope(|scope| {
        let recording =
            scope.spawn(|| store.record(&event(1, EventKind::AttemptFinished), &config()));
        let (guard, _) = CONTENDED
            .1
            .wait_timeout_while(CONTENDED.0.lock().unwrap(), Duration::from_secs(2), |v| !*v)
            .unwrap();
        let observed = *guard;
        drop(guard);
        tx.commit().unwrap();
        let result = recording.join().unwrap();
        assert!(
            observed,
            "recording must wait before opening a read snapshot"
        );
        assert_eq!(result.unwrap(), ReceiptStatus::Committed);
    });
    assert_eq!(
        store
            .record(&event(1, EventKind::AttemptFinished), &config())
            .unwrap(),
        ReceiptStatus::Duplicate
    );
}

fn provider_event(revision: u64, kind: EventKind) -> UsageEventV2 {
    let mut value = serde_json::to_value(event(revision, kind)).unwrap();
    value.as_object_mut().unwrap().remove("profile");
    value["schema"] = json!(SCHEMA_V2);
    value["attempt_id"] = json!("provider-attempt");
    value["event_id"] = json!(format!("provider-{revision}"));
    value["interpretation"] = json!({"kind":"trusted_provider_plugin","protocol":"gateway-provider/v1","provider_protocol":"synthetic/v1","package_id":"synthetic","package_version":"1.0.0","package_sha256":"a".repeat(64),"executable_sha256":"b".repeat(64)});
    value["usage"] = serde_json::to_value(CanonicalUsage::default()).unwrap();
    serde_json::from_value(value).unwrap()
}
#[test]
fn explicit_v2_guard_keeps_existing_bytes_identity_and_backup() {
    let directory = directory();
    let mut store = Store::open(directory.path(), true, true).unwrap();
    let legacy = event(0, EventKind::AttemptStarted);
    store.record(&legacy, &config()).unwrap();
    let before:(String,String,String)=store.connection.query_row("SELECT e.payload,e.sha256,a.identity FROM usage_events e JOIN usage_attempts a USING(producer_id,attempt_id)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    let provider = provider_event(0, EventKind::AttemptStarted);
    assert!(store.record(&provider, &config()).is_err());
    let backup = directory.path().join("before-v2.sqlite3");
    store.upgrade_v2(&backup).unwrap();
    assert_eq!(store.storage_version().unwrap(), 2);
    let saved = rusqlite::Connection::open(&backup).unwrap();
    assert_eq!(
        saved
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let after:(String,String,String)=store.connection.query_row("SELECT e.payload,e.sha256,a.identity FROM usage_events e JOIN usage_attempts a USING(producer_id,attempt_id)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(before, after);
    assert_eq!(before.0.as_bytes(), legacy.bytes().unwrap());
    assert_eq!(
        before.2,
        serde_json::to_string(&(
            &legacy.request_id,
            legacy.started_at_ms,
            &legacy.provider,
            &legacy.model_alias,
            &legacy.upstream_model,
            legacy.profile,
            &legacy.configuration_sha256
        ))
        .unwrap()
    );
    let recorded: RecordedEvent = serde_json::from_slice(&legacy.bytes().unwrap()).unwrap();
    assert_eq!(recorded.bytes().unwrap(), legacy.bytes().unwrap());
    let mut invalid = serde_json::to_value(&provider).unwrap();
    invalid["profile"] = json!(Profile::ResponsesV1);
    assert!(serde_json::from_value::<RecordedEvent>(invalid).is_err());
    assert_eq!(
        store.record(&provider, &config()).unwrap(),
        ReceiptStatus::Committed
    );
    assert_eq!(
        store.record(&provider, &config()).unwrap(),
        ReceiptStatus::Duplicate
    );
    let mut changed = provider_event(1, EventKind::UsageUpdated);
    changed.interpretation.package_sha256 = "c".repeat(64);
    assert_eq!(
        store.record(&changed, &config()).unwrap(),
        ReceiptStatus::Conflict
    );
    drop(store);
    assert!(Store::open(directory.path(), false, true).is_err());
    let reopened = Store::open_v2(directory.path(), false, true).unwrap();
    let groups = query::aggregate(&reopened, 0, 10, "UTC").unwrap();
    assert_eq!(groups["usage_contract"], SCHEMA_V2);
    assert_eq!(groups["groups"].as_array().unwrap().len(), 2);
    assert!(
        groups["groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g.get("interpretation").is_some())
    );
}
#[test]
fn unsupported_v2_export_is_blocked_before_transport_without_discarding_payload() {
    let directory = directory();
    let mut store = Store::open_v2(directory.path(), true, true).unwrap();
    let mut c = config();
    c.destinations.push(Destination::Http {
        id: "legacy".into(),
        url: "http://127.0.0.1:1/not-called".into(),
        bearer_file: "missing-unread-secret".into(),
    });
    let provider = provider_event(1, EventKind::AttemptFinished);
    store.record(&provider, &c).unwrap();
    assert_eq!(
        gateway_usage_recorder::export::run_once(&mut store, &c).unwrap(),
        0
    );
    let (state, payload): (String, String) = store
        .connection
        .query_row(
            "SELECT state,payload FROM usage_outbox JOIN usage_events USING(producer_id,event_id)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "blocked_unsupported_contract");
    assert_eq!(payload.as_bytes(), provider.bytes().unwrap());
}

#[test]
fn explicit_http_v2_export_preserves_mixed_event_bytes_and_exact_receipts() {
    use std::io::{BufRead, Read, Write};
    for corrupt_ack in [false, true] {
        let directory = directory();
        let mut store = Store::open_v2(directory.path(), true, true).unwrap();
        let secret = directory.path().join("export-secret");
        std::fs::write(&secret, b"synthetic-export-key").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut c = config();
        c.destinations.push(Destination::HttpV2 {
            id: "receiver".into(),
            url: format!("http://{address}/events"),
            bearer_file: secret.to_string_lossy().into(),
        });
        let legacy = event(1, EventKind::AttemptFinished);
        let provider = provider_event(1, EventKind::AttemptFinished);
        store.record(&legacy, &c).unwrap();
        store.record(&provider, &c).unwrap();
        let worker = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(socket);
            let mut length = 0;
            let mut line = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse::<usize>().unwrap();
                }
            }
            assert!(length > 0 && length < 1_048_576);
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let batch: BatchV2 = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(batch.schema, "gateway-usage-batch/v2");
            assert_eq!(batch.events.len(), 2);
            assert!(batch.events.iter().any(RecordedEvent::is_v2));
            assert!(batch.events.iter().any(|e| !e.is_v2()));
            let receipts: Vec<_> = batch
                .events
                .iter()
                .map(|e| Receipt {
                    producer_id: e.view().producer_id.clone(),
                    event_id: e.view().event_id.clone(),
                    sha256: if corrupt_ack {
                        "0".repeat(64)
                    } else {
                        digest(&e.bytes().unwrap())
                    },
                    status: ReceiptStatus::Committed,
                })
                .collect();
            let body = serde_json::to_vec(&BatchReceipt {
                schema: "gateway-usage-batch-receipt/v2".into(),
                receipts,
            })
            .unwrap();
            let mut socket = reader.into_inner();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            socket.write_all(&body).unwrap();
        });
        assert_eq!(
            gateway_usage_recorder::export::run_once(&mut store, &c).unwrap(),
            if corrupt_ack { 0 } else { 2 }
        );
        worker.join().unwrap();
        let states: Vec<String> = store
            .connection
            .prepare("SELECT state FROM usage_outbox")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            states,
            vec![if corrupt_ack { "blocked" } else { "committed" }; 2]
        );
        let payload: String = store
            .connection
            .query_row(
                "SELECT payload FROM usage_events WHERE event_id=?1",
                [&provider.event_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(payload.as_bytes(), provider.bytes().unwrap());
    }
}

#[test]
fn fixed_serve_v2_roundtrips_v1_and_v2_with_original_ack_digests() {
    use std::io::Write;
    let directory = directory();
    let store = Store::open_v2(directory.path(), true, true).unwrap();
    let producer = store.producer().unwrap();
    drop(store);
    let config_path = directory.path().join("recorder.json");
    std::fs::write(&config_path, serde_json::to_vec(&config()).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut legacy = event(1, EventKind::AttemptFinished);
    legacy.producer_id = producer.clone();
    let mut provider = provider_event(1, EventKind::AttemptFinished);
    provider.producer_id = producer.clone();
    let events = [RecordedEvent::V1(legacy), RecordedEvent::V2(provider)];
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_gateway-usage-recorder"))
        .arg("serve-v2")
        .current_dir(directory.path())
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for event in &events {
        stdin.write_all(&event.bytes().unwrap()).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let frames: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(frames.len(), 3);
    assert_eq!(
        frames[0],
        json!({"type":"ready","protocol":PROTOCOL_V2,"producer_id":producer,"capabilities":recorder_v2_capabilities()})
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(
            frames[index + 1],
            json!({"type":"committed","event_id":event.view().event_id,"sha256":digest(&event.bytes().unwrap())})
        );
    }
    let old = std::process::Command::new(env!("CARGO_BIN_EXE_gateway-usage-recorder"))
        .arg("serve")
        .current_dir(directory.path())
        .env_clear()
        .output()
        .unwrap();
    assert!(!old.status.success());
    assert!(old.stdout.is_empty());
}

#[test]
fn export_rejects_changed_or_noncanonical_stored_bytes_before_credentials_and_network() {
    for provider in [false, true] {
        for corruption in ["payload", "hash", "noncanonical"] {
            let directory = directory();
            let mut store = Store::open_v2(directory.path(), true, true).unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut config = config();
            config.destinations.push(Destination::HttpV2 {
                id: "collector".into(),
                url: format!("http://{}/events", listener.local_addr().unwrap()),
                // A credential read would fail differently, before reaching the listener.
                bearer_file: directory
                    .path()
                    .join("missing-unread-credential")
                    .to_string_lossy()
                    .into(),
            });
            let original: RecordedEvent = if provider {
                provider_event(1, EventKind::AttemptFinished).into()
            } else {
                event(1, EventKind::AttemptFinished).into()
            };
            store.record(&original, &config).unwrap();
            let bytes = original.bytes().unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let (payload, sha) = match corruption {
                "payload" => {
                    if provider {
                        value["interpretation"]["package_sha256"] = json!("c".repeat(64));
                    } else {
                        value["upstream_model"] = json!("changed-model");
                    }
                    (serde_json::to_string(&value).unwrap(), digest(&bytes))
                }
                "hash" => (String::from_utf8(bytes.clone()).unwrap(), "0".repeat(64)),
                _ => {
                    let formatted = serde_json::to_string_pretty(&value).unwrap();
                    let sha = digest(formatted.as_bytes());
                    (formatted, sha)
                }
            };
            store
                .connection
                .execute(
                    "UPDATE usage_events SET payload=?1,sha256=?2",
                    rusqlite::params![payload, sha],
                )
                .unwrap();
            let snapshot = |store: &Store| -> (String, String, String, i64, i64) {
                store.connection.query_row("SELECT payload,sha256,state,attempts,next_at_ms FROM usage_events JOIN usage_outbox USING(producer_id,event_id)", [],
                    |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap()
            };
            let before = snapshot(&store);
            assert_eq!(
                query::aggregate(&store, 0, 10, "UTC")
                    .unwrap_err()
                    .to_string(),
                "stored_usage_integrity"
            );
            for command in ["query", "export"] {
                let result =
                    std::process::Command::new(env!("CARGO_BIN_EXE_gateway-usage-recorder"))
                        .arg(command)
                        .arg("--store")
                        .arg(directory.path())
                        .env_clear()
                        .output()
                        .unwrap();
                assert!(!result.status.success());
                assert!(result.stdout.is_empty());
                assert_eq!(
                    String::from_utf8_lossy(&result.stderr).trim(),
                    "Usage recorder operation failed; check configuration, storage and delivery status"
                );
            }
            let failure =
                gateway_usage_recorder::export::run_once(&mut store, &config).unwrap_err();
            assert_eq!(
                failure.to_string(),
                "stored_usage_integrity",
                "{provider}:{corruption}"
            );
            assert!(
                matches!(listener.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
            );
            assert_eq!(snapshot(&store), before);
            assert_eq!(before.0, payload);
            assert_eq!(before.1, sha);
            assert_eq!(before.2, "pending");
            assert_eq!((before.3, before.4), (0, 0));
        }
    }
}

#[test]
fn live_v2_duplicate_cannot_ack_a_corrupted_committed_payload() {
    let directory = directory();
    let mut store = Store::open_v2(directory.path(), true, true).unwrap();
    let original = provider_event(1, EventKind::AttemptFinished);
    assert_eq!(
        store.record(&original, &config()).unwrap(),
        ReceiptStatus::Committed
    );
    let mut altered = serde_json::to_value(&original).unwrap();
    altered["interpretation"]["package_sha256"] = json!("c".repeat(64));
    let payload = serde_json::to_string(&altered).unwrap();
    store
        .connection
        .execute("UPDATE usage_events SET payload=?1", [&payload])
        .unwrap();
    let failure = store.record(&original, &config()).unwrap_err();
    assert_eq!(failure.to_string(), "stored_usage_integrity");
    let unchanged: (String, String) = store
        .connection
        .query_row("SELECT payload,sha256 FROM usage_events", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(unchanged, (payload, digest(&original.bytes().unwrap())));
}
