use std::{collections::BTreeMap, net::SocketAddr};

use serde::Deserialize;
use url::Url;

use crate::ConfigError;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    #[serde(default = "default_token_env")]
    pub local_token_env: String,
    pub source_url: Option<String>,
    #[serde(default)]
    pub limits: Limits,
    pub providers: BTreeMap<String, Provider>,
    pub models: BTreeMap<String, Model>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub base_url: String,
    pub api_key_env: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub provider: String,
    pub upstream_model: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_in_flight: usize,
    pub request_body_timeout_ms: u64,
    pub connect_timeout_ms: u64,
    pub response_header_timeout_ms: u64,
    pub stream_idle_timeout_ms: u64,
    pub shutdown_grace_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: 8 * 1024 * 1024,
            max_response_bytes: 16 * 1024 * 1024,
            max_in_flight: 32,
            request_body_timeout_ms: 30_000,
            connect_timeout_ms: 10_000,
            response_header_timeout_ms: 60_000,
            stream_idle_timeout_ms: 60_000,
            shutdown_grace_ms: 5_000,
        }
    }
}

fn default_listen() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}
fn default_token_env() -> String {
    "ARG_LOCAL_TOKEN".into()
}

fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
}

fn safe_label(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_/.:".contains(&b))
}

impl Config {
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        // Parser diagnostics may contain source lines. Never echo configuration text.
        let config: Self = toml::from_str(raw).map_err(|_| {
            ConfigError("Invalid TOML configuration or unknown configuration field".into())
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.listen.ip().is_loopback() {
            return Err(ConfigError(
                "Only loopback listen addresses are supported".into(),
            ));
        }
        if !valid_env_name(&self.local_token_env)
            || self.providers.is_empty()
            || self.models.is_empty()
        {
            return Err(ConfigError(
                "A valid local token environment name, provider and model are required".into(),
            ));
        }
        if let Some(source) = &self.source_url {
            let url = Url::parse(source).map_err(|_| ConfigError("Invalid source_url".into()))?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(ConfigError(
                    "source_url must be an HTTPS URL without credentials".into(),
                ));
            }
        }
        for (id, provider) in &self.providers {
            if !safe_label(id) || !valid_env_name(&provider.api_key_env) {
                return Err(ConfigError(
                    "Invalid provider identifier or credential environment name".into(),
                ));
            }
            provider.responses_url()?;
        }
        for (id, model) in &self.models {
            if !safe_label(id)
                || !self.providers.contains_key(&model.provider)
                || model.upstream_model.is_empty()
                || model.upstream_model.len() > 256
                || model.upstream_model.chars().any(char::is_control)
            {
                return Err(ConfigError(
                    "Invalid model mapping or unknown provider".into(),
                ));
            }
        }
        let l = &self.limits;
        if !(1..=64 * 1024 * 1024).contains(&l.max_request_bytes)
            || !(1..=64 * 1024 * 1024).contains(&l.max_response_bytes)
            || !(1..=1024).contains(&l.max_in_flight)
            || [
                l.request_body_timeout_ms,
                l.connect_timeout_ms,
                l.response_header_timeout_ms,
                l.stream_idle_timeout_ms,
                l.shutdown_grace_ms,
            ]
            .iter()
            .any(|n| !(1..=3_600_000).contains(n))
        {
            return Err(ConfigError("Limits must be positive: bodies <= 64 MiB, concurrency <= 1024, timeouts <= 1 hour".into()));
        }
        Ok(())
    }
}

impl Provider {
    pub fn responses_url(&self) -> Result<Url, ConfigError> {
        let mut url = Url::parse(&self.base_url)
            .map_err(|_| ConfigError("Invalid provider base_url".into()))?;
        let loopback = url
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        {
            return Err(ConfigError("Provider base_url requires HTTPS (HTTP only for a numeric loopback host), without credentials, query or fragment".into()));
        }
        let path = format!("{}/responses", url.path().trim_end_matches('/'));
        url.set_path(&path);
        Ok(url)
    }
}

/// Values are deliberately neither Debug nor Serialize.
pub struct Secrets {
    pub local_token: String,
    pub upstream_keys: BTreeMap<String, String>,
}

impl Secrets {
    pub fn from_env(config: &Config) -> Result<Self, ConfigError> {
        let get = |name: &str| {
            std::env::var(name).map_err(|_| {
                ConfigError(format!("Missing credential environment variable: {name}"))
            })
        };
        let mut upstream_keys = BTreeMap::new();
        for (id, provider) in &config.providers {
            upstream_keys.insert(id.clone(), get(&provider.api_key_env)?);
        }
        let secrets = Self {
            local_token: get(&config.local_token_env)?,
            upstream_keys,
        };
        secrets.validate(config)?;
        Ok(secrets)
    }

    pub fn validate(&self, config: &Config) -> Result<(), ConfigError> {
        let valid = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic());
        if self.local_token.len() < 32 || self.local_token.len() > 4096 || !valid(&self.local_token)
        {
            return Err(ConfigError(
                "Local token must contain 32..4096 printable non-space ASCII characters".into(),
            ));
        }
        for id in config.providers.keys() {
            if !self
                .upstream_keys
                .get(id)
                .is_some_and(|v| v.len() <= 8192 && valid(v) && v != &self.local_token)
            {
                return Err(ConfigError(format!(
                    "Missing or invalid upstream credential for provider {id}; local and provider credentials must differ"
                )));
            }
        }
        Ok(())
    }
}
