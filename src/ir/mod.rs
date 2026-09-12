//! Versioned, pure model contracts. These do not activate a provider or execute tools.
#![doc = include_str!("../../docs/ir.md")]
pub mod bridge;
pub mod capability;
pub mod continuity;
pub mod event;
pub mod grammar;
pub mod request;
pub mod responses;

pub const VERSION: u16 = 1;

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Deserialize,
    serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    #[default]
    Responses,
    Messages,
    ChatCompletions,
    GeminiInteractions,
}

/// Static diagnostics never contain prompts, tool arguments, or opaque state.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IrError {
    #[error("invalid IR field: {0}")]
    InvalidField(&'static str),
    #[error("invalid identifier")]
    InvalidIdentifier,
    #[error("unsupported IR version")]
    UnsupportedVersion,
    #[error("unsupported stateless feature")]
    UnsupportedFeature,
    #[error("extension has no cross-protocol translation")]
    UnsupportedExtension,
    #[error("extension conflicts with a typed field")]
    ExtensionConflict,
    #[error("protocol does not match the selected codec")]
    WrongProtocol,
    #[error("opaque state needs an explicit origin binding")]
    UnboundOpaqueState,
    #[error("opaque state binding differs from the target")]
    ContinuityMismatch,
    #[error("duplicate identity")]
    DuplicateId,
    #[error("invalid tool mapping")]
    InvalidToolMapping,
    #[error("tool arguments are not complete JSON")]
    InvalidJsonArguments,
    #[error("invalid event transition")]
    InvalidEventOrder,
    #[error("item is not registered")]
    UnknownItem,
    #[error("IR resource limit exceeded")]
    SizeLimit,
}

macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IrError> {
                let value = value.into();
                if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                    return Err(IrError::InvalidIdentifier);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

identity!(ItemId);
identity!(CallId);
identity!(ResponseId);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolIdentity {
    pub namespace: Option<String>,
    pub name: String,
}

impl ToolIdentity {
    pub fn new(namespace: Option<String>, name: impl Into<String>) -> Result<Self, IrError> {
        let result = Self {
            namespace,
            name: name.into(),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<(), IrError> {
        ItemId::new(self.name.clone())?;
        if let Some(value) = &self.namespace {
            ItemId::new(value.clone())?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolKind {
    Function,
    Custom,
}

pub mod reasoning;
