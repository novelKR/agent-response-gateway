//! Explicit reasoning wire contracts. Model names and endpoint URLs never select a dialect.
use super::{ApiProtocol, IrError, request::RequestIR};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReasoningContract {
    DeepSeek {
        version: u32,
        efforts: BTreeSet<String>,
        default_effort: String,
    },
    OpenRouter {
        version: u32,
        provider_endpoint: String,
        #[serde(default)]
        efforts: BTreeSet<String>,
        default_effort: Option<String>,
        max_tokens: Option<u64>,
        formats: BTreeSet<String>,
    },
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
            Self::DeepSeek {
                version: 1,
                efforts,
                default_effort,
            } => {
                api == ApiProtocol::ChatCompletions
                    && !efforts.is_empty()
                    && efforts.contains(default_effort)
                    && efforts
                        .iter()
                        .all(|e| matches!(e.as_str(), "low" | "high" | "max"))
            }
            Self::OpenRouter {
                version: 1,
                provider_endpoint,
                efforts,
                default_effort,
                max_tokens,
                formats,
            } => {
                api == ApiProtocol::ChatCompletions
                    && provider_endpoint.contains('/')
                    && provider_endpoint.len() <= 128
                    && provider_endpoint
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_/.:".contains(&b))
                    && !formats.is_empty()
                    && formats.iter().all(|f| known_router_format(f))
                    && match max_tokens {
                        Some(n) => *n > 0 && efforts.is_empty() && default_effort.is_none(),
                        None => {
                            !efforts.is_empty()
                                && default_effort.as_ref().is_some_and(|e| efforts.contains(e))
                                && efforts.iter().all(|e| {
                                    matches!(
                                        e.as_str(),
                                        "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                                    )
                                })
                        }
                    }
            }
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
        if matches!(
            self,
            Self::ClaudeAdaptive { .. } | Self::ClaudeManual { .. }
        ) && (request.generation.temperature.is_some() || request.generation.top_p.is_some())
        {
            return Err(IrError::UnsupportedFeature);
        }
        match self {
            Self::DeepSeek {
                efforts,
                default_effort,
                ..
            } => {
                validate_parallel_permission(request)?;
                if max > 393216 {
                    return Err(IrError::UnsupportedFeature);
                }
                let effort = effort.unwrap_or(default_effort);
                if !efforts.contains(effort)
                    || forced
                    || request.generation.temperature.is_some()
                    || request
                        .generation
                        .top_p
                        .as_ref()
                        .is_some_and(|v| !v.as_f64().is_some_and(|n| (0.95..=1.0).contains(&n)))
                {
                    return Err(IrError::UnsupportedFeature);
                }
                if request
                    .generation
                    .output
                    .as_ref()
                    .and_then(|o| o.format.as_ref())
                    .is_some_and(|f| matches!(f, super::request::OutputFormat::JsonSchema { .. }))
                    || request.tool_definitions().any(|t| {
                        matches!(
                            t.kind,
                            super::request::ToolDefinitionKind::Function {
                                strict: Some(true),
                                ..
                            }
                        )
                    })
                {
                    return Err(IrError::UnsupportedFeature);
                }
                Ok(json!({"thinking":{"type":"enabled"},"reasoning_effort":effort}))
            }
            Self::OpenRouter {
                efforts,
                default_effort,
                max_tokens,
                provider_endpoint,
                ..
            } => {
                let mut reasoning = json!({"enabled":true,"exclude":false});
                if let Some(budget) = max_tokens {
                    if effort.is_some() || *budget >= max {
                        return Err(IrError::UnsupportedFeature);
                    }
                    reasoning["max_tokens"] = json!(budget);
                } else {
                    let effort = effort
                        .or(default_effort.as_deref())
                        .ok_or(IrError::UnsupportedFeature)?;
                    if !efforts.contains(effort) {
                        return Err(IrError::UnsupportedFeature);
                    }
                    reasoning["effort"] = json!(effort);
                }
                Ok(
                    json!({"reasoning":reasoning,"provider":{"only":[provider_endpoint],"allow_fallbacks":false,"require_parameters":true}}),
                )
            }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatDialect {
    DeepSeek,
    OpenRouter,
}
impl ReasoningContract {
    pub fn chat_dialect(&self) -> Option<ChatDialect> {
        match self {
            Self::DeepSeek { .. } => Some(ChatDialect::DeepSeek),
            Self::OpenRouter { .. } => Some(ChatDialect::OpenRouter),
            _ => None,
        }
    }
    pub fn accepts_format(&self, format: &str) -> bool {
        matches!(self, Self::OpenRouter{formats,..} if formats.contains(format))
    }
}
fn known_router_format(value: &str) -> bool {
    matches!(
        value,
        "unknown"
            | "openai-responses-v1"
            | "azure-openai-responses-v1"
            | "bedrock-openai-responses-v1"
            | "bedrock-xai-responses-v1"
            | "xai-responses-v1"
            | "meta-responses-v1"
            | "anthropic-claude-v1"
            | "google-gemini-v1"
    )
}

/// Omitting a provider parallel-control field is equivalent only when parallel
/// calls are permitted, or no tool can be called at all (including compaction).
pub fn validate_parallel_permission(request: &RequestIR) -> Result<(), IrError> {
    if request.generation.parallel_tool_calls == Some(false)
        && request.tool_definitions().next().is_some()
        && !matches!(
            request.generation.tool_choice,
            Some(super::request::ToolChoice::None)
        )
    {
        return Err(IrError::UnsupportedFeature);
    }
    Ok(())
}
