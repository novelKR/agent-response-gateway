//! Versioned continuation records. Storage never authorizes inference or tool execution.
pub mod control;
mod replay;
mod sqlite;
pub use replay::{
    ENVELOPE_V2, NativeReplay, Outcome, REPLAY_V2, ReplayRecord, ReplayV2, public_reasoning,
};
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
    pub pending_tools: bool,
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
    pub input_len: usize,
    pub input_sha256: String,
    pub provider_status: String,
    pub steps: Vec<Value>,
    pub output: Vec<Value>,
}
impl Replay {
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || self.epoch <= 0
            || self.steps.is_empty()
            || self.input_sha256.len() != 64
            || !matches!(
                self.provider_status.as_str(),
                "completed" | "requires_action"
            )
        {
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
    fn bind_protection(&mut self, fingerprint: &str) -> Result<()>;
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
    fn finalize(
        &mut self,
        id: &str,
        attempt: &str,
        digest: &str,
        envelope: &str,
        pending_tools: bool,
    ) -> Result<()>;
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
        self.seal_record(&ReplayRecord::V1(replay.clone()))
    }
    pub fn seal_record(&self, replay: &ReplayRecord) -> Result<String> {
        replay.validate()?;
        let prefix = if replay.schema() == SCHEMA {
            ENVELOPE_PREFIX
        } else {
            ENVELOPE_V2
        };
        let aad = if replay.schema() == SCHEMA {
            self.id.clone()
        } else {
            format!("{prefix}{}", self.id)
        };
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
                aead::Aad::from(aad.as_bytes()),
                &mut bytes,
            )
            .map_err(|_| Error("encryption"))?;
        Ok(format!(
            "{prefix}{}.{}.{}",
            self.id,
            hex(&nonce),
            hex(&bytes)
        ))
    }
    pub fn open(&self, envelope: &str) -> Result<Replay> {
        match self.open_record(envelope)? {
            ReplayRecord::V1(v) => Ok(v),
            ReplayRecord::V2(_) => Err(Error("legacy replay required")),
        }
    }
    pub fn open_record(&self, envelope: &str) -> Result<ReplayRecord> {
        let prefix = if envelope.starts_with(ENVELOPE_PREFIX) {
            ENVELOPE_PREFIX
        } else {
            ENVELOPE_V2
        };
        let aad = if prefix == ENVELOPE_PREFIX {
            self.id.clone()
        } else {
            format!("{prefix}{}", self.id)
        };
        if envelope.len() > 2 * MAX_PAYLOAD + 1024 {
            return Err(Error("payload limit"));
        }
        let mut fields = envelope
            .strip_prefix(prefix)
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
                aead::Aad::from(aad.as_bytes()),
                &mut bytes,
            )
            .map_err(|_| Error("authentication"))?;
        let replay: ReplayRecord =
            serde_json::from_slice(plain).map_err(|_| Error("replay format"))?;
        replay.validate()?;
        if (prefix == ENVELOPE_PREFIX) != (replay.schema() == SCHEMA) {
            return Err(Error("replay version mismatch"));
        }
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
    pub async fn restore_record(&self, session: Session, envelope: String) -> Result<ReplayRecord> {
        self.access(move |store, key| {
            let original = key.open_record(&envelope)?;
            let hash = digest(&original)?;
            let replay = original.clone().normalize();
            if replay.session != session.id
                || replay.epoch != session.epoch
                || replay.origin != session.origin
            {
                return Err(Error("origin mismatch"));
            }
            let record = store.record(&replay.response)?;
            if record.session != session.id
                || record.epoch != session.epoch
                || record.digest != hash
            {
                return Err(Error("record mismatch"));
            }
            if let Some(saved) = record.envelope {
                let authoritative = key.open_record(&saved)?;
                if digest(&authoritative)? != hash {
                    return Err(Error("stored payload mismatch"));
                }
                Ok(authoritative)
            } else {
                store.repair(&record.id, &hash, &envelope)?;
                Ok(original)
            }
        })
        .await
    }
    pub async fn restore(&self, session: Session, envelope: String) -> Result<Replay> {
        match self.restore_record(session, envelope).await? {
            ReplayRecord::V1(v) => Ok(v),
            ReplayRecord::V2(_) => Err(Error("legacy replay required")),
        }
    }
    pub async fn finalize(
        &self,
        replay: impl Into<ReplayRecord> + Send + 'static,
    ) -> Result<String> {
        self.finalize_checked(replay, |_| Ok(())).await
    }
    pub async fn finalize_checked(
        &self,
        replay: impl Into<ReplayRecord> + Send + 'static,
        check: impl FnOnce(&str) -> Result<()> + Send + 'static,
    ) -> Result<String> {
        let record = replay.into();
        self.access(move |store, key| {
            let envelope = key.seal_record(&record)?;
            check(&envelope)?;
            let hash = digest(&record)?;
            let replay = record.normalize();
            store.finalize(
                &replay.session,
                &replay.response,
                &hash,
                &envelope,
                replay.outcome == Outcome::AwaitingTools,
            )?;
            Ok(envelope)
        })
        .await
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub directory: std::path::PathBuf,
    pub store_id: String,
    pub realm: String,
    pub generation: String,
    pub key_id: String,
    pub key_env: String,
    pub control_token_env: String,
    pub max_store_bytes: u64,
}
impl Configuration {
    pub fn validate(&self) -> std::result::Result<(), crate::ConfigError> {
        let fail = || crate::ConfigError("Invalid continuation configuration".into());
        for s in [
            &self.store_id,
            &self.realm,
            &self.generation,
            &self.key_id,
            &self.key_env,
            &self.control_token_env,
        ] {
            label(s).map_err(|_| fail())?;
        }
        if !self.directory.is_absolute()
            || self.max_store_bytes < 16 * 1024 * 1024
            || self.max_store_bytes > i64::MAX as u64
            || self.key_env == self.control_token_env
        {
            return Err(fail());
        }
        Ok(())
    }
    pub fn start(
        &self,
        secrets: &crate::Secrets,
    ) -> std::result::Result<(Runtime, String), crate::ConfigError> {
        let fail = || crate::ConfigError("Cannot initialize verified continuation runtime".into());
        let key = std::env::var(&self.key_env).map_err(|_| fail())?;
        let control = std::env::var(&self.control_token_env).map_err(|_| fail())?;
        if !(32..=4096).contains(&control.len())
            || !control.bytes().all(|b| b.is_ascii_graphic())
            || key == control
            || control == secrets.local_token
            || key == secrets.local_token
            || secrets
                .upstream_keys
                .values()
                .any(|v| v == &control || v == &key)
        {
            return Err(fail());
        }
        let bytes = unhex(&key).map_err(|_| fail())?;
        let protector = Protector::new(self.key_id.clone(), &bytes).map_err(|_| fail())?;
        let mut store =
            SqliteStore::open(&self.directory, false, self.max_store_bytes).map_err(|_| fail())?;
        if store.identity().map_err(|_| fail())? != self.store_id {
            return Err(fail());
        }
        let binding_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &bytes);
        let fingerprint = hex(ring::hmac::sign(
            &binding_key,
            format!(
                "gateway-store-protection/v1:{}:{}",
                self.store_id, self.key_id
            )
            .as_bytes(),
        )
        .as_ref());
        store.bind_protection(&fingerprint).map_err(|_| fail())?;
        Ok((Runtime::new(Box::new(store), protector), control))
    }
}
