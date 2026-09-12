//! Explicit, versioned policy selection. These declarations never qualify a provider.
use serde::{Deserialize, Serialize};

use crate::{
    ConfigError,
    ir::{
        ApiProtocol,
        capability::{BridgeRule, CapabilityProfile, Feature, Support},
    },
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityPolicy {
    pub version: u32,
    #[serde(default)]
    pub tools: ToolPolicy,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolPolicy {
    pub custom_input: Option<CustomInput>,
    pub namespaces: Option<Namespaces>,
    pub grammar: Option<GrammarPolicy>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomInput {
    Preserve,
    FunctionJson,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Namespaces {
    Preserve,
    Flatten,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrammarPolicy {
    Preserve,
    RegisteredOutputValidation,
}

#[derive(Clone, Debug, Serialize)]
pub struct BoundPolicy {
    pub id: String,
    pub policy: CompatibilityPolicy,
}

fn invalid() -> ConfigError {
    ConfigError("Invalid or conflicting compatibility policy".into())
}

impl CompatibilityPolicy {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != 1 {
            return Err(invalid());
        }
        Ok(())
    }

    /// Keep provider facts separate from the effective, implemented route capabilities.
    /// Existing bridge declarations are inherited; a new explicit choice must agree.
    pub fn apply(&self, source: &CapabilityProfile) -> Result<CapabilityProfile, ConfigError> {
        self.validate()?;
        let mut target = source.clone();
        let custom = self.tools.custom_input.map(|p| match p {
            CustomInput::Preserve => None,
            CustomInput::FunctionJson => Some(BridgeRule::CustomToolJson),
        });
        let namespace = self.tools.namespaces.map(|p| match p {
            Namespaces::Preserve => None,
            Namespaces::Flatten => Some(BridgeRule::ToolNamespace),
        });
        for (feature, selected) in [
            (Feature::CustomTools, custom),
            (Feature::NamespacedTools, namespace),
        ] {
            if let Some(rule) = selected {
                select(&mut target, feature, rule)?;
            }
        }
        let wrapped =
            target.support(Feature::CustomTools) == Support::Bridged(BridgeRule::CustomToolJson);
        if let Some(grammar) = self.tools.grammar {
            let rule = match grammar {
                GrammarPolicy::Preserve => None,
                GrammarPolicy::RegisteredOutputValidation if wrapped => {
                    Some(BridgeRule::CodexPatchGrammar)
                }
                GrammarPolicy::RegisteredOutputValidation
                    if source.protocol == ApiProtocol::Responses =>
                {
                    Some(BridgeRule::RegisteredGrammarValidation)
                }
                GrammarPolicy::RegisteredOutputValidation => return Err(invalid()),
            };
            select(&mut target, Feature::CustomGrammar, rule)?;
        }
        if wrapped && target.support(Feature::CustomGrammar) == Support::Native {
            // Function JSON cannot retain provider-side custom grammar generation.
            return Err(invalid());
        }
        for (feature, rule) in [
            (Feature::CustomTools, BridgeRule::CustomToolJson),
            (Feature::NamespacedTools, BridgeRule::ToolNamespace),
        ] {
            if target.support(feature) == Support::Bridged(rule)
                && target.support(Feature::FunctionTools) != Support::Native
            {
                return Err(invalid());
            }
        }
        if target.support(Feature::CustomGrammar) == Support::Bridged(BridgeRule::CodexPatchGrammar)
            && !wrapped
        {
            return Err(invalid());
        }
        target.validate().map_err(|_| invalid())?;
        Ok(target)
    }
}

fn select(
    profile: &mut CapabilityProfile,
    feature: Feature,
    rule: Option<BridgeRule>,
) -> Result<(), ConfigError> {
    let prior = profile.support(feature);
    match rule {
        None if matches!(prior, Support::Bridged(_)) => Err(invalid()),
        None => Ok(()),
        Some(rule) if prior == Support::Unsupported || prior == Support::Bridged(rule) => {
            profile.support.insert(feature, Support::Bridged(rule));
            Ok(())
        }
        _ => Err(invalid()),
    }
}

impl BoundPolicy {
    pub fn adapter_version(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("policy serialization");
        format!(
            "compatibility/1/{}",
            crate::continuation::hex(&crate::digest::sha256(&bytes))
        )
    }
}
