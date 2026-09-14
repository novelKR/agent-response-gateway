use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ApiProtocol, IrError, capability::CapabilityProfile};

/// A declaration snapshot, not a live capability attestation or bearer credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteSnapshot {
    pub provider_id: String,
    pub model: String,
    pub api: ApiProtocol,
    pub credential_binding: String,
    pub adapter_version: String,
    pub capabilities: CapabilityProfile,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

impl RouteSnapshot {
    pub fn validate(&self) -> Result<(), IrError> {
        for value in [
            &self.provider_id,
            &self.model,
            &self.credential_binding,
            &self.adapter_version,
        ] {
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                return Err(IrError::InvalidIdentifier);
            }
        }
        self.capabilities.validate()?;
        if self.api != self.capabilities.protocol {
            return Err(IrError::WrongProtocol);
        }
        if self.context_window == Some(0) || self.max_output_tokens == Some(0) {
            return Err(IrError::InvalidField("route_limits"));
        }
        Ok(())
    }
}

/// Scope is an opaque principal/auth realm reference owned by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuityBinding {
    pub route: RouteSnapshot,
    pub scope: String,
}

impl ContinuityBinding {
    pub fn validate(&self) -> Result<(), IrError> {
        self.route.validate()?;
        if self.scope.is_empty()
            || self.scope.len() > 512
            || self.scope.chars().any(char::is_control)
        {
            return Err(IrError::InvalidIdentifier);
        }
        Ok(())
    }
}

/// Deliberately not Debug/Serialize. This is a bound byte container, not encryption.
#[derive(Clone, PartialEq, Eq)]
pub struct OpaqueState {
    binding: ContinuityBinding,
    format: String,
    bytes: Vec<u8>,
}

impl OpaqueState {
    pub fn new(
        binding: ContinuityBinding,
        format: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Self, IrError> {
        binding.validate()?;
        let format = format.into();
        if format.is_empty() || format.len() > 128 || format.chars().any(char::is_control) {
            return Err(IrError::InvalidField("opaque_format"));
        }
        if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
            return Err(IrError::SizeLimit);
        }
        Ok(Self {
            binding,
            format,
            bytes,
        })
    }
    pub fn binding(&self) -> &ContinuityBinding {
        &self.binding
    }
    pub fn format(&self) -> &str {
        &self.format
    }
    pub fn replay<'a>(
        &'a self,
        target: &ContinuityBinding,
        format: &str,
    ) -> Result<&'a [u8], IrError> {
        target.validate()?;
        if target != &self.binding || format != self.format {
            return Err(IrError::ContinuityMismatch);
        }
        Ok(&self.bytes)
    }
}

/// Replay spans admitted by the continuation authentication boundary. The empty
/// default is safe; populated spans are constructed only inside this crate after
/// checking session, epoch, durable finalization and exact public/input digests.
/// This type deliberately has no Debug or deserialization implementation.
#[derive(Default)]
pub struct VerifiedProviderHistory {
    pub(crate) segments: std::collections::BTreeMap<usize, (usize, NativeState)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    AwaitingTools,
}

/// Native state remains separate from Responses output, even when it contains text.
/// No Debug implementation: these values can contain private provider signatures.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", deny_unknown_fields)]
pub enum NativeReplay {
    #[serde(rename = "gemini_steps")]
    Gemini { version: u32, steps: Vec<Value> },
    #[serde(rename = "chat_assistant")]
    Chat {
        version: u32,
        dialect: super::reasoning::ChatDialect,
        assistant: Value,
        controls: Value,
    },
    #[serde(rename = "messages_content")]
    Messages {
        version: u32,
        blocks: Vec<Value>,
        controls: Value,
    },
}
impl NativeReplay {
    pub fn validate(&self) -> std::result::Result<(), IrError> {
        match self {
            Self::Gemini { version: 1, steps } if !steps.is_empty() => Ok(()),
            Self::Chat {
                version: 1,
                assistant,
                controls,
                ..
            } if assistant.is_object() && controls.is_object() => Ok(()),
            Self::Messages {
                version: 1,
                blocks,
                controls,
            } if !blocks.is_empty() && controls.is_object() => Ok(()),
            _ => Err(IrError::ContinuityMismatch),
        }
    }
}

/// Internal state dispatch has no serde implementation, so it cannot widen old codec wire types.
#[derive(Clone, PartialEq)]
pub(crate) enum NativeState {
    Builtin(NativeReplay),
    Provider(ProviderNativeState),
}
impl From<NativeReplay> for NativeState {
    fn from(value: NativeReplay) -> Self {
        Self::Builtin(value)
    }
}
impl NativeState {
    pub(crate) fn builtin(&self) -> Result<&NativeReplay, IrError> {
        match self {
            Self::Builtin(value) => Ok(value),
            Self::Provider(_) => Err(IrError::ContinuityMismatch),
        }
    }
    pub(crate) fn provider(&self) -> Result<&ProviderNativeState, IrError> {
        match self {
            Self::Provider(value) => Ok(value),
            Self::Builtin(_) => Err(IrError::ContinuityMismatch),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ProviderStateBinding {
    pub protocol: String,
    pub provider_protocol: String,
    pub id: String,
    pub version: String,
    pub package_sha256: String,
    pub executable_sha256: String,
}
impl ProviderStateBinding {
    pub(crate) fn validate(&self) -> Result<(), IrError> {
        let id = &self.id;
        let parts: Vec<_> = self.version.split('.').collect();
        let hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.protocol != gateway_plugin_contract::PROVIDER_PROTOCOL
            || !gateway_plugin_contract::valid_provider_protocol(&self.provider_protocol)
            || id.is_empty()
            || id.len() > 64
            || !id.as_bytes()[0].is_ascii_lowercase()
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || parts.len() != 3
            || parts.iter().any(|p| {
                p.is_empty()
                    || p.len() > 6
                    || !p.bytes().all(|b| b.is_ascii_digit())
                    || (p.len() > 1 && p.starts_with('0'))
            })
            || !hash(&self.package_sha256)
            || !hash(&self.executable_sha256)
        {
            return Err(IrError::ContinuityMismatch);
        }
        Ok(())
    }
}
/// Plain native bytes only exist after host validation; never Debug or Serialize.
#[derive(Clone, PartialEq)]
pub(crate) struct ProviderNativeState {
    pub binding: ProviderStateBinding,
    pub format: String,
    pub version: u32,
    bytes: Vec<u8>,
}
impl ProviderNativeState {
    pub(crate) const MAX_BYTES: usize = 1024 * 1024;
    pub(crate) fn new(
        binding: ProviderStateBinding,
        format: String,
        version: u32,
        bytes: Vec<u8>,
    ) -> Result<Self, IrError> {
        binding.validate()?;
        if format.is_empty()
            || format.len() > 64
            || !format.as_bytes()[0].is_ascii_lowercase()
            || !format
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            || version == 0
            || bytes.len() > Self::MAX_BYTES
        {
            return Err(IrError::ContinuityMismatch);
        }
        Ok(Self {
            binding,
            format,
            version,
            bytes,
        })
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn validate_for(
        &self,
        binding: &ProviderStateBinding,
        expected_format: Option<(&str, u32)>,
    ) -> Result<(), IrError> {
        if &self.binding != binding
            || expected_format
                .is_some_and(|(format, version)| format != self.format || version != self.version)
        {
            return Err(IrError::ContinuityMismatch);
        }
        binding.validate()
    }
}
