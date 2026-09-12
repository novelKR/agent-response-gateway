use super::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::Duration,
};

pub struct SqliteStore {
    db: Connection,
    _owner: File,
    limit: u64,
}
fn sql(_: rusqlite::Error) -> Error {
    Error("database operation")
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| Error("encoding"))
}
fn load(db: &Connection, id: &str) -> Result<Session> {
    let text: String = db
        .query_row("SELECT value FROM sessions WHERE id=?1", [id], |r| r.get(0))
        .map_err(sql)?;
    serde_json::from_str(&text).map_err(|_| Error("session format"))
}
fn save(db: &Connection, s: &Session) -> Result<()> {
    db.execute(
        "UPDATE sessions SET value=?2 WHERE id=?1",
        params![s.id, encode(s)?],
    )
    .map_err(sql)?;
    Ok(())
}
impl SqliteStore {
    pub fn open(directory: &Path, initialize: bool, limit: u64) -> Result<Self> {
        if !directory.is_absolute() || limit < 1024 * 1024 {
            return Err(Error("store configuration"));
        }
        for p in directory.ancestors() {
            if std::fs::symlink_metadata(p)
                .map_err(|_| Error("store path"))?
                .file_type()
                .is_symlink()
            {
                return Err(Error("store link"));
            }
        }
        if !directory.is_dir() {
            return Err(Error("store directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(directory)
                .map_err(|_| Error("store permissions"))?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(Error("private store required"));
            }
        }
        let lock = directory.join("owner.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if lock
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(Error("lock link"));
        }
        let owner = options.open(lock).map_err(|_| Error("store lock"))?;
        owner.try_lock().map_err(|_| Error("store already owned"))?;
        let path = directory.join("continuation.sqlite3");
        if initialize {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(&path)
                .map_err(|_| Error("store already exists"))?;
        }
        let meta = path
            .symlink_metadata()
            .map_err(|_| Error("store missing"))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(Error("store file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() != 1 || meta.mode() & 0o077 != 0 {
                return Err(Error("store file permissions"));
            }
        }
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql)?;
        db.busy_timeout(Duration::from_secs(1)).map_err(sql)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )
        .map_err(sql)?;
        if initialize {
            db.execute_batch("BEGIN IMMEDIATE;
          CREATE TABLE metadata (schema TEXT NOT NULL, identity TEXT NOT NULL);
          CREATE TABLE sessions (id TEXT PRIMARY KEY, value TEXT NOT NULL);
          CREATE TABLE attempts (id TEXT PRIMARY KEY, session TEXT NOT NULL REFERENCES sessions(id), epoch INTEGER NOT NULL, input_digest TEXT NOT NULL, status TEXT NOT NULL, reserve INTEGER NOT NULL, UNIQUE(session,epoch,input_digest));
          CREATE TABLE records (id TEXT PRIMARY KEY REFERENCES attempts(id), digest TEXT NOT NULL, envelope TEXT);
          CREATE TABLE transitions (id TEXT PRIMARY KEY, session TEXT NOT NULL REFERENCES sessions(id), revision INTEGER NOT NULL, kind TEXT NOT NULL, decision TEXT NOT NULL);
          COMMIT;").map_err(sql)?;
            db.execute(
                "INSERT INTO metadata VALUES (?1,?2)",
                params![SCHEMA, uuid::Uuid::new_v4().to_string()],
            )
            .map_err(sql)?;
        }
        let schema: String = db
            .query_row("SELECT schema FROM metadata", [], |r| r.get(0))
            .map_err(sql)?;
        if schema != SCHEMA {
            return Err(Error("store schema"));
        }
        let mut this = Self {
            db,
            _owner: owner,
            limit,
        };
        // Crashed in-flight attempts stay uncertain. Never silently mark them complete.
        let tx = this.db.transaction().map_err(sql)?;
        let sessions: Vec<String> = {
            let mut q = tx
                .prepare("SELECT DISTINCT session FROM attempts WHERE status='pending'")
                .map_err(sql)?;
            q.query_map([], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        for id in sessions {
            let mut s = load(&tx, &id)?;
            s.status = "unknown".into();
            s.revision += 1;
            save(&tx, &s)?;
        }
        tx.execute(
            "UPDATE attempts SET status='unknown' WHERE status='pending'",
            [],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(this)
    }
    pub fn identity(&self) -> Result<String> {
        self.db
            .query_row("SELECT identity FROM metadata", [], |r| r.get(0))
            .map_err(sql)
    }
}
impl ContinuationStore for SqliteStore {
    fn create(&mut self, origin: &Origin) -> Result<Session> {
        origin.validate()?;
        let s = Session {
            id: uuid::Uuid::new_v4().to_string(),
            epoch: 1,
            revision: 1,
            origin: origin.clone(),
            status: "ready".into(),
            head: None,
            portable_sha256: None,
        };
        self.db
            .execute(
                "INSERT INTO sessions VALUES (?1,?2)",
                params![s.id, encode(&s)?],
            )
            .map_err(sql)?;
        Ok(s)
    }
    fn session(&mut self, id: &str) -> Result<Session> {
        label(id)?;
        load(&self.db, id)
    }
    fn begin(
        &mut self,
        id: &str,
        revision: i64,
        parent: Option<&str>,
        input_digest: &str,
        reserve: u64,
    ) -> Result<String> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sql)?;
        let mut s = load(&tx, id)?;
        if s.revision != revision
            || !matches!(s.status.as_str(), "ready" | "compacting")
            || s.head.as_deref() != parent
        {
            return Err(Error("session conflict or reconciliation required"));
        }
        let used: i64 = tx
            .query_row("SELECT COALESCE(SUM(reserve),0) FROM attempts", [], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        if reserve == 0
            || u64::try_from(used)
                .map_err(|_| Error("capacity format"))?
                .checked_add(reserve)
                .is_none_or(|v| v > self.limit)
        {
            return Err(Error("store capacity"));
        }
        let attempt = format!("resp_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO attempts VALUES (?1,?2,?3,?4,'pending',?5)",
            params![
                attempt,
                id,
                s.epoch,
                input_digest,
                i64::try_from(reserve).map_err(|_| Error("capacity limit"))?
            ],
        )
        .map_err(sql)?;
        s.status = if s.status == "compacting" {
            "compacting_pending"
        } else {
            "pending"
        }
        .into();
        s.revision += 1;
        save(&tx, &s)?;
        tx.commit().map_err(sql)?;
        Ok(attempt)
    }
    fn finalize(&mut self, id: &str, attempt: &str, digest: &str, envelope: &str) -> Result<()> {
        let tx = self.db.transaction().map_err(sql)?;
        let mut s = load(&tx, id)?;
        let reserve:i64=tx.query_row("SELECT reserve FROM attempts WHERE id=?1 AND session=?2 AND epoch=?3 AND status='pending'",params![attempt,id,s.epoch],|r|r.get(0)).map_err(sql)?;
        if i64::try_from(envelope.len()).map_err(|_| Error("payload limit"))? > reserve
            || !matches!(s.status.as_str(), "pending" | "compacting_pending")
        {
            return Err(Error("finalization conflict"));
        }
        tx.execute(
            "INSERT INTO records VALUES (?1,?2,?3)",
            params![attempt, digest, envelope],
        )
        .map_err(sql)?;
        tx.execute(
            "UPDATE attempts SET status='finalized' WHERE id=?1",
            [attempt],
        )
        .map_err(sql)?;
        s.status = if s.status == "compacting_pending" {
            "awaiting_compaction"
        } else {
            "ready"
        }
        .into();
        s.head = Some(attempt.into());
        s.revision += 1;
        save(&tx, &s)?;
        tx.commit().map_err(sql)?;
        Ok(())
    }
    fn uncertain(&mut self, id: &str, attempt: &str) -> Result<()> {
        let tx = self.db.transaction().map_err(sql)?;
        let count=tx.execute("UPDATE attempts SET status='unknown' WHERE id=?1 AND session=?2 AND status='pending'",params![attempt,id]).map_err(sql)?;
        if count != 0 {
            let mut s = load(&tx, id)?;
            s.status = "unknown".into();
            s.revision += 1;
            save(&tx, &s)?;
        }
        tx.commit().map_err(sql)?;
        Ok(())
    }
    fn record(&mut self, id: &str) -> Result<StoredReplay> {
        self.db.query_row("SELECT a.id,a.session,a.epoch,r.digest,r.envelope FROM attempts a JOIN records r ON a.id=r.id WHERE a.id=?1 AND a.status='finalized'",[id],|r|Ok(StoredReplay{id:r.get(0)?,session:r.get(1)?,epoch:r.get(2)?,digest:r.get(3)?,envelope:r.get(4)?})).optional().map_err(sql)?.ok_or(Error("finalized record missing"))
    }
    fn repair(&mut self, id: &str, digest: &str, envelope: &str) -> Result<()> {
        let changed=self.db.execute("UPDATE records SET envelope=?3 WHERE id=?1 AND digest=?2 AND envelope IS NULL AND EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND status='finalized' AND reserve>=length(?3))",params![id,digest,envelope]).map_err(sql)?;
        if changed != 1 {
            return Err(Error("repair conflict"));
        }
        Ok(())
    }
    fn transition(
        &mut self,
        id: &str,
        revision: i64,
        kind: &str,
        portable: Option<&str>,
        decision: &str,
    ) -> Result<Session> {
        label(decision)?;
        let tx = self.db.transaction().map_err(sql)?;
        let mut s = load(&tx, id)?;
        if s.revision != revision || matches!(s.status.as_str(), "pending" | "compacting_pending") {
            return Err(Error("transition conflict"));
        }
        match kind {
            "compact_begin" if s.status == "ready" => {
                s.status = "compacting".into();
            }
            "compact_commit" if s.status == "awaiting_compaction" => {
                if portable.is_none() {
                    return Err(Error("portable history required"));
                }
                s.epoch += 1;
                s.head = None;
                s.portable_sha256 = portable.map(str::to_owned);
                s.status = "ready".into();
            }
            "recover" => {
                if portable.is_none() {
                    return Err(Error("portable history required"));
                }
                s.epoch += 1;
                s.head = None;
                s.portable_sha256 = portable.map(str::to_owned);
                s.status = "ready".into();
            }
            _ => return Err(Error("unsupported transition")),
        }
        s.revision += 1;
        save(&tx, &s)?;
        tx.execute(
            "INSERT INTO transitions VALUES (?1,?2,?3,?4,?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                id,
                s.revision,
                kind,
                decision
            ],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(s)
    }
}
