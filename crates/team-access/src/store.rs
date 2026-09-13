use crate::*;
use gateway_management::{
    Backend, Digest, Effect, Error, FailureCode, Id, Journal, Operation, PreparedOperation,
    Request, Result, Snapshot, State, filesystem,
};
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path, time::Duration};
use zeroize::Zeroizing;
pub(crate) const DATABASE: &str = "team.sqlite3";
pub(crate) fn storage(_: rusqlite::Error) -> Error {
    Error::Storage
}
pub(crate) fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::InvalidInput)
}
pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    if value.len() > 65536 {
        return Err(Error::InvalidStore);
    }
    serde_json::from_str(value).map_err(|_| Error::InvalidStore)
}
fn valid_limit(maximum: u64) -> Result<()> {
    if !(1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&maximum) {
        return Err(Error::InvalidInput);
    }
    Ok(())
}
fn configure(db: &Connection, maximum: u64) -> Result<()> {
    valid_limit(maximum)?;
    db.busy_timeout(Duration::from_secs(1)).map_err(storage)?;
    let mode: String = db
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(storage)?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Storage);
    }
    db.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF;")
        .map_err(storage)?;
    let page: i64 = db
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .map_err(storage)?;
    db.pragma_update(
        None,
        "max_page_count",
        (maximum / u64::try_from(page).map_err(|_| Error::InvalidStore)?) as i64,
    )
    .map_err(storage)?;
    Ok(())
}
pub(crate) fn open_db(path: &Path, target: &Id, writable: bool) -> Result<Connection> {
    filesystem::directory(path)?;
    filesystem::regular(&path.join(DATABASE))?;
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let db =
        Connection::open_with_flags(path.join(DATABASE), flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(storage)?;
    db.busy_timeout(Duration::from_secs(1)).map_err(storage)?;
    let (schema, registered): (String, String) = db
        .query_row(
            "SELECT schema,target FROM metadata WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| Error::InvalidStore)?;
    if schema != STORE_SCHEMA {
        return Err(Error::UnsupportedSchema);
    }
    if registered != target.as_str() {
        return Err(Error::InvalidStore);
    }
    Ok(db)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredCredential {
    pub metadata: Credential,
    pub verifier: Digest,
    pub request_sha256: Digest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema: String,
    operation: Id,
    request_sha256: Digest,
    before: Snapshot,
    after: Snapshot,
    issued_credential_sha256: Option<Digest>,
}
fn subjects(db: &Connection) -> Result<Vec<Subject>> {
    let mut q = db
        .prepare("SELECT data FROM subjects ORDER BY id LIMIT 129")
        .map_err(storage)?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage)?;
    let result: Vec<Subject> = rows
        .map(|r| decode(&r.map_err(storage)?))
        .collect::<Result<_>>()?;
    if result.len() > 128 {
        return Err(Error::InvalidStore);
    }
    for value in &result {
        value
            .permissions
            .validate()
            .map_err(|_| Error::InvalidStore)?;
    }
    Ok(result)
}
fn credentials(db: &Connection) -> Result<Vec<StoredCredential>> {
    let mut q = db
        .prepare("SELECT data FROM credentials ORDER BY id LIMIT 1025")
        .map_err(storage)?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(storage)?;
    let result: Vec<_> = rows
        .map(|r| decode(&r.map_err(storage)?))
        .collect::<Result<_>>()?;
    if result.len() > 1024 {
        return Err(Error::InvalidStore);
    }
    Ok(result)
}
fn snapshot(db: &Connection) -> Result<Snapshot> {
    let revision: i64 = db
        .query_row(
            "SELECT generation FROM metadata WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let revision = u64::try_from(revision).map_err(|_| Error::InvalidStore)?;
    Ok(Snapshot {
        revision,
        digest: Digest::of(json(&(revision, subjects(db)?, credentials(db)?))?.as_bytes()),
    })
}
pub(crate) fn subject(db: &Connection, id: &Id) -> Result<Subject> {
    let text: Option<String> = db
        .query_row(
            "SELECT data FROM subjects WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let value: Subject = decode(&text.ok_or(Error::NotFound)?)?;
    if value.id != *id {
        return Err(Error::InvalidStore);
    }
    value.permissions.validate()?;
    Ok(value)
}
pub(crate) fn credential(db: &Connection, id: &Id) -> Result<StoredCredential> {
    let text: Option<String> = db
        .query_row(
            "SELECT data FROM credentials WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let value: StoredCredential = decode(&text.ok_or(Error::NotFound)?)?;
    if value.metadata.id != *id {
        return Err(Error::InvalidStore);
    }
    Ok(value)
}
fn validate(db: &Connection, command: &Command) -> Result<()> {
    match command {
        Command::Register { subject: id, .. } => {
            match subject(db, id) {
                Ok(_) => return Err(Error::Conflict),
                Err(Error::NotFound) => {}
                Err(error) => return Err(error),
            }
            if subjects(db)?.len() >= 128 {
                return Err(Error::Conflict);
            }
        }
        Command::PermissionsChange { subject: id, .. } => {
            subject(db, id)?;
        }
        Command::Issue {
            subject: id,
            credential: key,
            purpose,
        } => {
            let owner = subject(db, id)?;
            if !owner.permissions.enabled
                || *purpose == Purpose::Model && owner.permissions.routes.is_empty()
            {
                return Err(Error::Forbidden);
            }
            absent(db, key)?;
            if credentials(db)?.len() >= 1024 {
                return Err(Error::Conflict);
            }
        }
        Command::Revoke { credential: key } => {
            if credential(db, key)?.metadata.revoked {
                return Err(Error::Conflict);
            }
        }
        Command::Rotate {
            credential: key,
            replacement,
        } => {
            let old = credential(db, key)?;
            let owner = subject(db, &old.metadata.subject)?;
            if old.metadata.revoked || !owner.permissions.enabled {
                return Err(Error::Conflict);
            }
            absent(db, replacement)?;
            if credentials(db)?.len() >= 1024 {
                return Err(Error::Conflict);
            }
        }
    }
    Ok(())
}
fn absent(db: &Connection, id: &Id) -> Result<()> {
    match credential(db, id) {
        Err(Error::NotFound) => Ok(()),
        Ok(_) => Err(Error::Conflict),
        Err(error) => Err(error),
    }
}
fn evidence(db: &Connection, operation: &Operation) -> Result<Option<(Evidence, Digest)>> {
    let text: Option<String> = db
        .query_row(
            "SELECT evidence FROM receipts WHERE operation=?1",
            [operation.id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let Some(text) = text else {
        return Ok(None);
    };
    let value: Evidence = decode(&text)?;
    if value.schema != "gateway-team-effect/v1"
        || value.operation != operation.id
        || value.request_sha256 != operation.request.fingerprint()?
        || value.before != operation.request.expected
        || value.before.revision.checked_add(1) != Some(value.after.revision)
    {
        return Err(Error::InvalidStore);
    }
    Ok(Some((value, Digest::of(text.as_bytes()))))
}
pub(crate) fn credential_digest(value: &StoredCredential) -> Result<Digest> {
    Ok(Digest::of(json(value)?.as_bytes()))
}
pub(crate) fn verify_issuance(
    db: &Connection,
    operation: &Operation,
    key: &StoredCredential,
) -> Result<()> {
    let (value, digest) = evidence(db, operation)?.ok_or(Error::InvalidStore)?;
    if value.issued_credential_sha256.as_ref() != Some(&credential_digest(key)?)
        || !matches!(operation.events.last().and_then(|e|e.effect.as_ref()),Some(Effect::Applied{after,evidence_sha256}) if after == &value.after && evidence_sha256 == &digest)
    {
        return Err(Error::InvalidStore);
    }
    Ok(())
}
/// The owner lease and a SQLite IMMEDIATE transaction prevent concurrent stale mutations.
/// Host-supplied actors are trusted only at the authentication adapter boundary.
pub struct Manager {
    db: Connection,
    target: Id,
    _owner: File,
}
impl Manager {
    pub fn initialize(path: &Path, target: Id, maximum_bytes: u64) -> Result<Self> {
        valid_limit(maximum_bytes)?;
        filesystem::directory(path)?;
        let owner = filesystem::lease(&path.join("owner.lock"))?;
        let file = filesystem::private_new(&path.join(DATABASE))?;
        file.sync_all().map_err(|_| Error::Storage)?;
        drop(file);
        let mut db = Connection::open_with_flags(
            path.join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage)?;
        configure(&db, maximum_bytes)?;
        let tx = db.transaction().map_err(storage)?;
        tx.execute_batch("CREATE TABLE metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema TEXT NOT NULL,target TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>=0));
CREATE TABLE subjects(id TEXT PRIMARY KEY,data TEXT NOT NULL);
CREATE TABLE credentials(id TEXT PRIMARY KEY,verifier TEXT NOT NULL UNIQUE,data TEXT NOT NULL);
CREATE TABLE receipts(operation TEXT PRIMARY KEY,evidence TEXT NOT NULL);
CREATE TRIGGER receipts_no_update BEFORE UPDATE ON receipts BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER receipts_no_delete BEFORE DELETE ON receipts BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER subjects_no_delete BEFORE DELETE ON subjects BEGIN SELECT RAISE(ABORT,'retained'); END;
CREATE TRIGGER credentials_no_delete BEFORE DELETE ON credentials BEGIN SELECT RAISE(ABORT,'retained'); END;").map_err(storage)?;
        tx.execute(
            "INSERT INTO metadata VALUES(1,?1,?2,0)",
            params![STORE_SCHEMA, target.as_str()],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(Self {
            db,
            target,
            _owner: owner,
        })
    }
    pub fn open(path: &Path, target: Id, maximum_bytes: u64) -> Result<Self> {
        valid_limit(maximum_bytes)?;
        filesystem::directory(path)?;
        let owner = filesystem::lease(&path.join("owner.lock"))?;
        let db = open_db(path, &target, true)?;
        configure(&db, maximum_bytes)?;
        snapshot(&db)?;
        Ok(Self {
            db,
            target,
            _owner: owner,
        })
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let tx = self.db.unchecked_transaction().map_err(storage)?;
        snapshot(&tx)
    }
    pub fn inventory(&self) -> Result<Inventory> {
        let tx = self.db.unchecked_transaction().map_err(storage)?;
        Ok(Inventory {
            schema: SCHEMA,
            target: self.target.clone(),
            snapshot: snapshot(&tx)?,
            subjects: subjects(&tx)?,
            credentials: credentials(&tx)?.into_iter().map(|c| c.metadata).collect(),
        })
    }
    pub fn execute(
        &mut self,
        journal: &mut Journal,
        actor: &gateway_management::Actor,
        request: &Request,
        command: &Command,
    ) -> Result<Completion> {
        if request.target != self.target
            || request.action != command.action()
            || request.parameters_sha256 != command.digest()?
        {
            return Err(Error::InvalidInput);
        }
        let mut secret = None;
        let operation = journal.execute(
            actor,
            request,
            &mut Bound {
                manager: self,
                command,
                secret: &mut secret,
            },
        )?;
        if operation.state != State::Succeeded {
            secret = None;
        }
        Ok(Completion { operation, secret })
    }
    pub fn reconcile(&mut self, operation: &Operation) -> Result<Effect> {
        if operation.request.target != self.target {
            return Err(Error::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let Some((value, digest)) = evidence(&tx, operation)? else {
            return Ok(Effect::Uncertain {
                code: FailureCode::Unverified,
            });
        };
        Ok(Effect::Applied {
            after: value.after,
            evidence_sha256: digest,
        })
    }
    /// Coherent explicit backup; does not overwrite a file or copy SQLite/WAL files individually.
    pub fn backup(&self, destination: &Path) -> Result<()> {
        filesystem::directory(destination.parent().ok_or(Error::InvalidStore)?)?;
        let file = filesystem::private_new(destination)?;
        file.sync_all().map_err(|_| Error::Storage)?;
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
        std::fs::OpenOptions::new()
            .write(true)
            .open(destination)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Storage)
    }
}
struct Bound<'a> {
    manager: &'a mut Manager,
    command: &'a Command,
    secret: &'a mut Option<Secret>,
}
impl Backend for Bound<'_> {
    fn prepare<'a>(&'a mut self, request: &'a Request) -> Result<Box<dyn PreparedOperation + 'a>> {
        let tx = self
            .manager
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        validate(&tx, self.command)?;
        let before = snapshot(&tx)?;
        Ok(Box::new(Prepared {
            tx: Some(tx),
            command: self.command,
            before,
            request_sha256: request.fingerprint()?,
            operation: None,
            secret: self.secret,
        }))
    }
    fn reconcile(&mut self, operation: &Operation) -> Result<Effect> {
        self.manager.reconcile(operation)
    }
}
struct Prepared<'a> {
    tx: Option<Transaction<'a>>,
    command: &'a Command,
    before: Snapshot,
    request_sha256: Digest,
    operation: Option<Id>,
    secret: &'a mut Option<Secret>,
}
fn revoke(db: &Connection, key: &Id) -> Result<()> {
    let mut value = credential(db, key)?;
    value.metadata.revoked = true;
    db.execute(
        "UPDATE credentials SET data=?2 WHERE id=?1",
        params![key.as_str(), json(&value)?],
    )
    .map_err(storage)?;
    Ok(())
}
fn issue(
    db: &Connection,
    owner: Id,
    key: Id,
    purpose: Purpose,
    operation: &Id,
    fingerprint: &Digest,
) -> Result<Secret> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    SystemRandom::new()
        .fill(bytes.as_mut())
        .map_err(|_| Error::Storage)?;
    let mut raw = Zeroizing::new(purpose.prefix().to_owned());
    for byte in bytes.iter() {
        use std::fmt::Write;
        write!(&mut *raw, "{byte:02x}").map_err(|_| Error::Storage)?;
    }
    let verifier = Digest::of(raw.as_bytes());
    let value = StoredCredential {
        metadata: Credential {
            id: key.clone(),
            subject: owner,
            purpose,
            revoked: false,
            issued_operation: operation.clone(),
        },
        verifier: verifier.clone(),
        request_sha256: fingerprint.clone(),
    };
    db.execute(
        "INSERT INTO credentials VALUES(?1,?2,?3)",
        params![key.as_str(), verifier.as_str(), json(&value)?],
    )
    .map_err(storage)?;
    Ok(Secret {
        credential: key,
        value: raw,
    })
}
impl PreparedOperation for Prepared<'_> {
    fn before(&self) -> &Snapshot {
        &self.before
    }
    fn accepted(&mut self, operation: &Id) {
        self.operation = Some(operation.clone());
    }
    fn apply(&mut self) -> Effect {
        let Some(tx) = self.tx.take() else {
            return Effect::Uncertain {
                code: FailureCode::Unverified,
            };
        };
        let Some(operation) = self.operation.as_ref() else {
            return Effect::NotApplied {
                code: FailureCode::Rejected,
            };
        };
        let effect = (|| -> Result<(Snapshot, Digest, Option<Secret>)> {
            let secret = match self.command {
                Command::Register {
                    subject: id,
                    permissions,
                } => {
                    let value = Subject {
                        id: id.clone(),
                        revision: 1,
                        permissions: permissions.clone(),
                    };
                    tx.execute(
                        "INSERT INTO subjects VALUES(?1,?2)",
                        params![id.as_str(), json(&value)?],
                    )
                    .map_err(storage)?;
                    None
                }
                Command::PermissionsChange {
                    subject: id,
                    permissions,
                } => {
                    let mut value = subject(&tx, id)?;
                    value.revision = value.revision.checked_add(1).ok_or(Error::Conflict)?;
                    value.permissions = permissions.clone();
                    tx.execute(
                        "UPDATE subjects SET data=?2 WHERE id=?1",
                        params![id.as_str(), json(&value)?],
                    )
                    .map_err(storage)?;
                    None
                }
                Command::Issue {
                    subject,
                    credential,
                    purpose,
                } => Some(issue(
                    &tx,
                    subject.clone(),
                    credential.clone(),
                    *purpose,
                    operation,
                    &self.request_sha256,
                )?),
                Command::Revoke { credential } => {
                    revoke(&tx, credential)?;
                    None
                }
                Command::Rotate {
                    credential: key,
                    replacement,
                } => {
                    let old = credential(&tx, key)?;
                    revoke(&tx, key)?;
                    Some(issue(
                        &tx,
                        old.metadata.subject,
                        replacement.clone(),
                        old.metadata.purpose,
                        operation,
                        &self.request_sha256,
                    )?)
                }
            };
            let changed=tx.execute("UPDATE metadata SET generation=generation+1 WHERE singleton=1 AND generation<9223372036854775807",[]).map_err(storage)?;
            if changed != 1 {
                return Err(Error::Conflict);
            }
            let after = snapshot(&tx)?;
            let text = json(&Evidence {
                schema: "gateway-team-effect/v1".into(),
                operation: operation.clone(),
                request_sha256: self.request_sha256.clone(),
                before: self.before.clone(),
                after: after.clone(),
                issued_credential_sha256: secret
                    .as_ref()
                    .map(|secret| {
                        credential(&tx, secret.credential_id())
                            .and_then(|key| credential_digest(&key))
                    })
                    .transpose()?,
            })?;
            tx.execute(
                "INSERT INTO receipts VALUES(?1,?2)",
                params![operation.as_str(), text],
            )
            .map_err(storage)?;
            let digest = Digest::of(text.as_bytes());
            Ok((after, digest, secret))
        })();
        match effect {
            Err(_) => match tx.rollback() {
                Ok(()) => Effect::NotApplied {
                    code: FailureCode::EffectFailed,
                },
                Err(_) => Effect::Uncertain {
                    code: FailureCode::Unverified,
                },
            },
            Ok((after, evidence_sha256, secret)) => match tx.commit() {
                Ok(()) => {
                    *self.secret = secret;
                    Effect::Applied {
                        after,
                        evidence_sha256,
                    }
                }
                Err(_) => Effect::Uncertain {
                    code: FailureCode::Unverified,
                },
            },
        }
    }
}
