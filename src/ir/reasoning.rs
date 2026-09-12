//! Explicit reasoning wire contracts. Model names and endpoint URLs never select a dialect.
use super::{ApiProtocol, IrError, request::RequestIR};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReasoningContract {
    ClaudeAdaptive {
        version: u32,
        efforts: BTreeSet<String>,
        default_effort: String,
        #[serde(default)]
        allow_forced_tools: bool,
    },
    ClaudeManual {
        version: u32,
        budget_tokens: u64,
        #[serde(default)]
        effort_budgets: BTreeMap<String, u64>,
        #[serde(default)]
        interleaved_beta: bool,
    },
}
impl ReasoningContract {
    pub fn validate(&self, api: ApiProtocol) -> Result<(), IrError> {
        let valid = match self {
            Self::ClaudeAdaptive {
                version: 1,
                efforts,
                default_effort,
                ..
            } => {
                api == ApiProtocol::Messages
                    && !efforts.is_empty()
                    && efforts.contains(default_effort)
                    && efforts
                        .iter()
                        .all(|e| matches!(e.as_str(), "low" | "medium" | "high" | "xhigh" | "max"))
            }
            Self::ClaudeManual {
                version: 1,
                budget_tokens,
                effort_budgets,
                ..
            } => {
                api == ApiProtocol::Messages
                    && *budget_tokens >= 1024
                    && effort_budgets.iter().all(|(e, b)| {
                        matches!(e.as_str(), "low" | "medium" | "high" | "xhigh" | "max")
                            && *b >= 1024
                    })
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(IrError::UnsupportedFeature)
        }
    }
    pub fn controls(&self, request: &RequestIR, max: u64) -> Result<Value, IrError> {
        let options = request.generation.reasoning.as_ref();
        if options
            .and_then(|r| r.summary.as_deref())
            .is_some_and(|s| s != "auto")
        {
            return Err(IrError::UnsupportedFeature);
        }
        let effort = options.and_then(|r| r.effort.as_deref());
        let forced = matches!(
            request.generation.tool_choice,
            Some(super::request::ToolChoice::Required | super::request::ToolChoice::Named { .. })
        );
        // Claude thinking does not implement ordinary temperature/top-p control.
        if request.generation.temperature.is_some() || request.generation.top_p.is_some() {
            return Err(IrError::UnsupportedFeature);
        }
        match self {
            Self::ClaudeAdaptive {
                efforts,
                default_effort,
                allow_forced_tools,
                ..
            } => {
                let effort = effort.unwrap_or(default_effort);
                if !efforts.contains(effort) || (forced && !allow_forced_tools) {
                    return Err(IrError::UnsupportedFeature);
                }
                Ok(json!({"thinking":{"type":"adaptive","display":"summarized"},"effort":effort}))
            }
            Self::ClaudeManual {
                budget_tokens,
                effort_budgets,
                ..
            } => {
                let budget = match effort {
                    Some(e) => *effort_budgets.get(e).ok_or(IrError::UnsupportedFeature)?,
                    None => *budget_tokens,
                };
                if budget >= max || forced {
                    return Err(IrError::UnsupportedFeature);
                }
                Ok(
                    json!({"thinking":{"type":"enabled","display":"summarized","budget_tokens":budget}}),
                )
            }
        }
    }
    pub fn beta_header(&self) -> Option<&'static str> {
        match self {
            Self::ClaudeManual {
                interleaved_beta: true,
                ..
            } => Some("interleaved-thinking-2025-05-14"),
            _ => None,
        }
    }
}
