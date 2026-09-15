use gateway_management::{Error, Id, Result};
use gateway_usage_contract::RecordedEvent;
use std::path::Path;
/// Exact lookup only. Implementations never discover subjects from time/model proximity.
pub trait UsageReader: Send {
    fn lookup(&mut self, producer: &Id, request: &Id) -> Result<Vec<RecordedEvent>>;
}
/// Uses the existing Recorder's explicit read-only opening and current-event schema.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct SqliteUsage {
    store: gateway_usage_recorder::Store,
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub struct SqliteUsage;
impl SqliteUsage {
    pub const fn supported() -> bool {
        cfg!(any(target_os = "linux", target_os = "macos"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn open(_path: &Path) -> Result<Self> {
        Err(Error::Unsupported)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn open(path: &Path) -> Result<Self> {
        gateway_usage_recorder::Store::open(path, false, false)
            .map(|store| Self { store })
            .map_err(|_| Error::InvalidStore)
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl UsageReader for SqliteUsage {
    fn lookup(&mut self, producer: &Id, request: &Id) -> Result<Vec<RecordedEvent>> {
        if self.store.producer().map_err(|_| Error::Storage)? != producer.as_str() {
            return Err(Error::Conflict);
        }
        let mut query=self.store.connection.prepare("SELECT CASE WHEN length(payload)<=65536 THEN payload ELSE NULL END,sha256 FROM usage_current WHERE producer_id=?1 AND json_extract(payload,'$.request_id')=?2 ORDER BY attempt_id LIMIT 17").map_err(|_|Error::Storage)?;
        let rows = query
            .query_map(
                rusqlite::params![producer.as_str(), request.as_str()],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|_| Error::Storage)?;
        let mut values = Vec::new();
        for row in rows {
            let (text, sha256) = row.map_err(|_| Error::Storage)?;
            let text = text.ok_or(Error::InvalidStore)?;
            let value = RecordedEvent::from_stored_bytes(text.as_bytes(), &sha256)
                .map_err(|_| Error::InvalidStore)?;
            if *value.view().producer_id != producer.as_str()
                || *value.view().request_id != request.as_str()
            {
                return Err(Error::InvalidStore);
            }
            values.push(value);
        }
        if values.len() > 16 {
            return Err(Error::Conflict);
        }
        Ok(values)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl UsageReader for SqliteUsage {
    fn lookup(&mut self, _producer: &Id, _request: &Id) -> Result<Vec<RecordedEvent>> {
        Err(Error::Unsupported)
    }
}
