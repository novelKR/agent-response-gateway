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
    pub(crate) segments: std::collections::BTreeMap<usize, (usize, NativeReplay)>,
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
            Self::Messages {
                version: 1,
                blocks,
                controls,
            } if !blocks.is_empty() && controls.is_object() => Ok(()),
            _ => Err(IrError::ContinuityMismatch),
        }
    }
}
