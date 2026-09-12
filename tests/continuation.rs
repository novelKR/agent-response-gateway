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
        steps: vec![json!({"type":"thought","signature":"synthetic-private"})],
        output: vec![json!({"type":"message","content":[]})],
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
    store.finalize(&s.id, &id, "hash", "ciphertext").unwrap();
    assert!(store.finalize(&s.id, &id, "hash", "ciphertext").is_err());
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
    store.finalize(&s.id, &id, "hash", "ciphertext").unwrap();
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
