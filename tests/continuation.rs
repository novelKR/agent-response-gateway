use agent_response_gateway::continuation::*;
use serde_json::json;
use std::path::Path;

fn private() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    t
}
fn origin() -> Origin {
    Origin {
        route: json!({"model":"synthetic","protocol":"v1"}),
        realm: "test".into(),
        generation: "1".into(),
    }
}
fn replay(s: &Session, id: String) -> Replay {
    Replay {
        schema: SCHEMA.into(),
        session: s.id.clone(),
        epoch: s.epoch,
        origin: s.origin.clone(),
        response: id,
        parent: s.head.clone(),
        input_len: 0,
        input_sha256: digest(&json!([])).unwrap(),
        provider_status: "completed".into(),
        steps: vec![json!({"type":"thought","signature":"synthetic-private"})],
        output: vec![json!({"type":"message","content":[]})],
    }
}
fn legacy_v2(value: Replay) -> ReplayV2 {
    ReplayV2 {
        schema: REPLAY_V2.into(),
        session: value.session,
        epoch: value.epoch,
        origin: value.origin,
        response: value.response,
        parent: value.parent,
        input_len: value.input_len,
        input_sha256: value.input_sha256,
        outcome: if value.provider_status == "requires_action" {
            Outcome::AwaitingTools
        } else {
            Outcome::Completed
        },
        native: NativeReplay::Gemini {
            version: 1,
            steps: value.steps,
        },
        output: value.output,
    }
}
fn open(path: &Path, init: bool) -> SqliteStore {
    SqliteStore::open(&path.canonicalize().unwrap(), init, 16 * 1024 * 1024).unwrap()
}

#[test]
fn encrypted_replay_authenticates_key_and_all_payload_bytes() {
    let key = Protector::new("key1".into(), &[1; 32]).unwrap();
    let t = private();
    let mut store = open(t.path(), true);
    let s = store.create(&origin()).unwrap();
    let r = replay(&s, "resp_test".into());
    let token = key.seal(&r).unwrap();
    assert!(!token.contains("synthetic-private"));
    assert_eq!(
        digest(&key.open(&token).unwrap()).unwrap(),
        digest(&r).unwrap()
    );
    assert_ne!(key.seal(&r).unwrap(), token);
    assert!(
        Protector::new("key1".into(), &[2; 32])
            .unwrap()
            .open(&token)
            .is_err()
    );
    assert!(
        Protector::new("key2".into(), &[1; 32])
            .unwrap()
            .open(&token)
            .is_err()
    );
    let mut changed = token.into_bytes();
    let i = changed.len() - 1;
    changed[i] = if changed[i] == b'0' { b'1' } else { b'0' };
    assert!(key.open(std::str::from_utf8(&changed).unwrap()).is_err());
}

fn store_contract(store: &mut dyn ContinuationStore) {
    let s = store.create(&origin()).unwrap();
    let id = store
        .begin(&s.id, s.revision, None, "input1", 1024)
        .unwrap();
    assert!(
        store
            .begin(&s.id, s.revision, None, "input1", 1024)
            .is_err()
    );
    assert!(store.record(&id).is_err());
    store
        .finalize(&s.id, &id, "hash", "ciphertext", false)
        .unwrap();
    assert!(
        store
            .finalize(&s.id, &id, "hash", "ciphertext", false)
            .is_err()
    );
    let next = store.session(&s.id).unwrap();
    assert_eq!(next.head.as_deref(), Some(id.as_str()));
    assert!(
        store
            .begin(&s.id, next.revision, None, "input2", 1024)
            .is_err()
    );
    assert!(
        store
            .begin(&s.id, next.revision, Some(&id), "input1", 1024)
            .is_err()
    );
    let second = store
        .begin(&s.id, next.revision, Some(&id), "input2", 1024)
        .unwrap();
    store.uncertain(&s.id, &second).unwrap();
    let unknown = store.session(&s.id).unwrap();
    assert_eq!(unknown.status, "unknown");
    assert!(
        store
            .begin(&s.id, unknown.revision, Some(&id), "input3", 1024)
            .is_err()
    );
    let restored = store
        .transition(
            &s.id,
            unknown.revision,
            "recover",
            Some("portable-digest"),
            "host-decision",
        )
        .unwrap();
    assert_eq!(restored.epoch, 2);
    assert!(restored.head.is_none());
    assert!(
        store
            .transition(
                &s.id,
                unknown.revision,
                "recover",
                Some("portable"),
                "host-decision"
            )
            .is_err()
    );
}
#[test]
fn sqlite_satisfies_backend_independent_transaction_contract() {
    let t = private();
    store_contract(&mut open(t.path(), true));
}

#[test]
fn restart_preserves_finalized_and_marks_pending_unknown() {
    let t = private();
    let id;
    {
        let mut store = open(t.path(), true);
        let s = store.create(&origin()).unwrap();
        id = s.id.clone();
        store.begin(&id, s.revision, None, "input", 1024).unwrap();
        assert!(
            SqliteStore::open(&t.path().canonicalize().unwrap(), false, 16 * 1024 * 1024).is_err()
        );
    }
    let mut store = open(t.path(), false);
    assert_eq!(store.session(&id).unwrap().status, "unknown");
    assert!(SqliteStore::open(&t.path().canonicalize().unwrap(), true, 16 * 1024 * 1024).is_err());
    let missing = private();
    assert!(
        SqliteStore::open(
            &missing.path().canonicalize().unwrap(),
            false,
            16 * 1024 * 1024
        )
        .is_err()
    );
}
#[tokio::test]
async fn hybrid_repairs_only_a_finalized_record_with_matching_origin() {
    let t = private();
    let mut store = open(t.path(), true);
    let s = store.create(&origin()).unwrap();
    let id = store
        .begin(&s.id, s.revision, None, "input", 1024 * 1024)
        .unwrap();
    let r = replay(&s, id.clone());
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("key".into(), &[1; 32]).unwrap(),
    );
    let token = runtime.finalize(r.clone()).await.unwrap();
    drop(runtime);
    // Simulate payload loss while preserving the authoritative finalized attempt.
    let db = rusqlite::Connection::open(t.path().join("continuation.sqlite3")).unwrap();
    db.execute("UPDATE records SET envelope=NULL", []).unwrap();
    drop(db);
    let store = open(t.path(), false);
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("key".into(), &[1; 32]).unwrap(),
    );
    assert_eq!(
        runtime
            .restore(s.clone(), token.clone())
            .await
            .unwrap()
            .steps,
        r.steps
    );
    let query = id.clone();
    assert!(
        runtime
            .access(move |s, _| Ok(s.record(&query)?.envelope.is_some()))
            .await
            .unwrap()
    );
    let mut foreign = s.clone();
    foreign.origin.generation = "2".into();
    assert!(runtime.restore(foreign, token.clone()).await.is_err());
    drop(runtime);
    let db = rusqlite::Connection::open(t.path().join("continuation.sqlite3")).unwrap();
    db.execute("DELETE FROM records", []).unwrap();
    drop(db);
    let runtime = Runtime::new(
        Box::new(open(t.path(), false)),
        Protector::new("key".into(), &[1; 32]).unwrap(),
    );
    assert!(runtime.restore(s, token).await.is_err());
}
#[test]
fn capacity_is_reserved_before_dispatch_and_never_evicts_attempts() {
    let t = private();
    let mut store =
        SqliteStore::open(&t.path().canonicalize().unwrap(), true, 1024 * 1024).unwrap();
    let s = store.create(&origin()).unwrap();
    assert!(
        store
            .begin(&s.id, s.revision, None, "input", 1024 * 1024 + 1)
            .is_err()
    );
    assert_eq!(store.session(&s.id).unwrap().status, "ready");
}
#[test]
fn compaction_requires_explicit_begin_finalization_and_commit() {
    let t = private();
    let mut store = open(t.path(), true);
    let s = store.create(&origin()).unwrap();
    assert!(
        store
            .transition(&s.id, s.revision, "compact_commit", Some("x"), "decision")
            .is_err()
    );
    let s = store
        .transition(&s.id, s.revision, "compact_begin", None, "decision")
        .unwrap();
    let id = store
        .begin(&s.id, s.revision, None, "compact-input", 1024)
        .unwrap();
    store
        .finalize(&s.id, &id, "hash", "ciphertext", false)
        .unwrap();
    let s = store.session(&s.id).unwrap();
    assert_eq!(s.status, "awaiting_compaction");
    assert!(
        store
            .begin(&s.id, s.revision, Some(&id), "new", 1024)
            .is_err()
    );
    let s = store
        .transition(
            &s.id,
            s.revision,
            "compact_commit",
            Some("portable"),
            "decision",
        )
        .unwrap();
    assert_eq!(s.epoch, 2);
    assert_eq!(s.status, "ready");
}

#[test]
fn key_binding_and_pending_tool_transitions_are_explicit() {
    let t = private();
    let mut store = open(t.path(), true);
    store.bind_protection("synthetic-key-binding").unwrap();
    store.bind_protection("synthetic-key-binding").unwrap();
    assert!(store.bind_protection("other-key-binding").is_err());
    let s = store.create(&origin()).unwrap();
    let attempt = store.begin(&s.id, s.revision, None, "input", 1024).unwrap();
    store
        .finalize(&s.id, &attempt, "hash", "ciphertext", true)
        .unwrap();
    let s = store.session(&s.id).unwrap();
    assert!(s.pending_tools);
    assert!(
        store
            .transition(&s.id, s.revision, "compact_begin", None, "host-decision")
            .is_err()
    );
}

#[tokio::test]
async fn publication_limit_rejection_does_not_finalize_execution() {
    let t = private();
    let mut store = open(t.path(), true);
    let s = store.create(&origin()).unwrap();
    let id = store.begin(&s.id, s.revision, None, "input", 8192).unwrap();
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("key1".into(), &[1; 32]).unwrap(),
    );
    assert!(
        runtime
            .finalize_checked(replay(&s, id.clone()), |_| Err(Error(
                "test publication limit"
            )))
            .await
            .is_err()
    );
    runtime
        .access(move |store, _| {
            assert!(store.record(&id).is_err());
            assert_eq!(store.session(&s.id)?.status, "pending");
            Ok(())
        })
        .await
        .unwrap();
}

#[test]
fn replay_versions_authenticate_layout_and_version_before_normalization() {
    let t = private();
    let mut store = open(t.path(), true);
    let s = store.create(&origin()).unwrap();
    let old = replay(&s, "response_legacy".into());
    let key = Protector::new("testkey".into(), &[7; 32]).unwrap();
    let v1 = key.seal(&old).unwrap();
    let record = key.open_record(&v1).unwrap();
    assert_eq!(digest(&record).unwrap(), digest(&old).unwrap());
    let normalized = legacy_v2(old);
    assert_eq!(normalized.schema, REPLAY_V2);
    let v2 = key.seal_record(&normalized.clone().into()).unwrap();
    assert!(v2.starts_with(ENVELOPE_V2));
    assert!(key.open(&v2).is_err());
    assert!(
        key.open_record(&v2.replacen(ENVELOPE_V2, ENVELOPE_PREFIX, 1))
            .is_err()
    );
    assert!(
        key.open_record(&v1.replacen(ENVELOPE_PREFIX, ENVELOPE_V2, 1))
            .is_err()
    );
    assert_eq!(
        digest(&key.open_record(&v2).unwrap()).unwrap(),
        digest(&normalized).unwrap()
    );
    let mut invalid = normalized;
    invalid.native = NativeReplay::Gemini {
        version: 2,
        steps: vec![json!({"type":"thought"})],
    };
    assert!(key.seal_record(&invalid.into()).is_err());
}

#[tokio::test]
async fn finalized_v2_public_reasoning_and_native_state_repair_together() {
    let t = private();
    let mut store = open(t.path(), true);
    let s = store.create(&origin()).unwrap();
    let id = store
        .begin(&s.id, s.revision, None, "request", 4096)
        .unwrap();
    let mut v2 = legacy_v2(replay(&s, id.clone()));
    v2.output = vec![public_reasoning(
        "reasoning_test",
        "synthetic public reasoning",
    )];
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("testkey".into(), &[7; 32]).unwrap(),
    );
    let token = runtime.finalize(v2.clone()).await.unwrap();
    drop(runtime);
    let db = rusqlite::Connection::open(t.path().join("continuation.sqlite3")).unwrap();
    db.execute("UPDATE records SET envelope=NULL", []).unwrap();
    drop(db);
    let runtime = Runtime::new(
        Box::new(open(t.path(), false)),
        Protector::new("testkey".into(), &[7; 32]).unwrap(),
    );
    let saved = runtime
        .restore_record(s.clone(), token.clone())
        .await
        .unwrap();
    assert_eq!(digest(&saved).unwrap(), digest(&v2).unwrap());
    let mut edited = v2;
    edited.output[0]["summary"][0]["text"] = json!("changed display");
    let forged = Protector::new("testkey".into(), &[7; 32])
        .unwrap()
        .seal_record(&edited.into())
        .unwrap();
    assert!(runtime.restore_record(s, forged).await.is_err());
}

fn provider_origin() -> Origin {
    Origin {
        route: json!({"api":"plugin","model":"synthetic","provider_plugin":{
        "protocol":"gateway-provider/v1","provider_protocol":"synthetic-provider/v1","id":"synthetic-provider","version":"1.0.0",
        "package_sha256":"a".repeat(64),"executable_sha256":"b".repeat(64)}}),
        realm: "test".into(),
        generation: "1".into(),
    }
}
fn provider_replay(session: &Session, response: String, bytes: &[u8]) -> ReplayV3 {
    use base64::Engine;
    ReplayV3 {
        schema: REPLAY_V3.into(),
        session: session.id.clone(),
        epoch: session.epoch,
        origin: session.origin.clone(),
        response,
        parent: session.head.clone(),
        input_len: 0,
        input_sha256: digest(&json!([])).unwrap(),
        outcome: Outcome::Completed,
        native: ProviderReplayWire {
            binding: ProviderBindingWire {
                protocol: "gateway-provider/v1".into(),
                provider_protocol: "synthetic-provider/v1".into(),
                id: "synthetic-provider".into(),
                version: "1.0.0".into(),
                package_sha256: "a".repeat(64),
                executable_sha256: "b".repeat(64),
            },
            format: "synthetic-counter".into(),
            version: 1,
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
        output: vec![],
    }
}
#[test]
fn provider_record_has_explicit_version_canonical_binary_and_size_bounds() {
    let temp = private();
    let mut store = open(temp.path(), true);
    let session = store.create(&provider_origin()).unwrap();
    let replay = provider_replay(&session, "resp_provider".into(), &[0, 255, 1, 128, 0]);
    let protector = Protector::new("key1".into(), &[1; 32]).unwrap();
    let token = protector.seal_record(&replay.clone().into()).unwrap();
    assert!(token.starts_with(ENVELOPE_V3));
    assert!(protector.open(&token).is_err());
    let original = protector.open_record(&token).unwrap();
    assert_eq!(digest(&original).unwrap(), digest(&replay).unwrap());
    for prefix in [ENVELOPE_PREFIX, ENVELOPE_V2, "arg-continuation-v4."] {
        assert!(
            protector
                .open_record(&token.replacen(ENVELOPE_V3, prefix, 1))
                .is_err()
        );
    }
    let mut corrupt = token.into_bytes();
    let n = corrupt.len() - 1;
    corrupt[n] = if corrupt[n] == b'0' { b'1' } else { b'0' };
    assert!(
        protector
            .open_record(std::str::from_utf8(&corrupt).unwrap())
            .is_err()
    );
    for data in ["AA", "AA===", "AB==", "AA==\n", "_w==", "===="] {
        let mut invalid = replay.clone();
        invalid.native.data_base64 = data.into();
        assert!(protector.seal_record(&invalid.into()).is_err());
    }
    let mut invalid = replay.clone();
    invalid.native.version = 0;
    assert!(protector.seal_record(&invalid.into()).is_err());
    let mut invalid = replay.clone();
    invalid.native.binding.package_sha256 = "c".repeat(64);
    assert!(protector.seal_record(&invalid.into()).is_err());
    let limit = provider_replay(&session, "resp_limit".into(), &vec![0; 1024 * 1024]);
    assert!(protector.seal_record(&limit.clone().into()).is_ok());
    assert!(
        protector
            .seal_record(
                &provider_replay(&session, "resp_large".into(), &vec![0; 1024 * 1024 + 1]).into()
            )
            .is_err()
    );
    let mut too_large = limit;
    too_large.output = vec![json!({"text":"x".repeat(1024*1024)})];
    assert!(protector.seal_record(&too_large.into()).is_err());
    let mut old_wire = serde_json::to_value(&replay).unwrap();
    old_wire["schema"] = json!(REPLAY_V2);
    assert!(serde_json::from_value::<ReplayV2>(old_wire).is_err());
    assert!(
        serde_json::from_value::<NativeReplay>(
            json!({"format":"provider","version":1,"data_base64":"AA=="})
        )
        .is_err()
    );
}
#[tokio::test]
async fn provider_checkpoint_restart_repair_and_origin_pinning_preserve_legacy_rows() {
    let temp = private();
    let mut store = open(temp.path(), true);
    let legacy = store.create(&origin()).unwrap();
    let legacy_id = store
        .begin(&legacy.id, legacy.revision, None, "old", 8192)
        .unwrap();
    let session = store.create(&provider_origin()).unwrap();
    let id = store
        .begin(&session.id, session.revision, None, "new", 8192)
        .unwrap();
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("key1".into(), &[1; 32]).unwrap(),
    );
    let old = legacy_v2(replay(&legacy, legacy_id.clone()));
    let old_token = runtime.finalize(old.clone()).await.unwrap();
    let first = provider_replay(&session, id.clone(), &[0, 255, 17]);
    let token = runtime.finalize(first.clone()).await.unwrap();
    drop(runtime);
    let db = rusqlite::Connection::open(temp.path().join("continuation.sqlite3")).unwrap();
    let prior: (String, String) = db
        .query_row(
            "SELECT digest,envelope FROM records WHERE id=?1",
            [&legacy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    db.execute("UPDATE records SET envelope=NULL WHERE id=?1", [&id])
        .unwrap();
    drop(db);
    let mut store = open(temp.path(), false);
    let head = store.session(&session.id).unwrap();
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("key1".into(), &[1; 32]).unwrap(),
    );
    let restored = runtime
        .restore_record(head.clone(), token.clone())
        .await
        .unwrap();
    assert_eq!(digest(&restored).unwrap(), digest(&first).unwrap());
    assert!(runtime.restore(head.clone(), token.clone()).await.is_err());
    for field in [
        "realm",
        "generation",
        "epoch",
        "revision",
        "pending_tools",
        "package",
        "protocol",
    ] {
        let mut wrong = head.clone();
        match field {
            "realm" => wrong.origin.realm = "other".into(),
            "generation" => wrong.origin.generation = "2".into(),
            "epoch" => wrong.epoch += 1,
            "revision" => wrong.revision -= 1,
            "pending_tools" => wrong.pending_tools = !wrong.pending_tools,
            "package" => {
                wrong.origin.route["provider_plugin"]["package_sha256"] = json!("c".repeat(64))
            }
            _ => wrong.origin.route["provider_plugin"]["provider_protocol"] = json!("other/v1"),
        };
        assert!(runtime.restore_record(wrong, token.clone()).await.is_err());
    }
    let s = head.clone();
    let attempt = runtime
        .access(move |store, _| store.begin(&s.id, s.revision, s.head.as_deref(), "next", 8192))
        .await
        .unwrap();
    let next = provider_replay(&head, attempt.clone(), &[1, 2, 3]);
    for kind in ["format", "version", "origin", "parent", "package"] {
        let mut wrong = next.clone();
        match kind {
            "format" => wrong.native.format = "changed".into(),
            "version" => wrong.native.version = 2,
            "origin" => wrong.origin.realm = "other".into(),
            "parent" => wrong.parent = None,
            _ => {
                wrong.native.binding.package_sha256 = "c".repeat(64);
                wrong.origin.route["provider_plugin"]["package_sha256"] = json!("c".repeat(64));
            }
        };
        assert!(runtime.finalize(wrong).await.is_err());
    }
    let next_token = runtime.finalize(next.clone()).await.unwrap();
    let sid = head.id.clone();
    let latest = runtime
        .access(move |store, _| store.session(&sid))
        .await
        .unwrap();
    assert!(
        runtime
            .restore_record(latest.clone(), next_token)
            .await
            .is_ok()
    );
    let sid = latest.id.clone();
    let rev = head.revision;
    assert!(
        runtime
            .access(move |store, _| store.begin(&sid, rev, Some(&attempt), "stale", 8192))
            .await
            .is_err()
    );
    drop(runtime);
    let db = rusqlite::Connection::open(temp.path().join("continuation.sqlite3")).unwrap();
    let after: (String, String) = db
        .query_row(
            "SELECT digest,envelope FROM records WHERE id=?1",
            [&legacy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(prior, after);
    assert_eq!(after.0, digest(&old).unwrap());
    assert_eq!(after.1, old_token);
}
#[tokio::test]
async fn provider_failed_publication_and_reservation_leave_unfinalized_attempts() {
    let temp = private();
    let mut store = open(temp.path(), true);
    let session = store.create(&provider_origin()).unwrap();
    let id = store
        .begin(&session.id, session.revision, None, "input", 64)
        .unwrap();
    let runtime = Runtime::new(
        Box::new(store),
        Protector::new("key1".into(), &[1; 32]).unwrap(),
    );
    let replay = provider_replay(&session, id.clone(), &[0]);
    assert!(
        runtime
            .finalize_checked(replay.clone(), |_| Err(Error(
                "synthetic publication failure"
            )))
            .await
            .is_err()
    );
    assert!(runtime.finalize(replay).await.is_err());
    let sid = session.id.clone();
    runtime
        .access(move |store, _| {
            assert!(store.record(&id).is_err());
            assert_eq!(store.session(&sid)?.status, "pending");
            store.uncertain(&sid, &id)?;
            Ok(())
        })
        .await
        .unwrap();
}
