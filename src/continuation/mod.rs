//! Versioned continuation records. Storage never authorizes inference or tool execution.
pub mod control;
mod sqlite;
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use sqlite::SqliteStore;
use std::sync::{Arc, Mutex};

pub const SCHEMA: &str = "gateway-continuation/v1";
pub const ENVELOPE_PREFIX: &str = "arg-continuation-v1.";
const MAX_PAYLOAD: usize = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("Continuation operation rejected: {0}")]
pub struct Error(pub &'static str);
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub route: Value,
    pub realm: String,
    pub generation: String,
}
impl Origin {
    pub fn validate(&self) -> Result<()> {
        label(&self.realm)?;
        label(&self.generation)?;
        if !self.route.is_object() {
            return Err(Error("invalid origin"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub epoch: i64,
    pub revision: i64,
    pub origin: Origin,
    pub status: String,
    pub head: Option<String>,
    pub portable_sha256: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub schema: String,
    pub session: String,
    pub epoch: i64,
    pub origin: Origin,
    pub response: String,
    pub parent: Option<String>,
    pub steps: Vec<Value>,
    pub output: Vec<Value>,
}
impl Replay {
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA || self.epoch <= 0 || self.steps.is_empty() {
            return Err(Error("invalid replay"));
        }
        label(&self.session)?;
        label(&self.response)?;
        self.origin.validate()?;
        if let Some(parent) = &self.parent {
            label(parent)?;
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct StoredReplay {
    pub id: String,
    pub session: String,
    pub epoch: i64,
    pub digest: String,
    pub envelope: Option<String>,
}

/// Transactions are business operations rather than database-specific SQL primitives.
/// PostgreSQL implementations must pass the same contract tests, including compare-and-set.
pub trait ContinuationStore: Send {
    fn create(&mut self, origin: &Origin) -> Result<Session>;
    fn session(&mut self, id: &str) -> Result<Session>;
    fn begin(
        &mut self,
        id: &str,
        revision: i64,
        parent: Option<&str>,
        input_digest: &str,
        reserve: u64,
    ) -> Result<String>;
    fn finalize(&mut self, id: &str, attempt: &str, digest: &str, envelope: &str) -> Result<()>;
    fn uncertain(&mut self, id: &str, attempt: &str) -> Result<()>;
    fn record(&mut self, id: &str) -> Result<StoredReplay>;
    fn repair(&mut self, id: &str, digest: &str, envelope: &str) -> Result<()>;
    fn transition(
        &mut self,
        id: &str,
        revision: i64,
        kind: &str,
        portable: Option<&str>,
        decision: &str,
    ) -> Result<Session>;
}

/// Keys are supplied by the host; never derived from provider credentials or local tokens.
pub struct Protector {
    key: aead::LessSafeKey,
    id: String,
}
impl Protector {
    pub fn new(id: String, key: &[u8]) -> Result<Self> {
        label(&id)?;
        let key = aead::UnboundKey::new(&aead::AES_256_GCM, key)
            .map_err(|_| Error("invalid protection key"))?;
        Ok(Self {
            key: aead::LessSafeKey::new(key),
            id,
        })
    }
    pub fn seal(&self, replay: &Replay) -> Result<String> {
        replay.validate()?;
        let mut bytes = serde_json::to_vec(replay).map_err(|_| Error("encoding"))?;
        if bytes.len() > MAX_PAYLOAD {
            return Err(Error("payload limit"));
        }
        let mut nonce = [0; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| Error("randomness"))?;
        self.key
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.id.as_bytes()),
                &mut bytes,
            )
            .map_err(|_| Error("encryption"))?;
        Ok(format!(
            "{ENVELOPE_PREFIX}{}.{}.{}",
            self.id,
            hex(&nonce),
            hex(&bytes)
        ))
    }
    pub fn open(&self, envelope: &str) -> Result<Replay> {
        if envelope.len() > 2 * MAX_PAYLOAD + 1024 {
            return Err(Error("payload limit"));
        }
        let mut fields = envelope
            .strip_prefix(ENVELOPE_PREFIX)
            .ok_or(Error("envelope version"))?
            .split('.');
        if fields.next() != Some(self.id.as_str()) {
            return Err(Error("key identity"));
        }
        let nonce: [u8; 12] = unhex(fields.next().ok_or(Error("envelope"))?)?
            .try_into()
            .map_err(|_| Error("nonce"))?;
        let mut bytes = unhex(fields.next().ok_or(Error("envelope"))?)?;
        if fields.next().is_some() {
            return Err(Error("envelope"));
        }
        let plain = self
            .key
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.id.as_bytes()),
                &mut bytes,
            )
            .map_err(|_| Error("authentication"))?;
        let replay: Replay = serde_json::from_slice(plain).map_err(|_| Error("replay format"))?;
        replay.validate()?;
        Ok(replay)
    }
}

pub fn label(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error("invalid identifier"));
    }
    Ok(())
}
pub fn digest(value: &impl Serialize) -> Result<String> {
    Ok(hex(&crate::digest::sha256(
        &serde_json::to_vec(value).map_err(|_| Error("encoding"))?,
    )))
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error("hex encoding"));
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| {
            u8::from_str_radix(
                std::str::from_utf8(v).map_err(|_| Error("hex encoding"))?,
                16,
            )
            .map_err(|_| Error("hex encoding"))
        })
        .collect()
}

/// Blocking database and crypto work is always run outside Tokio's reactor threads.
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Mutex<Box<dyn ContinuationStore>>>,
    protector: Arc<Protector>,
}
impl Runtime {
    pub fn new(store: Box<dyn ContinuationStore>, protector: Protector) -> Self {
        Self {
            inner: Arc::new(Mutex::new(store)),
            protector: Arc::new(protector),
        }
    }
    pub async fn access<T: Send + 'static>(
        &self,
        op: impl FnOnce(&mut dyn ContinuationStore, &Protector) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = this.inner.lock().map_err(|_| Error("store unavailable"))?;
            op(store.as_mut(), &this.protector)
        })
        .await
        .map_err(|_| Error("worker failed"))?
    }
    pub async fn restore(&self, session: Session, envelope: String) -> Result<Replay> {
        self.access(move |store, key| {
            let replay = key.open(&envelope)?;
            if replay.session != session.id
                || replay.epoch != session.epoch
                || replay.origin != session.origin
            {
                return Err(Error("origin mismatch"));
            }
            let record = store.record(&replay.response)?;
            let hash = digest(&replay)?;
            if record.session != session.id
                || record.epoch != session.epoch
                || record.digest != hash
            {
                return Err(Error("record mismatch"));
            }
            if let Some(saved) = record.envelope {
                if digest(&key.open(&saved)?)? != hash {
                    return Err(Error("stored payload mismatch"));
                }
            } else {
                store.repair(&record.id, &hash, &envelope)?;
            }
            Ok(replay)
        })
        .await
    }
    pub async fn finalize(&self, replay: Replay) -> Result<String> {
        self.access(move |store, key| {
            let envelope = key.seal(&replay)?;
            store.finalize(
                &replay.session,
                &replay.response,
                &digest(&replay)?,
                &envelope,
            )?;
            Ok(envelope)
        })
        .await
    }
}
