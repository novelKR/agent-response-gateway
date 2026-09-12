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
    pub fn configuration(&self) -> &Value {
        &self.configuration
    }
    pub fn configuration_sha256(&self) -> &str {
        &self.configuration_sha256
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
                        Support::Bridged(BridgeRule::GeminiInstructionEnvelope) => {
                            "bridged_gemini_instruction_envelope"
                        }
                        Support::Bridged(BridgeRule::CustomToolJson) => "bridged_custom_tool_json",
                        Support::Bridged(BridgeRule::ToolNamespace) => "bridged_tool_namespace",
                        Support::Bridged(BridgeRule::CodexPatchGrammar) => {
                            "bridged_codex_patch_grammar"
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
            if snapshot.api == crate::ir::ApiProtocol::GeminiInteractions {
                route_projection["wire_contract_sha256"] =
                    json!("5aad40046c3245d672393942f1554e35a293179d145ce8e6e2aad08bbc79ceb4");
                route_projection["wire_contract_version"] = json!("v1");
            }
            if self.models[&route.alias].usage_profile.is_some() {
                route_projection["usage_profile"] =
                    json!(self.models[&route.alias].resolved_usage_profile());
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
        let bytes = serde_json::to_vec(&configuration)
            .expect("normalized configuration contains JSON values");
        let configuration_sha256 = sha256(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(EmbeddedManifest {
            schema: if self.continuation.is_some() {
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
