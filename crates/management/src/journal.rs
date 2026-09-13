use crate::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::{File, OpenOptions},
    path::{Component, Path},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DATABASE: &str = "management.sqlite3";
const MAX_EVENTS: u64 = 256;

/// One writer for the explicitly selected management store. Opening never starts effects.
pub struct Journal {
    db: Connection,
    _owner: File,
}
/// Read-only evidence access; cannot bypass actor grants or alter operation history.
pub struct Reader {
    db: Connection,
}

fn storage(_: rusqlite::Error) -> Error {
    Error::Storage
}
fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::InvalidInput)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    if value.len() > 65536 {
        return Err(Error::InvalidStore);
    }
    serde_json::from_str(value).map_err(|_| Error::InvalidStore)
}
fn now() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Storage)?
            .as_millis(),
    )
    .map_err(|_| Error::Storage)
}
fn directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|p| matches!(p, Component::ParentDir)) {
        return Err(Error::InvalidStore);
    }
    for ancestor in path.ancestors() {
        let meta = std::fs::symlink_metadata(ancestor).map_err(|_| Error::InvalidStore)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::InvalidStore);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(Error::InvalidStore);
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)
            .map_err(|_| Error::InvalidStore)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(Error::InvalidStore);
        }
    }
    Ok(())
}
fn regular(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| Error::InvalidStore)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::InvalidStore);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = std::fs::metadata(path.parent().ok_or(Error::InvalidStore)?)
            .map_err(|_| Error::InvalidStore)?;
        if meta.nlink() != 1 || meta.mode() & 0o077 != 0 || meta.uid() != parent.uid() {
            return Err(Error::InvalidStore);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::InvalidStore);
        }
    }
    Ok(())
}
fn private_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| Error::Storage)
}
fn owner(directory: &Path) -> Result<File> {
    let path = directory.join("owner.lock");
    match private_new(&path) {
        Ok(file) => drop(file),
        Err(_) if path.symlink_metadata().is_ok() => {}
        Err(error) => return Err(error),
    }
    regular(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|_| Error::Storage)?;
    file.try_lock().map_err(|_| Error::AlreadyOwned)?;
    Ok(file)
}
fn schema(db: &Connection) -> Result<()> {
    let value: String = db
        .query_row("SELECT schema FROM metadata WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .map_err(|_| Error::InvalidStore)?;
    if value != STORE_SCHEMA {
        return Err(Error::UnsupportedSchema);
    }
    Ok(())
}
fn open_db(directory_path: &Path, writable: bool) -> Result<Connection> {
    directory(directory_path)?;
    let path = directory_path.join(DATABASE);
    regular(&path)?;
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let db = Connection::open_with_flags(&path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(storage)?;
    db.busy_timeout(Duration::from_secs(1)).map_err(storage)?;
    schema(&db)?;
    Ok(db)
}
fn recorded(db: &Connection, id: &Id) -> Result<Operation> {
    let text: Option<String> = db
        .query_row(
            "SELECT intent FROM operations WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let mut operation: Operation = decode(&text.ok_or(Error::NotFound)?)?;
    if operation.id != *id
        || operation.schema != CONTRACT
        || !operation.events.is_empty()
        || operation.state != State::Queued
        || operation.authorization.action != operation.request.action
        || operation.authorization.target != operation.request.target
    {
        return Err(Error::InvalidStore);
    }
    let mut q = db
        .prepare("SELECT event FROM events WHERE operation_id=?1 ORDER BY sequence")
        .map_err(storage)?;
    for row in q
        .query_map([id.as_str()], |r| r.get::<_, String>(0))
        .map_err(storage)?
    {
        let event: Event = decode(&row.map_err(storage)?)?;
        if event.sequence != operation.events.len() as u64 + 1 || event.sequence > MAX_EVENTS {
            return Err(Error::InvalidStore);
        }
        match event.phase {
            Phase::Recovery if event.actor.is_none() && event.authorization.is_none() => {}
            Phase::Accepted
                if event.actor.as_ref() == Some(&operation.actor)
                    && event.authorization.as_ref() == Some(&operation.authorization) => {}
            Phase::Started | Phase::Finished
                if event.actor.as_ref() == Some(&operation.actor)
                    && event.authorization.is_none() => {}
            Phase::Reconciled
                if event.actor.is_some()
                    && event.authorization.as_ref()
                        == Some(&Grant {
                            action: Action::Reconcile,
                            target: operation.request.target.clone(),
                        }) => {}
            _ => return Err(Error::InvalidStore),
        }
        if operation.events.is_empty() {
            if event.phase != Phase::Accepted
                || event.state != State::Queued
                || event.effect.is_some()
            {
                return Err(Error::InvalidStore);
            }
        } else if !valid_next(
            operation.state,
            event.phase,
            event.state,
            event.effect.as_ref(),
        ) {
            return Err(Error::InvalidStore);
        }
        operation.state = event.state;
        operation.events.push(event);
    }
    if operation.events.is_empty() {
        return Err(Error::InvalidStore);
    }
    Ok(operation)
}
fn valid_next(before: State, phase: Phase, state: State, effect: Option<&Effect>) -> bool {
    match phase {
        Phase::Accepted => false,
        Phase::Started => before == State::Queued && state == State::Running && effect.is_none(),
        Phase::Finished => before == State::Running && effect.is_some_and(|e| state == e.state()),
        Phase::Recovery => {
            matches!(
                (before, effect),
                (
                    State::Queued,
                    Some(Effect::NotApplied {
                        code: FailureCode::Interrupted
                    })
                ) | (
                    State::Running,
                    Some(Effect::Uncertain {
                        code: FailureCode::Interrupted
                    })
                )
            ) && effect.is_some_and(|e| state == e.state())
        }
        Phase::Reconciled => {
            before == State::Uncertain && effect.is_some_and(|e| state == e.state())
        }
    }
}

impl Journal {
    /// Initialize only in an existing private directory; never replaces a store.
    pub fn initialize(path: &Path, maximum_bytes: u64) -> Result<Self> {
        valid_limit(maximum_bytes)?;
        directory(path)?;
        let owner = owner(path)?;
        let file = private_new(&path.join(DATABASE))?;
        file.sync_all().map_err(|_| Error::Storage)?;
        drop(file);
        let mut db = Connection::open_with_flags(
            path.join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage)?;
        configure(&db, maximum_bytes)?;
        let tx = db.transaction().map_err(storage)?;
        tx.execute_batch("
CREATE TABLE metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema TEXT NOT NULL);
CREATE TABLE operations(id TEXT PRIMARY KEY,subject TEXT NOT NULL,idempotency_key TEXT NOT NULL,fingerprint TEXT NOT NULL,intent TEXT NOT NULL,UNIQUE(subject,idempotency_key));
CREATE TABLE events(operation_id TEXT NOT NULL REFERENCES operations(id),sequence INTEGER NOT NULL,event TEXT NOT NULL,PRIMARY KEY(operation_id,sequence));
CREATE TRIGGER operations_no_update BEFORE UPDATE ON operations BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER operations_no_delete BEFORE DELETE ON operations BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER events_no_update BEFORE UPDATE ON events BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events BEGIN SELECT RAISE(ABORT,'immutable'); END;
").map_err(storage)?;
        tx.execute("INSERT INTO metadata VALUES(1,?1)", [STORE_SCHEMA])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(Self { db, _owner: owner })
    }
    pub fn open(path: &Path, maximum_bytes: u64) -> Result<Self> {
        valid_limit(maximum_bytes)?;
        directory(path)?;
        let owner = owner(path)?;
        let db = open_db(path, true)?;
        configure(&db, maximum_bytes)?;
        let mut this = Self { db, _owner: owner };
        this.recover()?;
        Ok(this)
    }
    fn recover(&mut self) -> Result<()> {
        let ids = {
            let mut q = self
                .db
                .prepare(
                    "SELECT o.id FROM operations o JOIN events e ON e.operation_id=o.id
WHERE e.sequence=(SELECT MAX(sequence) FROM events WHERE operation_id=o.id)
AND json_extract(e.event,'$.state') IN ('queued','running')",
                )
                .map_err(storage)?;
            q.query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?
        };
        for id in ids {
            let id = Id::new(id).map_err(|_| Error::InvalidStore)?;
            self.settle_interrupted(&id)?;
        }
        Ok(())
    }
    // Exclusive writer ownership and the synchronous executor establish that no
    // effect is executing through this Journal when this method is entered.
    fn settle_interrupted(&mut self, id: &Id) -> Result<Operation> {
        let operation = recorded(&self.db, id)?;
        let effect = match operation.state {
            State::Queued => Some(Effect::NotApplied {
                code: FailureCode::Interrupted,
            }),
            State::Running => Some(Effect::Uncertain {
                code: FailureCode::Interrupted,
            }),
            _ => None,
        };
        if let Some(effect) = effect {
            self.append(id, None, Phase::Recovery, effect.state(), Some(effect))?;
            recorded(&self.db, id)
        } else {
            Ok(operation)
        }
    }
    fn lookup(&self, actor: &Actor, request: &Request) -> Result<Option<Operation>> {
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT id,fingerprint FROM operations WHERE subject=?1 AND idempotency_key=?2",
                params![
                    actor.identity().subject.as_str(),
                    request.idempotency_key.as_str()
                ],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        match row {
            None => Ok(None),
            Some((id, digest)) => {
                if digest != request.fingerprint()?.as_str() {
                    return Err(Error::Conflict);
                }
                Ok(Some(recorded(
                    &self.db,
                    &Id::new(id).map_err(|_| Error::InvalidStore)?,
                )?))
            }
        }
    }
    fn accept(&mut self, actor: &Actor, request: &Request, grant: Grant) -> Result<Id> {
        let id = Id::new(uuid::Uuid::new_v4().to_string())?;
        let operation = Operation {
            schema: CONTRACT.into(),
            id: id.clone(),
            actor: actor.identity().clone(),
            authorization: grant,
            request: request.clone(),
            state: State::Queued,
            events: vec![],
        };
        let event = Event {
            sequence: 1,
            at_ms: now()?,
            phase: Phase::Accepted,
            state: State::Queued,
            actor: Some(actor.identity().clone()),
            authorization: Some(operation.authorization.clone()),
            effect: None,
        };
        let tx = self.db.transaction().map_err(storage)?;
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3,?4,?5)",
            params![
                id.as_str(),
                actor.identity().subject.as_str(),
                request.idempotency_key.as_str(),
                request.fingerprint()?.as_str(),
                json(&operation)?
            ],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO events VALUES(?1,1,?2)",
            params![id.as_str(), json(&event)?],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(id)
    }
    fn append(
        &mut self,
        id: &Id,
        actor: Option<Identity>,
        phase: Phase,
        state: State,
        effect: Option<Effect>,
    ) -> Result<()> {
        let before = recorded(&self.db, id)?;
        if !valid_next(before.state, phase, state, effect.as_ref()) {
            return Err(Error::InvalidTransition);
        }
        let sequence = before.events.len() as u64 + 1;
        if sequence > MAX_EVENTS {
            return Err(Error::Storage);
        }
        let event = Event {
            sequence,
            at_ms: now()?,
            phase,
            state,
            actor,
            authorization: (phase == Phase::Reconciled).then(|| Grant {
                action: Action::Reconcile,
                target: before.request.target.clone(),
            }),
            effect,
        };
        self.db
            .execute(
                "INSERT INTO events VALUES(?1,?2,?3)",
                params![id.as_str(), sequence as i64, json(&event)?],
            )
            .map_err(storage)?;
        Ok(())
    }
    /// Authorization is checked before duplicate lookup; a revoked caller cannot retrieve old results.
    /// The adapter's prepared guard spans durable intent and application.
    pub fn execute(
        &mut self,
        actor: &Actor,
        request: &Request,
        backend: &mut impl Backend,
    ) -> Result<Operation> {
        request.validate()?;
        let grant = actor.authorize(request.action, &request.target)?;
        if let Some(operation) = self.lookup(actor, request)? {
            return self.settle_interrupted(&operation.id);
        }
        let mut prepared = backend.prepare(request)?;
        if prepared.before() != &request.expected {
            return Err(Error::Conflict);
        }
        let id = self.accept(actor, request, grant)?;
        // No adapter effect may run if this commit fails.
        self.append(
            &id,
            Some(actor.identity().clone()),
            Phase::Started,
            State::Running,
            None,
        )?;
        let effect = prepared.apply();
        drop(prepared);
        self.append(
            &id,
            Some(actor.identity().clone()),
            Phase::Finished,
            effect.state(),
            Some(effect),
        )
        .map_err(|_| Error::Uncertain(id.clone()))?;
        recorded(&self.db, &id).map_err(|_| Error::Uncertain(id))
    }
    pub fn reconcile(
        &mut self,
        actor: &Actor,
        target: &Id,
        id: &Id,
        backend: &mut impl Backend,
    ) -> Result<Operation> {
        actor.authorize(Action::Reconcile, target)?;
        let operation = recorded(&self.db, id)?;
        if operation.request.target != *target {
            return Err(Error::NotFound);
        }
        let operation = self.settle_interrupted(id)?;
        if operation.state != State::Uncertain {
            return Err(Error::InvalidTransition);
        }
        let effect = backend.reconcile(&operation)?;
        self.append(
            id,
            Some(actor.identity().clone()),
            Phase::Reconciled,
            effect.state(),
            Some(effect),
        )?;
        recorded(&self.db, id)
    }
    /// Consistent SQLite backup, including committed WAL data; never overwrites a file.
    /// The destination is a private host path, not an untrusted HTTP parameter.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        directory(destination.parent().ok_or(Error::InvalidStore)?)?;
        let file = private_new(destination)?;
        drop(file);
        let mut output = Connection::open_with_flags(
            destination,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage)?;
        let backup = rusqlite::backup::Backup::new(&self.db, &mut output).map_err(storage)?;
        backup
            .run_to_completion(32, Duration::from_millis(10), None)
            .map_err(storage)?;
        drop(backup);
        output.close().map_err(|_| Error::Storage)?;
        // FlushFileBuffers requires GENERIC_WRITE on Windows.
        OpenOptions::new()
            .write(true)
            .open(destination)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Storage)
    }
}
fn valid_limit(maximum_bytes: u64) -> Result<()> {
    if !(1024 * 1024..=i64::MAX as u64).contains(&maximum_bytes) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn configure(db: &Connection, maximum_bytes: u64) -> Result<()> {
    valid_limit(maximum_bytes)?;
    db.busy_timeout(Duration::from_secs(1)).map_err(storage)?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .map_err(storage)?;
    let page_size: i64 = db
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .map_err(storage)?;
    let page_size = u64::try_from(page_size).map_err(|_| Error::InvalidStore)?;
    if page_size == 0 {
        return Err(Error::InvalidStore);
    }
    db.pragma_update(None, "max_page_count", (maximum_bytes / page_size) as i64)
        .map_err(storage)?;
    Ok(())
}
impl Reader {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            db: open_db(path, false)?,
        })
    }
    pub fn get(&self, actor: &Actor, target: &Id, id: &Id) -> Result<Operation> {
        actor.authorize(Action::ReadOperations, target)?;
        let operation = recorded(&self.db, id)?;
        if operation.request.target != *target {
            return Err(Error::NotFound);
        }
        Ok(operation)
    }
    /// The cursor is a store-local row ordinal, not a timestamp or global operation ID.
    pub fn list(
        &self,
        actor: &Actor,
        target: &Id,
        after: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Operation)>> {
        actor.authorize(Action::ReadOperations, target)?;
        if after > i64::MAX as u64 || limit == 0 || limit > 100 {
            return Err(Error::InvalidInput);
        }
        let mut q = self
            .db
            .prepare(
                "SELECT rowid,id FROM operations WHERE rowid>?1
AND json_extract(intent,'$.request.target')=?2 ORDER BY rowid LIMIT ?3",
            )
            .map_err(storage)?;
        let rows = q
            .query_map(params![after as i64, target.as_str(), limit as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(storage)?;
        rows.map(|r| {
            let (ordinal, id) = r.map_err(storage)?;
            Ok((
                u64::try_from(ordinal).map_err(|_| Error::InvalidStore)?,
                recorded(&self.db, &Id::new(id).map_err(|_| Error::InvalidStore)?)?,
            ))
        })
        .collect()
    }
}

#[cfg(test)]
mod tests;
