use gateway_management::{Error, Id, Result};
use gateway_usage_contract::UsageEvent;
use std::path::Path;
/// Exact lookup only. Implementations never discover subjects from time/model proximity.
pub trait UsageReader: Send {
    fn lookup(&mut self, producer: &Id, request: &Id) -> Result<Vec<UsageEvent>>;
}
/// Uses the existing Recorder's explicit read-only opening and current-event schema.
pub struct SqliteUsage {
    store: gateway_usage_recorder::Store,
}
impl SqliteUsage {
    pub fn open(path: &Path) -> Result<Self> {
        gateway_usage_recorder::Store::open(path, false, false)
            .map(|store| Self { store })
            .map_err(|_| Error::InvalidStore)
    }
}
impl UsageReader for SqliteUsage {
    fn lookup(&mut self, producer: &Id, request: &Id) -> Result<Vec<UsageEvent>> {
        if self.store.producer().map_err(|_| Error::Storage)? != producer.as_str() {
            return Err(Error::Conflict);
        }
        let mut query=self.store.connection.prepare("SELECT CASE WHEN length(payload)<=65536 THEN payload ELSE NULL END FROM usage_current WHERE producer_id=?1 AND json_extract(payload,'$.request_id')=?2 ORDER BY attempt_id LIMIT 17").map_err(|_|Error::Storage)?;
        let rows = query
            .query_map(
                rusqlite::params![producer.as_str(), request.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|_| Error::Storage)?;
        let mut values = Vec::new();
        for row in rows {
            let text = row
                .map_err(|_| Error::Storage)?
                .ok_or(Error::InvalidStore)?;
            let value: UsageEvent = serde_json::from_str(&text).map_err(|_| Error::InvalidStore)?;
            if !value.validate()
                || value.producer_id != producer.as_str()
                || value.request_id != request.as_str()
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
