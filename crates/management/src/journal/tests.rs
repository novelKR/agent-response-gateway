use super::*;

#[test]
fn live_writer_can_reconcile_a_failed_final_commit_after_storage_recovers() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    journal.db.execute_batch("CREATE TRIGGER fail_finish BEFORE INSERT ON events WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let mut backend = Fixture::new();
    let Error::Uncertain(operation_id) = journal
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap_err()
    else {
        panic!("uncertainty must identify the operation")
    };
    journal
        .db
        .execute_batch("DROP TRIGGER fail_finish")
        .unwrap();
    let op = journal
        .reconcile(
            &actor("operator"),
            &id("instance"),
            &operation_id,
            &mut backend,
        )
        .unwrap();
    assert_eq!(op.state, State::Succeeded);
    assert!(op.events[2].actor.is_none());
    assert_eq!(op.events[2].phase, Phase::Recovery);
    assert_eq!(
        op.events[3].authorization.as_ref().unwrap().action,
        Action::Reconcile
    );
    assert_eq!(backend.effects.get(), 1);
}

#[test]
fn invalid_budget_does_not_create_a_store() {
    let d = private_directory();
    let path = root(&d);
    assert!(matches!(
        Journal::initialize(&path, 0),
        Err(Error::InvalidInput)
    ));
    assert!(!path.join(DATABASE).exists());
    assert!(!path.join("owner.lock").exists());
}

use std::{cell::Cell, rc::Rc};
const LIMIT: u64 = 8 * 1024 * 1024;

fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn snapshot(n: u64) -> Snapshot {
    Snapshot {
        revision: n,
        digest: Digest::of(&n.to_be_bytes()),
    }
}
fn actor(subject: &str) -> Actor {
    Actor::new(
        Identity {
            subject: id(subject),
            credential: id("synthetic-credential-id"),
        },
        [
            Action::RuntimeStart,
            Action::ReadOperations,
            Action::Reconcile,
        ]
        .into_iter()
        .map(|action| Grant {
            action,
            target: id("instance"),
        }),
    )
    .unwrap()
}
fn request() -> Request {
    Request {
        target: id("instance"),
        action: Action::RuntimeStart,
        expected: snapshot(0),
        idempotency_key: id("request-1"),
        parameters_sha256: Digest::of(b"synthetic-private-parameters"),
    }
}
fn private_directory() -> tempfile::TempDir {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/management-tests");
    std::fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory
}
fn root(d: &tempfile::TempDir) -> std::path::PathBuf {
    d.path().canonicalize().unwrap()
}

struct Fixture {
    before: Snapshot,
    effects: Rc<Cell<usize>>,
    effect: Effect,
}
impl Fixture {
    fn new() -> Self {
        Self {
            before: snapshot(0),
            effects: Rc::new(Cell::new(0)),
            effect: Effect::Applied {
                after: snapshot(1),
                evidence_sha256: Digest::of(b"proof"),
            },
        }
    }
}
struct Prepared<'a>(&'a mut Fixture);
impl PreparedOperation for Prepared<'_> {
    fn before(&self) -> &Snapshot {
        &self.0.before
    }
    fn apply(&mut self) -> Effect {
        self.0.effects.set(self.0.effects.get() + 1);
        self.0.effect.clone()
    }
}
impl Backend for Fixture {
    fn prepare<'a>(&'a mut self, _: &'a Request) -> Result<Box<dyn PreparedOperation + 'a>> {
        Ok(Box::new(Prepared(self)))
    }
    fn reconcile(&mut self, _: &Operation) -> Result<Effect> {
        Ok(self.effect.clone())
    }
}

#[test]
fn applies_once_and_records_authority_and_actual_evidence() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    let mut backend = Fixture::new();
    let op = journal
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap();
    assert_eq!(op.state, State::Succeeded);
    assert_eq!(
        op.events.iter().map(|e| e.phase).collect::<Vec<_>>(),
        vec![Phase::Accepted, Phase::Started, Phase::Finished]
    );
    assert_eq!(
        op.authorization,
        Grant {
            action: Action::RuntimeStart,
            target: id("instance")
        }
    );
    assert_eq!(backend.effects.get(), 1);
    assert_eq!(
        journal
            .execute(&actor("alice"), &request(), &mut backend)
            .unwrap(),
        op
    );
    assert_eq!(backend.effects.get(), 1);
    let text = json(&op).unwrap();
    assert!(!text.contains("synthetic-private-parameters"));
    assert!(!text.contains("grants"));
    let reader = Reader::open(&path).unwrap();
    assert_eq!(
        reader
            .get(&actor("alice"), &id("instance"), &op.id)
            .unwrap(),
        op
    );
}

#[test]
fn conflicting_key_and_revoked_grants_never_replay_or_disclose() {
    let d = private_directory();
    let mut journal = Journal::initialize(&root(&d), LIMIT).unwrap();
    let mut backend = Fixture::new();
    journal
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap();
    let mut changed = request();
    changed.parameters_sha256 = Digest::of(b"different");
    assert_eq!(
        journal.execute(&actor("alice"), &changed, &mut backend),
        Err(Error::Conflict)
    );
    let revoked = Actor::new(actor("alice").identity().clone(), []).unwrap();
    assert_eq!(
        journal.execute(&revoked, &request(), &mut backend),
        Err(Error::Forbidden)
    );
    assert_eq!(backend.effects.get(), 1);
    // Independent principals may reuse a client key.
    assert!(
        journal
            .execute(&actor("bob"), &request(), &mut backend)
            .is_ok()
    );
    assert_eq!(backend.effects.get(), 2);
}

#[test]
fn stale_snapshot_rejects_before_journal_or_effect() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    let mut backend = Fixture::new();
    backend.before = snapshot(1);
    assert_eq!(
        journal.execute(&actor("alice"), &request(), &mut backend),
        Err(Error::Conflict)
    );
    assert_eq!(backend.effects.get(), 0);
    assert!(
        Reader::open(&path)
            .unwrap()
            .list(&actor("alice"), &id("instance"), 0, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn journal_failure_prevents_effects() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    journal.db.pragma_update(None, "query_only", true).unwrap();
    let mut backend = Fixture::new();
    assert_eq!(
        journal.execute(&actor("alice"), &request(), &mut backend),
        Err(Error::Storage)
    );
    assert_eq!(backend.effects.get(), 0);
    assert!(
        Reader::open(&path)
            .unwrap()
            .list(&actor("alice"), &id("instance"), 0, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn interruption_before_started_is_proven_not_applied() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    journal.db.execute_batch("CREATE TRIGGER fail_start BEFORE INSERT ON events WHEN NEW.sequence=2 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let mut backend = Fixture::new();
    assert_eq!(
        journal.execute(&actor("alice"), &request(), &mut backend),
        Err(Error::Storage)
    );
    assert_eq!(backend.effects.get(), 0);
    journal.db.execute_batch("DROP TRIGGER fail_start").unwrap();
    drop(journal);
    let mut recovered = Journal::open(&path, LIMIT).unwrap();
    let op = recovered
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap();
    assert_eq!(op.state, State::Failed);
    assert_eq!(op.events.last().unwrap().phase, Phase::Recovery);
    assert_eq!(backend.effects.get(), 0);
}

#[test]
fn lost_final_record_is_uncertain_then_explicitly_reconciled_without_replay() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    journal.db.execute_batch("CREATE TRIGGER fail_finish BEFORE INSERT ON events WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    let mut backend = Fixture::new();
    let error = journal
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap_err();
    let Error::Uncertain(operation_id) = error else {
        panic!("expected uncertain outcome")
    };
    assert_eq!(backend.effects.get(), 1);
    journal
        .db
        .execute_batch("DROP TRIGGER fail_finish")
        .unwrap();
    drop(journal);
    let mut recovered = Journal::open(&path, LIMIT).unwrap();
    let op = recovered
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap();
    assert_eq!(op.state, State::Uncertain);
    assert_eq!(backend.effects.get(), 1);
    let reconciled = recovered
        .reconcile(
            &actor("operator"),
            &id("instance"),
            &operation_id,
            &mut backend,
        )
        .unwrap();
    assert_eq!(reconciled.state, State::Succeeded);
    assert_eq!(reconciled.actor.subject, id("alice"));
    assert_eq!(
        reconciled
            .events
            .last()
            .unwrap()
            .actor
            .as_ref()
            .unwrap()
            .subject,
        id("operator")
    );
    assert_eq!(reconciled.events[2].state, State::Uncertain);
    assert_eq!(reconciled.events[3].phase, Phase::Reconciled);
    assert_eq!(backend.effects.get(), 1);
    assert_eq!(
        recovered.reconcile(
            &actor("operator"),
            &id("instance"),
            &operation_id,
            &mut backend
        ),
        Err(Error::InvalidTransition)
    );
}

#[test]
fn explicit_uncertainty_survives_retries_and_restart() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    let mut backend = Fixture::new();
    backend.effect = Effect::Uncertain {
        code: FailureCode::Unverified,
    };
    let op = journal
        .execute(&actor("alice"), &request(), &mut backend)
        .unwrap();
    drop(journal);
    let mut recovered = Journal::open(&path, LIMIT).unwrap();
    assert_eq!(
        recovered
            .execute(&actor("alice"), &request(), &mut backend)
            .unwrap(),
        op
    );
    assert_eq!(backend.effects.get(), 1);
}

#[test]
fn writers_are_exclusive_readers_are_scoped_and_cursor_is_bounded() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    assert!(matches!(
        Journal::open(&path, LIMIT),
        Err(Error::AlreadyOwned)
    ));
    let reader = Reader::open(&path).unwrap();
    let op = journal
        .execute(&actor("alice"), &request(), &mut Fixture::new())
        .unwrap();
    let no_access = Actor::new(actor("alice").identity().clone(), []).unwrap();
    assert_eq!(
        reader.get(&no_access, &id("instance"), &op.id),
        Err(Error::Forbidden)
    );
    let rows = reader.list(&actor("alice"), &id("instance"), 0, 1).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        reader
            .list(&actor("alice"), &id("instance"), rows[0].0, 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        reader.list(&actor("alice"), &id("instance"), 0, 101),
        Err(Error::InvalidInput)
    );
}

#[test]
fn backup_includes_committed_evidence_without_overwrite() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    let op = journal
        .execute(&actor("alice"), &request(), &mut Fixture::new())
        .unwrap();
    let destination = path.join("backup.sqlite3");
    journal.backup(&destination).unwrap();
    let backup =
        Connection::open_with_flags(&destination, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(recorded(&backup, &op.id).unwrap(), op);
    assert!(journal.backup(&destination).is_err());
    assert_eq!(recorded(&backup, &op.id).unwrap(), op);
    regular(&destination).unwrap();
}

#[test]
fn schema_and_immutable_evidence_reject_changes() {
    let d = private_directory();
    let path = root(&d);
    let mut journal = Journal::initialize(&path, LIMIT).unwrap();
    let op = journal
        .execute(&actor("alice"), &request(), &mut Fixture::new())
        .unwrap();
    assert!(journal.db.execute("DELETE FROM events", []).is_err());
    assert!(
        journal
            .db
            .execute("UPDATE operations SET fingerprint='changed'", [])
            .is_err()
    );
    assert_eq!(recorded(&journal.db, &op.id).unwrap(), op);
    journal
        .db
        .execute("UPDATE metadata SET schema='unsupported'", [])
        .unwrap();
    drop(journal);
    assert!(matches!(
        Journal::open(&path, LIMIT),
        Err(Error::UnsupportedSchema)
    ));
}

#[test]
fn typed_contract_rejects_unsafe_ids_hashes_and_unknown_fields() {
    for value in ["", "../escape", "with space", "/absolute", "line\nbreak"] {
        assert!(Id::new(value).is_err());
    }
    assert!(Digest::try_from("a".repeat(63)).is_err());
    assert!(Digest::try_from("A".repeat(64)).is_err());
    let mut wire = serde_json::to_value(request()).unwrap();
    wire["role"] = serde_json::json!("operator");
    assert!(serde_json::from_value::<Request>(wire).is_err());
    let mut read_request = request();
    read_request.action = Action::ReadState;
    assert_eq!(read_request.validate(), Err(Error::InvalidInput));
    assert_eq!(
        actor("alice").capabilities(
            &id("instance"),
            &[Action::PackageInstall, Action::RuntimeStart]
        ),
        vec![Action::RuntimeStart]
    );
}

#[cfg(unix)]
#[test]
fn unsafe_store_links_permissions_and_hardlinks_reject() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = private_directory();
    let path = root(&d);
    let journal = Journal::initialize(&path, LIMIT).unwrap();
    drop(journal);
    let linked = path.join("alias");
    symlink(&path, &linked).unwrap();
    assert!(matches!(Reader::open(&linked), Err(Error::InvalidStore)));
    std::fs::hard_link(path.join(DATABASE), path.join("hardlink")).unwrap();
    assert!(matches!(
        Journal::open(&path, LIMIT),
        Err(Error::InvalidStore)
    ));
    std::fs::remove_file(path.join("hardlink")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(Reader::open(&path), Err(Error::InvalidStore)));
}
