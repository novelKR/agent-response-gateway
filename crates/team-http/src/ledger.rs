use crate::contract::*;
use gateway_management::{Digest, Error, Id, Result, filesystem};
use gateway_team_access::Principal;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{fs::File, path::Path, time::Duration};
const DATABASE: &str = "team-requests.sqlite3";
const MAX_REQUESTS: i64 = 1_000_000;
const MAX_SESSIONS: i64 = 16384;
fn storage(_: rusqlite::Error) -> Error {
    Error::Storage
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::InvalidInput)
}
fn decode<T: DeserializeOwned>(value: &str) -> Result<T> {
    if value.len() > 65536 {
        return Err(Error::InvalidStore);
    }
    serde_json::from_str(value).map_err(|_| Error::InvalidStore)
}
#[derive(Clone, Debug, Serialize)]
pub struct Record {
    pub cursor: u64,
    pub admission: Admission,
    pub headers: Option<Headers>,
    pub finished: Option<Finished>,
}
/// Separate opt-in request evidence store. No model content, key value or usage estimation.
pub struct Ledger {
    db: Connection,
    target: Id,
    _owner: File,
}
impl Ledger {
    pub fn initialize(directory: &Path, target: Id, maximum_bytes: u64) -> Result<Self> {
        valid_limit(maximum_bytes)?;
        filesystem::directory(directory)?;
        let owner = filesystem::lease(&directory.join("owner.lock"))?;
        let file = filesystem::private_new(&directory.join(DATABASE))?;
        file.sync_all().map_err(|_| Error::Storage)?;
        drop(file);
        let mut db = Connection::open_with_flags(
            directory.join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage)?;
        configure(&db, maximum_bytes)?;
        let tx = db.transaction().map_err(storage)?;
        tx.execute_batch("CREATE TABLE metadata(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema TEXT NOT NULL,target TEXT NOT NULL);
CREATE TABLE requests(id TEXT NOT NULL UNIQUE,subject TEXT NOT NULL,admitted_ms INTEGER NOT NULL,intent TEXT NOT NULL);
CREATE INDEX request_scope ON requests(subject,admitted_ms);
CREATE TABLE request_events(request TEXT NOT NULL REFERENCES requests(id),phase TEXT NOT NULL CHECK(phase IN ('headers','finished')),evidence TEXT NOT NULL,PRIMARY KEY(request,phase));
CREATE TABLE links(producer TEXT NOT NULL,gateway_request TEXT NOT NULL,request TEXT NOT NULL UNIQUE REFERENCES requests(id),PRIMARY KEY(producer,gateway_request));
CREATE TABLE sessions(id TEXT NOT NULL UNIQUE,subject TEXT NOT NULL,idempotency_key TEXT NOT NULL,intent TEXT NOT NULL,UNIQUE(subject,idempotency_key));
CREATE TABLE session_bindings(session TEXT PRIMARY KEY REFERENCES sessions(id),internal TEXT NOT NULL UNIQUE,evidence TEXT NOT NULL);
CREATE TRIGGER request_no_update BEFORE UPDATE ON requests BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER request_no_delete BEFORE DELETE ON requests BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER event_no_update BEFORE UPDATE ON request_events BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER event_no_delete BEFORE DELETE ON request_events BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER link_no_update BEFORE UPDATE ON links BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER link_no_delete BEFORE DELETE ON links BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER session_no_update BEFORE UPDATE ON sessions BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER session_no_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER binding_no_update BEFORE UPDATE ON session_bindings BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER binding_no_delete BEFORE DELETE ON session_bindings BEGIN SELECT RAISE(ABORT,'immutable'); END;").map_err(storage)?;
        tx.execute(
            "INSERT INTO metadata VALUES(1,?1,?2)",
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
    pub fn open(directory: &Path, target: Id, maximum_bytes: u64) -> Result<Self> {
        valid_limit(maximum_bytes)?;
        filesystem::directory(directory)?;
        let owner = filesystem::lease(&directory.join("owner.lock"))?;
        filesystem::regular(&directory.join(DATABASE))?;
        let db = Connection::open_with_flags(
            directory.join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(storage)?;
        let (schema, stored): (String, String) = db
            .query_row(
                "SELECT schema,target FROM metadata WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| Error::InvalidStore)?;
        if schema != STORE_SCHEMA {
            return Err(Error::UnsupportedSchema);
        }
        if stored != target.as_str() {
            return Err(Error::InvalidStore);
        }
        configure(&db, maximum_bytes)?;
        Ok(Self {
            db,
            target,
            _owner: owner,
        })
    }
    pub fn target(&self) -> &Id {
        &self.target
    }
    pub fn admit(
        &mut self,
        principal: &Principal,
        peer: &Peer,
        route: String,
        session: Option<Id>,
    ) -> Result<Admission> {
        if peer.identity.target != self.target
            || !principal.permissions.permits_route(&route)
            || !peer.routes.contains_key(&route)
        {
            return Err(Error::Forbidden);
        }
        let admission = Admission {
            id: new_id(),
            subject: principal.identity.subject.clone(),
            credential: principal.identity.credential.clone(),
            authorization_sha256: principal.authorization_version.clone(),
            route,
            at_ms: now()?,
            instance: peer.identity.instance.clone(),
            configuration_sha256: peer.identity.configuration_sha256.clone(),
            producer: peer.identity.producer.clone(),
            session,
        };
        let tx = self.db.transaction().map_err(storage)?;
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get(0))
            .map_err(storage)?;
        if count >= MAX_REQUESTS {
            return Err(Error::Conflict);
        }
        tx.execute(
            "INSERT INTO requests VALUES(?1,?2,?3,?4)",
            params![
                admission.id.as_str(),
                admission.subject.as_str(),
                i64::try_from(admission.at_ms).map_err(|_| Error::Storage)?,
                encode(&admission)?
            ],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(admission)
    }
    pub fn headers(&mut self, admission: &Admission, headers: &Headers) -> Result<()> {
        let tx = self.db.transaction().map_err(storage)?;
        if let (Some(producer), Some(gateway_request)) =
            (&admission.producer, &headers.gateway_request)
        {
            tx.execute(
                "INSERT INTO links VALUES(?1,?2,?3)",
                params![
                    producer.as_str(),
                    gateway_request.as_str(),
                    admission.id.as_str()
                ],
            )
            .map_err(storage)?;
        }
        tx.execute(
            "INSERT INTO request_events VALUES(?1,'headers',?2)",
            params![admission.id.as_str(), encode(headers)?],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)
    }
    pub fn finish(&mut self, id: &Id, end: End) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO request_events VALUES(?1,'finished',?2)",
                params![
                    id.as_str(),
                    encode(&Finished {
                        at_ms: now()?,
                        transport: end
                    })?
                ],
            )
            .map_err(storage)?;
        Ok(())
    }
    pub fn records(&self, principal: &Principal, query: &UsageQuery) -> Result<Vec<Record>> {
        query.validate()?;
        if query.all && !principal.permissions.read_all_usage {
            return Err(Error::Forbidden);
        }
        let tx = self.db.unchecked_transaction().map_err(storage)?;
        let mut q=tx.prepare("SELECT rowid,intent FROM requests WHERE rowid>?1 AND admitted_ms>=?2 AND admitted_ms<?3 AND (?4 OR subject=?5) ORDER BY rowid LIMIT 100").map_err(storage)?;
        let rows = q
            .query_map(
                params![
                    query.after as i64,
                    query.from_ms as i64,
                    query.to_ms as i64,
                    query.all,
                    principal.identity.subject.as_str()
                ],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .map_err(storage)?;
        rows.map(|row| {
            let (cursor, text) = row.map_err(storage)?;
            let admission: Admission = decode(&text)?;
            if !query.all && admission.subject != principal.identity.subject {
                return Err(Error::InvalidStore);
            }
            let headers = event::<Headers>(&tx, &admission.id, "headers")?;
            let finished = event::<Finished>(&tx, &admission.id, "finished")?;
            Ok(Record {
                cursor: u64::try_from(cursor).map_err(|_| Error::InvalidStore)?,
                admission,
                headers,
                finished,
            })
        })
        .collect()
    }
    pub fn session_intent(
        &mut self,
        principal: &Principal,
        route: &str,
        origin: &Digest,
        key: Id,
    ) -> Result<(SessionIntent, bool)> {
        if !principal.permissions.permits_route(route) {
            return Err(Error::Forbidden);
        }
        let tx = self.db.transaction().map_err(storage)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT intent FROM sessions WHERE subject=?1 AND idempotency_key=?2",
                params![principal.identity.subject.as_str(), key.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if let Some(previous) = previous {
            let value: SessionIntent = decode(&previous)?;
            if value.subject != principal.identity.subject
                || value.route != route
                || value.origin_sha256 != *origin
            {
                return Err(Error::Conflict);
            }
            return Ok((value, false));
        }
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .map_err(storage)?;
        if count >= MAX_SESSIONS {
            return Err(Error::Conflict);
        }
        let intent = SessionIntent {
            id: new_id(),
            subject: principal.identity.subject.clone(),
            credential: principal.identity.credential.clone(),
            route: route.into(),
            origin_sha256: origin.clone(),
            idempotency_key: key,
            at_ms: now()?,
        };
        tx.execute(
            "INSERT INTO sessions VALUES(?1,?2,?3,?4)",
            params![
                intent.id.as_str(),
                intent.subject.as_str(),
                intent.idempotency_key.as_str(),
                encode(&intent)?
            ],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok((intent, true))
    }
    pub fn bind_session(&mut self, intent: &SessionIntent, internal: Id) -> Result<()> {
        let binding = SessionBinding {
            session: intent.id.clone(),
            internal,
            origin_sha256: intent.origin_sha256.clone(),
        };
        self.db
            .execute(
                "INSERT INTO session_bindings VALUES(?1,?2,?3)",
                params![
                    intent.id.as_str(),
                    binding.internal.as_str(),
                    encode(&binding)?
                ],
            )
            .map_err(storage)?;
        Ok(())
    }
    pub fn session(
        &self,
        principal: &Principal,
        id: &Id,
    ) -> Result<(SessionIntent, Option<SessionBinding>)> {
        let tx = self.db.unchecked_transaction().map_err(storage)?;
        let text: Option<String> = tx
            .query_row(
                "SELECT intent FROM sessions WHERE id=?1 AND subject=?2",
                params![id.as_str(), principal.identity.subject.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let value: SessionIntent = decode(&text.ok_or(Error::NotFound)?)?;
        if value.id != *id || value.subject != principal.identity.subject {
            return Err(Error::InvalidStore);
        }
        if !principal.permissions.permits_route(&value.route) {
            return Err(Error::Forbidden);
        }
        let binding: Option<String> = tx
            .query_row(
                "SELECT evidence FROM session_bindings WHERE session=?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let binding: Option<SessionBinding> = binding.map(|v| decode(&v)).transpose()?;
        if binding
            .as_ref()
            .is_some_and(|b| b.session != *id || b.origin_sha256 != value.origin_sha256)
        {
            return Err(Error::InvalidStore);
        }
        Ok((value, binding))
    }
    pub fn session_view(&self, principal: &Principal, id: &Id) -> Result<Value> {
        let (intent, binding) = self.session(principal, id)?;
        Ok(
            json!({"schema":SCHEMA,"session":intent.id,"model":intent.route,"state":if binding.is_some(){"bound"}else{"unconfirmed"}}),
        )
    }
    pub fn backup(&self, destination: &Path) -> Result<()> {
        filesystem::directory(destination.parent().ok_or(Error::InvalidStore)?)?;
        drop(filesystem::private_new(destination)?);
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
fn event<T: DeserializeOwned>(db: &Connection, id: &Id, phase: &str) -> Result<Option<T>> {
    let text: Option<String> = db
        .query_row(
            "SELECT evidence FROM request_events WHERE request=?1 AND phase=?2",
            params![id.as_str(), phase],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    text.map(|t| decode(&t)).transpose()
}
fn valid_limit(value: u64) -> Result<()> {
    if !(1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&value) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn configure(db: &Connection, maximum: u64) -> Result<()> {
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
    if page <= 0 {
        return Err(Error::InvalidStore);
    }
    db.pragma_update(None, "max_page_count", (maximum / page as u64) as i64)
        .map_err(storage)?;
    Ok(())
}
