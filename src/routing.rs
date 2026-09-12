//! One request's declared route and admission. No history binding or provider probe.
use serde_json::{Map, Value};
use url::Url;

use crate::{
    Config, ConfigError,
    config::UpstreamAuth,
    ir::{
        ApiProtocol, IrError,
        capability::{CapabilityProfile, TranslationPlan, plan_translation_with_history},
        continuity::{ContinuityBinding, RouteSnapshot},
        request::RequestIR,
        responses,
    },
};

#[derive(Clone, Debug)]
pub struct ResolvedRoute {
    pub compatibility: Option<crate::compatibility::BoundPolicy>,
    pub managed: bool,
    pub alias: String,
    pub endpoint: Url,
    pub auth: UpstreamAuth,
    pub messages_version: Option<String>,
    pub snapshot: RouteSnapshot,
    pub tested_codex_version: Option<String>,
}

pub enum AdmittedRequest {
    Native(Map<String, Value>),
    Translated {
        request: Box<RequestIR>,
        plan: Box<TranslationPlan>,
    },
}

impl Config {
    /// Freeze declarations only; this does not qualify an adapter or bind persisted state.
    pub fn resolve_route(&self, alias: &str) -> Result<ResolvedRoute, ConfigError> {
        let model = self
            .models
            .get(alias)
            .ok_or_else(|| ConfigError("Model is not registered".into()))?;
        self.validate_route(model)?;
        let provider = self
            .providers
            .get(&model.provider)
            .ok_or_else(|| ConfigError("Unknown provider".into()))?;
        let profile = model
            .capability_profile
            .as_ref()
            .map(|id| (id, &self.capability_profiles[id]));
        let compatibility =
            model
                .compatibility_policy
                .as_ref()
                .map(|id| crate::compatibility::BoundPolicy {
                    id: id.clone(),
                    policy: self.compatibility_policies[id].clone(),
                });
        let mut snapshot = RouteSnapshot {
            provider_id: model.provider.clone(),
            model: model.upstream_model.clone(),
            api: model.api,
            // A per-request reference only. Not a credential generation or resume proof.
            credential_binding: provider.api_key_env.clone(),
            adapter_version: "1".into(),
            capabilities: profile.map_or_else(
                || CapabilityProfile {
                    reasoning_contract: None,
                    id: "unqualified-passthrough".into(),
                    version: "1".into(),
                    protocol: model.api,
                    support: Default::default(),
                },
                |(id, p)| p.capabilities(id),
            ),
            context_window: profile.map(|(_, p)| p.context_window),
            max_output_tokens: profile.map(|(_, p)| p.max_output_tokens),
        };
        if let Some(binding) = &compatibility {
            snapshot.capabilities = binding.policy.apply(&snapshot.capabilities)?;
            snapshot.adapter_version = binding.adapter_version();
        }
        if let Some(packs) = self.route_pack_projection(model) {
            snapshot.adapter_version = format!(
                "{}/profile-packs/1/{}",
                snapshot.adapter_version,
                crate::continuation::hex(&crate::digest::sha256(
                    &serde_json::to_vec(&packs).expect("pack binding")
                ))
            );
        }
        snapshot
            .validate()
            .map_err(|_| ConfigError("Invalid route snapshot".into()))?;
        Ok(ResolvedRoute {
            compatibility,
            managed: model.continuation_mode.unwrap_or(
                if model.api == ApiProtocol::GeminiInteractions {
                    crate::config::ContinuationMode::Managed
                } else {
                    crate::config::ContinuationMode::Stateless
                },
            ) == crate::config::ContinuationMode::Managed,
            alias: alias.into(),
            endpoint: provider.api_url(model.api)?,
            auth: model.auth.unwrap_or_default(),
            messages_version: model.messages_version.clone(),
            snapshot,
            tested_codex_version: profile.map(|(_, p)| p.tested_codex_version.clone()),
        })
    }
}

impl ResolvedRoute {
    /// Admit an HTTP payload after shared stateless normalization.
    pub fn admit(&self, payload: Map<String, Value>) -> Result<AdmittedRequest, IrError> {
        self.admit_verified(
            payload,
            &crate::ir::continuity::VerifiedProviderHistory::default(),
        )
    }
    pub(crate) fn admit_verified(
        &self,
        mut payload: Map<String, Value>,
        history: &crate::ir::continuity::VerifiedProviderHistory,
    ) -> Result<AdmittedRequest, IrError> {
        if let Some(limit) = self.snapshot.max_output_tokens
            && let Some(value) = payload.get("max_output_tokens")
        {
            let requested = value
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or(IrError::InvalidField("max_output_tokens"))?;
            if requested > limit {
                return Err(IrError::UnsupportedFeature);
            }
        }
        payload.insert("model".into(), Value::String(self.snapshot.model.clone()));
        if self.snapshot.api == ApiProtocol::Responses && self.compatibility.is_none() {
            return Ok(AdmittedRequest::Native(payload));
        }
        if self.snapshot.api == ApiProtocol::Responses {
            let request = responses::decode(Value::Object(payload), None)?;
            let target = ContinuityBinding {
                route: self.snapshot.clone(),
                scope: "stateless-request".into(),
            };
            let plan = plan_translation_with_history(&request, &target, history)?;
            return Ok(AdmittedRequest::Translated {
                request: Box::new(request),
                plan: Box::new(plan),
            });
        }
        // Complete, explicit exception list for nonsemantic transport/output hints.
        if let Some(metadata) = payload.remove("client_metadata")
            && !metadata
                .as_object()
                .is_some_and(|m| m.values().all(Value::is_string))
        {
            return Err(IrError::InvalidField("client_metadata"));
        }
        if let Some(key) = payload.remove("prompt_cache_key")
            && !key.is_string()
        {
            return Err(IrError::InvalidField("prompt_cache_key"));
        }
        if let Some(include) = payload.remove("include")
            && !include.as_array().is_some_and(|values| {
                values
                    .iter()
                    .all(|v| v.as_str() == Some("reasoning.encrypted_content"))
            })
        {
            return Err(IrError::UnsupportedExtension);
        }
        // No origin is supplied: a converted request may not import opaque history.
        let request = responses::decode(Value::Object(payload), None)?;
        let target = ContinuityBinding {
            route: self.snapshot.clone(),
            scope: "stateless-request".into(),
        };
        let plan = plan_translation_with_history(&request, &target, history)?;
        Ok(AdmittedRequest::Translated {
            request: Box::new(request),
            plan: Box::new(plan),
        })
    }
}
