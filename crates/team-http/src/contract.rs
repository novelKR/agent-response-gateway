use gateway_management::{Digest, Error, Id, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, net::SocketAddr, time::Duration};
use zeroize::Zeroizing;
pub const SCHEMA: &str = "gateway-team-http/v1";
pub const STORE_SCHEMA: &str = "gateway-team-requests/v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedOrigin {
    pub route: Value,
    pub realm: String,
    pub generation: String,
}
impl ManagedOrigin {
    pub fn digest(&self) -> Result<Digest> {
        if !self.route.is_object()
            || [&self.realm, &self.generation].iter().any(|v| {
                v.is_empty()
                    || v.len() > 128
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(Error::InvalidInput);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InvalidInput)?;
        if bytes.len() > 65536 {
            return Err(Error::InvalidInput);
        }
        Ok(Digest::of(&bytes))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub managed: Option<ManagedOrigin>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerIdentity {
    pub target: Id,
    pub instance: Id,
    pub configuration_sha256: Digest,
    pub producer: Option<Id>,
}
/// Verified host registration, never a request-body URL or credential. No Debug/Serialize.
#[derive(Clone)]
pub struct Peer {
    pub identity: PeerIdentity,
    pub routes: BTreeMap<String, Route>,
    pub(crate) endpoint: url::Url,
    pub(crate) local_token: Zeroizing<String>,
    pub(crate) control_token: Option<Zeroizing<String>>,
}
impl Peer {
    pub fn new(
        identity: PeerIdentity,
        endpoint: &str,
        routes: BTreeMap<String, Route>,
        local_token: String,
        control_token: Option<String>,
    ) -> Result<Self> {
        let endpoint = url::Url::parse(endpoint).map_err(|_| Error::InvalidInput)?;
        let host = endpoint
            .host_str()
            .ok_or(Error::InvalidInput)?
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .map_err(|_| Error::InvalidInput)?;
        if endpoint.scheme() != "http"
            || !host.is_loopback()
            || endpoint.port_or_known_default() == Some(0)
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
            || routes.is_empty()
            || routes.len() > 128
            || !token(&local_token)
            || control_token.as_ref().is_some_and(|v| !token(v))
            || control_token.as_ref() == Some(&local_token)
        {
            return Err(Error::InvalidInput);
        }
        for (alias, route) in &routes {
            if alias.is_empty()
                || alias.len() > 256
                || alias.trim() != alias
                || alias.chars().any(char::is_control)
            {
                return Err(Error::InvalidInput);
            }
            if let Some(origin) = &route.managed {
                origin.digest()?;
                if control_token.is_none() {
                    return Err(Error::InvalidInput);
                }
            }
        }
        Ok(Self {
            identity,
            endpoint,
            routes,
            local_token: Zeroizing::new(local_token),
            control_token: control_token.map(Zeroizing::new),
        })
    }
    pub(crate) fn url(&self, path: &str) -> Result<url::Url> {
        self.endpoint.join(path).map_err(|_| Error::InvalidInput)
    }
}
fn token(value: &str) -> bool {
    !value.starts_with("gwt1_")
        && (32..=4096).contains(&value.len())
        && value.bytes().all(|b| b.is_ascii_graphic())
}
/// The host withdraws/replaces a peer when its owned execution changes. Already admitted
/// requests keep their exact peer snapshot; new admissions never adopt a port/PID implicitly.
pub trait PeerSource: Send + Sync {
    fn current(&self) -> Result<Peer>;
}
impl PeerSource for Peer {
    fn current(&self) -> Result<Peer> {
        Ok(self.clone())
    }
}
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_in_flight: usize,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub body_timeout: Duration,
    pub header_timeout: Duration,
    pub idle_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_in_flight: 32,
            max_request_bytes: 2 * 1024 * 1024,
            max_response_bytes: 16 * 1024 * 1024,
            body_timeout: Duration::from_secs(30),
            header_timeout: Duration::from_secs(120),
            idle_timeout: Duration::from_secs(120),
        }
    }
}
impl Limits {
    pub(crate) fn validate(&self, bound: SocketAddr) -> Result<()> {
        if !bound.ip().is_loopback()
            || bound.port() == 0
            || !(1..=256).contains(&self.max_in_flight)
            || !(1024..=16 * 1024 * 1024).contains(&self.max_request_bytes)
            || !(1024..=64 * 1024 * 1024).contains(&self.max_response_bytes)
            || [self.body_timeout, self.header_timeout, self.idle_timeout]
                .into_iter()
                .any(|t| t < Duration::from_millis(10) || t > Duration::from_secs(600))
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Eof,
    ClientDisconnected,
    GatewayConnectionFailed,
    GatewayBodyLost,
    HeaderTimeout,
    IdleTimeout,
    ResponseLimit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub id: Id,
    pub subject: Id,
    pub credential: Id,
    pub authorization_sha256: Digest,
    pub route: String,
    pub at_ms: u64,
    pub instance: Id,
    pub configuration_sha256: Digest,
    pub producer: Option<Id>,
    pub session: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Headers {
    pub at_ms: u64,
    pub status: u16,
    pub gateway_request: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finished {
    pub at_ms: u64,
    pub transport: End,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionIntent {
    pub id: Id,
    pub subject: Id,
    pub credential: Id,
    pub route: String,
    pub origin_sha256: Digest,
    pub idempotency_key: Id,
    pub at_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub session: Id,
    pub internal: Id,
    pub origin_sha256: Digest,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageQuery {
    pub from_ms: u64,
    pub to_ms: u64,
    #[serde(default)]
    pub after: u64,
    #[serde(default)]
    pub all: bool,
}
impl UsageQuery {
    pub fn validate(&self) -> Result<()> {
        if self.from_ms >= self.to_ms
            || self.to_ms > i64::MAX as u64
            || self.to_ms - self.from_ms > 366 * 86400000
            || self.after > i64::MAX as u64
        {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
}
pub(crate) fn now() -> Result<u64> {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Storage)?
            .as_millis(),
    )
    .map_err(|_| Error::Storage)
}
pub(crate) fn new_id() -> Id {
    Id::new(uuid::Uuid::new_v4().to_string()).expect("generated UUID")
}

/// Validate the usage-specific nested contract without changing team/admin transport.
/// V1-only reports retain their original shape; V2 events require an exact declaration.
pub fn validate_usage_report(value: &Value) -> Result<()> {
    if value["schema"] != SCHEMA {
        return Err(Error::UnsupportedSchema);
    }
    let rows = value["requests"].as_array().ok_or(Error::InvalidInput)?;
    let mut versioned = false;
    for row in rows {
        if row["usage"]["state"] == "observed" {
            let events = row["usage"]["attempts"]
                .as_array()
                .ok_or(Error::InvalidInput)?;
            if events.is_empty() || events.len() > 16 {
                return Err(Error::InvalidInput);
            }
            for raw in events {
                let event: gateway_usage_contract::RecordedEvent =
                    serde_json::from_value(raw.clone()).map_err(|_| Error::UnsupportedSchema)?;
                if !event.validate() {
                    return Err(Error::InvalidInput);
                }
                versioned |= event.is_v2();
            }
        }
    }
    if versioned {
        if value.get("usage_event_schemas")
            != Some(&serde_json::json!([
                gateway_usage_contract::SCHEMA,
                gateway_usage_contract::SCHEMA_V2
            ]))
        {
            return Err(Error::UnsupportedSchema);
        }
    } else if value.get("usage_event_schemas").is_some() {
        return Err(Error::UnsupportedSchema);
    }
    Ok(())
}
