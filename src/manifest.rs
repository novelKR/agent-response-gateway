//! Offline normalized configuration report for a host-supervised gateway child.
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Config, ConfigError,
    digest::sha256,
    ir::capability::{BridgeRule, Support},
};

pub const MANIFEST_SCHEMA: &str = "gateway-embedded-manifest/v1";
pub const READY_SCHEMA: &str = "gateway-ready/v1";

#[derive(Serialize)]
struct Package {
    name: &'static str,
    version: &'static str,
}

/// Contains local configuration references, never credential values or provider results.
/// Construct through Config::manifest to bind the immutable projection and digest together.
#[derive(Serialize)]
pub struct EmbeddedManifest {
    schema: &'static str,
    package: Package,
    client_api: &'static str,
    lifecycle: &'static str,
    configuration: Value,
    configuration_sha256: String,
}
impl EmbeddedManifest {
    /// Produce the existing readiness projection from this exact inspected configuration.
    /// Optional hosts and the CLI share the version mapping and address binding.
    pub fn readiness(
        &self,
        bound: std::net::SocketAddr,
        extensions: Option<&crate::extensions::ExtensionPlan>,
    ) -> Result<Value, ConfigError> {
        let configured: std::net::SocketAddr = self.configuration["listen"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| ConfigError("Invalid readiness address binding".into()))?;
        if !bound.ip().is_loopback()
            || bound.port() == 0
            || configured.ip() != bound.ip()
            || (configured.port() != 0 && configured.port() != bound.port())
        {
            return Err(ConfigError("Invalid readiness address binding".into()));
        }
        let mut ready = json!({"event":"ready","address":bound.to_string(),
            "base_url":format!("http://{bound}/v1"),"version":self.package.version,
            "schema":self.ready_schema(),"manifest_schema":self.schema,
            "configuration_sha256":self.configuration_sha256});
        if let Some(plan) = extensions {
            let base = serde_json::to_value(self)
                .map_err(|_| ConfigError("Cannot serialize inspected configuration".into()))?;
            let extended = plan.manifest(&base)?;
            let schema = match extended["schema"].as_str() {
                Some("gateway-extended-manifest/v1") => "gateway-extended-ready/v1",
                Some("gateway-extended-manifest/v2") => "gateway-extended-ready/v2",
                Some("gateway-extended-manifest/v3") => "gateway-extended-ready/v3",
                Some("gateway-extended-manifest/v4") => "gateway-extended-ready/v4",
                Some("gateway-extended-manifest/v5") => "gateway-extended-ready/v5",
                Some("gateway-extended-manifest/v6") => "gateway-extended-ready/v6",
                Some("gateway-extended-manifest/v7") => "gateway-extended-ready/v7",
                _ => return Err(ConfigError("Unsupported readiness schema".into())),
            };
            ready["schema"] = json!(schema);
            ready["manifest_schema"] = extended["schema"].clone();
            ready["execution_sha256"] = extended["execution_sha256"].clone();
        }
        Ok(ready)
    }
    pub fn configuration(&self) -> &Value {
        &self.configuration
    }
    pub fn configuration_sha256(&self) -> &str {
        &self.configuration_sha256
    }
    pub fn schema(&self) -> &'static str {
        self.schema
    }
    pub fn ready_schema(&self) -> &'static str {
        match self.schema {
            "gateway-embedded-manifest/v7" => "gateway-ready/v7",
            "gateway-embedded-manifest/v6" => "gateway-ready/v6",
            "gateway-embedded-manifest/v5" => "gateway-ready/v5",
            "gateway-embedded-manifest/v4" => "gateway-ready/v4",
            "gateway-embedded-manifest/v3" => "gateway-ready/v3",
            _ => READY_SCHEMA,
        }
    }
}
impl Config {
    /// Resolve the same validated route/default rules as serve, without reading any secret.
    pub fn manifest(&self) -> Result<EmbeddedManifest, ConfigError> {
        self.validate()?;
        let mut routes = Vec::new();
        for alias in self.models.keys() {
            let route = self.resolve_route(alias)?;
            let snapshot = &route.snapshot;
            let support: serde_json::Map<String, Value> = snapshot
                .capabilities
                .support
                .iter()
                .filter(|(_, support)| **support != Support::Unsupported)
                .map(|(feature, support)| {
                    let key = serde_json::to_value(feature)
                        .expect("feature enum")
                        .as_str()
                        .expect("feature name")
                        .to_owned();
                    let value = match support {
                        Support::Native => "native",
                        Support::Bridged(BridgeRule::CodeModeTextParts) => {
                            "bridged_code_mode_text_parts"
                        }
                        Support::Bridged(BridgeRule::ChatInstructionEnvelope) => {
                            "bridged_chat_instruction_envelope"
                        }
                        Support::Bridged(BridgeRule::ProviderParallelPermission) => {
                            "bridged_parallel_permission"
                        }
                        Support::Bridged(BridgeRule::GeminiInstructionEnvelope) => {
                            "bridged_gemini_instruction_envelope"
                        }
                        Support::Bridged(BridgeRule::CustomToolJson) => "bridged_custom_tool_json",
                        Support::Bridged(BridgeRule::ToolNamespace) => "bridged_tool_namespace",
                        Support::Bridged(BridgeRule::CodexPatchGrammar) => {
                            "bridged_codex_patch_grammar"
                        }
                        Support::Bridged(BridgeRule::RegisteredGrammarValidation) => {
                            "registered_grammar_output_validation"
                        }
                        Support::Bridged(BridgeRule::MessagesInstructionEnvelope) => {
                            "bridged_instruction_envelope"
                        }
                        Support::Unsupported => unreachable!("unsupported is the omitted default"),
                    };
                    (key, json!(value))
                })
                .collect();
            let mut route_projection = json!({
                "alias":route.alias,"provider_id":snapshot.provider_id,"endpoint":route.endpoint.as_str(),
                "upstream_model":snapshot.model,"api":snapshot.api,"auth":route.auth,
                "api_key_env":self.providers[&snapshot.provider_id].api_key_env,
                "messages_version":route.messages_version,"adapter_version":snapshot.adapter_version,
                "capability_profile":{"id":snapshot.capabilities.id,"version":snapshot.capabilities.version,
                    "protocol":snapshot.capabilities.protocol,"support":support},
                "context_window":snapshot.context_window,"max_output_tokens":snapshot.max_output_tokens,
                "tested_codex_version":route.tested_codex_version,
            });
            if let Some(editing) = &route.editing {
                route_projection["editing"] = json!({"id": self.models[&route.alias].editing_policy, "contract":"gateway-editing-policy/v1", "policy":editing});
            }
            if let Some(id) = &self.models[&route.alias].api_codec {
                route_projection["api_codec"] = self.codecs[id].projection();
            }
            if let Some(packs) = self.route_pack_projection(&self.models[&route.alias]) {
                route_projection["profile_packs"] = packs;
            }
            if let Some(policy) = &route.compatibility {
                let profile_id = self.models[&route.alias]
                    .capability_profile
                    .as_ref()
                    .expect("checked profile");
                route_projection["compatibility"] = json!({
                    "id": policy.id,
                    "policy": policy.policy,
                    "admission": "checked",
                    "on_unsupported": "reject",
                    "provider_support": self.capability_profiles[profile_id].support,
                    "contract": "gateway-tool-compatibility/v1",
                });
            }
            if snapshot.api == crate::ir::ApiProtocol::GeminiInteractions {
                route_projection["wire_contract_sha256"] =
                    json!("5aad40046c3245d672393942f1554e35a293179d145ce8e6e2aad08bbc79ceb4");
                route_projection["wire_contract_version"] = json!("v1");
            }
            // Preserve the original Gemini binding byte for byte.
            if snapshot.api != crate::ir::ApiProtocol::GeminiInteractions && route.managed {
                route_projection["continuation_mode"] = json!("managed");
                route_projection["capability_profile"]["reasoning_contract"] =
                    json!(snapshot.capabilities.reasoning_contract);
                route_projection["wire_contract_version"] = json!("2026-09-12");
                route_projection["wire_contract_sha256"] = json!(crate::continuation::hex(
                    &crate::digest::sha256(include_bytes!("../tests/reasoning/wire-lock.json"))
                ));
            }
            if matches!(
                snapshot.capabilities.reasoning_contract,
                Some(crate::ir::reasoning::ReasoningContract::DeepSeek { .. })
            ) {
                route_projection["wire_supplement_sha256"] =
                    json!(crate::continuation::hex(&crate::digest::sha256(
                        include_bytes!("../tests/reasoning/deepseek-api-lock.json")
                    )));
            }
            if self.models[&route.alias].usage_profile.is_some() {
                route_projection["usage_profile"] =
                    json!(self.resolved_usage_profile(&self.models[&route.alias]));
            }
            routes.push(route_projection);
        }
        // Existing serve requires credentials for every configured provider, including unused ones.
        let credentials: std::collections::BTreeMap<_, _> = self
            .providers
            .iter()
            .map(|(id, p)| (id, &p.api_key_env))
            .collect();
        let mut configuration = sorted(
            json!({"listen":self.listen.to_string(),"source_url":self.source_url,
            "local_token_env":self.local_token_env,"upstream_credential_references":credentials,
            "limits":self.limits,"routes":routes}),
        );
        if let Some(c) = &self.continuation {
            configuration["continuation"] =
                serde_json::to_value(c).expect("continuation configuration");
            configuration["replay_versions"] = json!({"read":[1,2],"write":2});
            configuration = sorted(configuration);
        }
        if let Some(packs) = self.profile_pack_projection() {
            configuration["profile_packs"] = packs;
            configuration = sorted(configuration);
        }
        let bytes = serde_json::to_vec(&configuration)
            .expect("normalized configuration contains JSON values");
        let configuration_sha256 = sha256(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(EmbeddedManifest {
            schema: if self.models.values().any(|m| m.editing_policy.is_some())
                || self
                    .profile_packs
                    .as_ref()
                    .is_some_and(|p| p.editing_contract())
            {
                "gateway-embedded-manifest/v7"
            } else if self.models.values().any(|m| m.api_codec.is_some()) {
                "gateway-embedded-manifest/v6"
            } else if self.profile_packs.is_some() {
                "gateway-embedded-manifest/v5"
            } else if self
                .models
                .values()
                .any(|m| m.compatibility_policy.is_some())
            {
                "gateway-embedded-manifest/v4"
            } else if self.continuation.is_some() {
                "gateway-embedded-manifest/v3"
            } else {
                MANIFEST_SCHEMA
            },
            package: Package {
                name: env!("CARGO_PKG_NAME"),
                version: env!("CARGO_PKG_VERSION"),
            },
            client_api: "responses",
            lifecycle: "host-supervised-process/v1",
            configuration,
            configuration_sha256,
        })
    }
}
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let entries: std::collections::BTreeMap<_, _> = fields.into_iter().collect();
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        value => value,
    }
}
