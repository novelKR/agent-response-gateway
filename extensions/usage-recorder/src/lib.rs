//! Standalone recorder. The model gateway does not link this crate or database drivers.
pub mod export;
pub mod query;
use gateway_usage_contract::{EventKind, ReceiptStatus, RecorderConfig, UsageEvent, digest};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const STORAGE_VERSION: i64 = 1;
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// Native extensions are trusted same-user code; reject accidental links and public state.
pub fn private_path(path: &Path, directory: bool) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("invalid_private_path".into());
    }
    for p in path.ancestors() {
        if fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("linked_private_path".into());
        }
    }
    let m = fs::symlink_metadata(path)?;
    if directory != m.is_dir() {
        return Err("invalid_private_path".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.mode() & 0o077 != 0 || (!directory && (!m.is_file() || m.nlink() != 1)) {
            return Err("insecure_private_path".into());
        }
        if !directory
            && let Some(parent) = path.parent()
            && fs::metadata(parent)?.uid() != m.uid()
        {
            return Err("private_owner_mismatch".into());
        }
    }
    #[cfg(not(unix))]
    {
        return Err("unsupported_native_platform".into());
    }
    Ok(())
}
pub fn read_private(path: &Path, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    private_path(path, false)?;
    let f = File::open(path)?;
    if f.metadata()?.len() > limit {
        return Err("private_file_too_large".into());
    }
    let mut data = vec![];
    f.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err("private_file_too_large".into());
    }
    Ok(data)
}
pub fn config(path: &Path) -> Result<RecorderConfig> {
    let c: RecorderConfig = serde_json::from_slice(&read_private(path, 65536)?)?;
    if c.schema != "gateway-usage-recorder-config/v1" || c.destinations.len() > 8 {
        return Err("invalid_recorder_config".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for d in &c.destinations {
        match d {
            gateway_usage_contract::Destination::Http {
                url, bearer_file, ..
            } => {
                let url = reqwest::Url::parse(url)?;
                let local = url
                    .host_str()
                    .and_then(|v| v.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
                    .is_some_and(|v| v.is_loopback());
                if !(url.scheme() == "https" || (url.scheme() == "http" && local))
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !Path::new(bearer_file).is_absolute()
                {
                    return Err("invalid_destination".into());
                }
            }
            gateway_usage_contract::Destination::Postgres {
                connection_file,
                tls_ca_file,
                ..
            } => {
                if !Path::new(connection_file).is_absolute()
                    || tls_ca_file
                        .as_ref()
                        .is_some_and(|p| !Path::new(p).is_absolute())
                {
                    return Err("invalid_destination".into());
                }
            }
        }
        if !gateway_usage_contract::safe_label(d.id()) || !ids.insert(d.id()) {
            return Err("invalid_destination".into());
        }
    }
    Ok(c)
}
struct StoreLock(File);
impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub struct Store {
    pub connection: Connection,
    pub directory: PathBuf,
    _lock: Option<StoreLock>,
}
impl Store {
    pub fn open(directory: &Path, create: bool, writer: bool) -> Result<Self> {
        private_path(directory, true)?;
        let lock = if writer {
            let path = directory.join(".writer.lock");
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            if path.exists() {
                private_path(&path, false)?;
            }
            let f = options.open(&path)?;
            private_path(&path, false)?;
            f.try_lock()?;
            Some(StoreLock(f))
        } else {
            None
        };
        let path = directory.join("usage.sqlite3");
        if create {
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            opts.open(&path)?;
        }
        private_path(&path, false)?;
        for suffix in ["-wal", "-shm"] {
            let p = directory.join(format!("usage.sqlite3{suffix}"));
            if p.exists() {
                private_path(&p, false)?;
            }
        }
        let flags = if writer {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let connection = Connection::open_with_flags(&path, flags)?;
        connection.busy_timeout(Duration::from_secs(3))?;
        if writer {
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "FULL")?;
        }
        if create {
            connection.execute_batch("BEGIN;
CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE usage_attempts(producer_id TEXT NOT NULL,attempt_id TEXT NOT NULL,identity TEXT NOT NULL,PRIMARY KEY(producer_id,attempt_id));
CREATE TABLE usage_events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,producer_id TEXT NOT NULL,event_id TEXT NOT NULL,attempt_id TEXT NOT NULL,revision INTEGER NOT NULL,kind TEXT NOT NULL,started_at_ms INTEGER NOT NULL,observed_at_ms INTEGER NOT NULL,sha256 TEXT NOT NULL,payload TEXT NOT NULL,UNIQUE(producer_id,event_id),UNIQUE(producer_id,attempt_id,revision));
CREATE TABLE usage_tombstones(producer_id TEXT NOT NULL,event_id TEXT NOT NULL,attempt_id TEXT NOT NULL,revision INTEGER NOT NULL,sha256 TEXT NOT NULL,kind TEXT NOT NULL,PRIMARY KEY(producer_id,event_id),UNIQUE(producer_id,attempt_id,revision));
CREATE VIEW usage_current AS SELECT e.* FROM usage_events e WHERE e.revision=(SELECT MAX(x.revision) FROM usage_events x WHERE x.producer_id=e.producer_id AND x.attempt_id=e.attempt_id);
CREATE TABLE destinations(id TEXT PRIMARY KEY,sha256 TEXT NOT NULL);
CREATE TABLE usage_outbox(producer_id TEXT NOT NULL,event_id TEXT NOT NULL,destination TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'pending',attempts INTEGER NOT NULL DEFAULT 0,next_at_ms INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(producer_id,event_id,destination));
PRAGMA user_version=1;COMMIT;")?;
            connection.execute(
                "INSERT INTO metadata VALUES('producer_id',?1)",
                [uuid::Uuid::new_v4().to_string()],
            )?;
        }
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version != STORAGE_VERSION {
            return Err("unsupported_storage_schema".into());
        }
        Ok(Self {
            connection,
            directory: directory.into(),
            _lock: lock,
        })
    }
    pub fn producer(&self) -> Result<String> {
        Ok(self.connection.query_row(
            "SELECT value FROM metadata WHERE key='producer_id'",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn bind_destinations(&mut self, c: &RecorderConfig) -> Result<()> {
        let tx = self.connection.transaction()?;
        let existing: Vec<String> = tx
            .prepare("SELECT id FROM destinations")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for id in existing {
            if !c.destinations.iter().any(|d| d.id() == id) {
                let pending: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM usage_outbox WHERE destination=? AND state!='committed'",
                    [&id],
                    |r| r.get(0),
                )?;
                if pending > 0 {
                    return Err("destination_has_pending_events".into());
                }
            }
        }
        for d in &c.destinations {
            let sha = digest(&serde_json::to_vec(d)?);
            let old: Option<String> = tx
                .query_row(
                    "SELECT sha256 FROM destinations WHERE id=?",
                    [d.id()],
                    |r| r.get(0),
                )
                .optional()?;
            if old.as_ref().is_some_and(|v| *v != sha) {
                return Err("destination_identity_changed".into());
            }
            tx.execute(
                "INSERT OR IGNORE INTO destinations VALUES(?1,?2)",
                params![d.id(), sha],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn record(&mut self, event: &UsageEvent, c: &RecorderConfig) -> Result<ReceiptStatus> {
        let bytes = event.bytes()?;
        let sha = digest(&bytes);
        let payload = String::from_utf8(bytes)?;
        let revision = i64::try_from(event.revision)?;
        let start = i64::try_from(event.started_at_ms)?;
        let observed = i64::try_from(event.observed_at_ms)?;
        let identity = serde_json::to_string(&(
            &event.request_id,
            event.started_at_ms,
            &event.provider,
            &event.model_alias,
            &event.upstream_model,
            event.profile,
            &event.configuration_sha256,
        ))?;
        let tx = self.connection.transaction()?;
        let prior:Option<String>=tx.query_row("SELECT sha256 FROM (SELECT producer_id,event_id,attempt_id,revision,sha256 FROM usage_events UNION ALL SELECT producer_id,event_id,attempt_id,revision,sha256 FROM usage_tombstones) WHERE producer_id=?1 AND (event_id=?2 OR (attempt_id=?3 AND revision=?4))",params![event.producer_id,event.event_id,event.attempt_id,revision],|r|r.get(0)).optional()?;
        if let Some(old) = prior {
            return Ok(if old == sha {
                ReceiptStatus::Duplicate
            } else {
                ReceiptStatus::Conflict
            });
        }
        let retired: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM usage_tombstones WHERE producer_id=?1 AND attempt_id=?2 AND kind='attempt_finished')", params![event.producer_id,event.attempt_id], |r|r.get(0))?;
        if retired {
            return Ok(ReceiptStatus::Conflict);
        }
        let old_identity: Option<String> = tx
            .query_row(
                "SELECT identity FROM usage_attempts WHERE producer_id=?1 AND attempt_id=?2",
                params![event.producer_id, event.attempt_id],
                |r| r.get(0),
            )
            .optional()?;
        if old_identity.as_ref().is_some_and(|v| *v != identity) {
            return Ok(ReceiptStatus::Conflict);
        }
        let latest_revision: Option<i64> = tx.query_row(
            "SELECT MAX(revision) FROM usage_events WHERE producer_id=?1 AND attempt_id=?2",
            params![event.producer_id, event.attempt_id],
            |r| r.get(0),
        )?;
        if event.kind == EventKind::AttemptFinished && latest_revision.is_some_and(|v| v > revision)
        {
            return Ok(ReceiptStatus::Conflict);
        }
        let final_revision:Option<i64>=tx.query_row("SELECT revision FROM usage_events WHERE producer_id=?1 AND attempt_id=?2 AND kind='attempt_finished'",params![event.producer_id,event.attempt_id],|r|r.get(0)).optional()?;
        if final_revision.is_some_and(|v| revision >= v || event.kind == EventKind::AttemptFinished)
        {
            return Ok(ReceiptStatus::Conflict);
        }
        tx.execute(
            "INSERT OR IGNORE INTO usage_attempts VALUES(?1,?2,?3)",
            params![event.producer_id, event.attempt_id, identity],
        )?;
        let kind = match event.kind {
            EventKind::AttemptStarted => "attempt_started",
            EventKind::UsageUpdated => "usage_updated",
            EventKind::AttemptFinished => "attempt_finished",
        };
        tx.execute(
            "INSERT INTO usage_events(producer_id,event_id,attempt_id,revision,kind,started_at_ms,observed_at_ms,sha256,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                event.producer_id,
                event.event_id,
                event.attempt_id,
                revision,
                kind,
                start,
                observed,
                sha,
                payload
            ],
        )?;
        for d in &c.destinations {
            tx.execute(
                "INSERT INTO usage_outbox(producer_id,event_id,destination) VALUES(?1,?2,?3)",
                params![event.producer_id, event.event_id, d.id()],
            )?;
        }
        tx.commit()?;
        Ok(ReceiptStatus::Committed)
    }
    pub fn backup(&self, destination: &Path) -> Result<()> {
        private_path(destination.parent().ok_or("invalid_backup_path")?, true)?;
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        opts.open(destination)?;
        self.connection.backup("main", destination, None)?;
        Ok(())
    }
}
