use agent_response_gateway::{Config, manifest::MANIFEST_SCHEMA};
use serde_json::{Value, json};

fn raw() -> &'static str {
    r#"
[providers.mock]
base_url="https://example.test/v1/"
api_key_env="SYNTHETIC_KEY"
[models.writer]
provider="mock"
upstream_model="synthetic-모형"
"#
}
fn document(config: &Config) -> Value {
    serde_json::to_value(config.manifest().unwrap()).unwrap()
}
#[test]
fn manifest_normalizes_defaults_endpoint_and_field_order_without_credentials() {
    let original = Config::parse(raw()).unwrap();
    let explicit = Config::parse(
        r#"
local_token_env="ARG_LOCAL_TOKEN"
listen="127.0.0.1:0"
[models.writer]
upstream_model="synthetic-모형"
provider="mock"
api="responses"
auth="bearer"
[providers.mock]
api_key_env="SYNTHETIC_KEY"
base_url="https://example.test:443/v1"
"#,
    )
    .unwrap();
    let a = document(&original);
    let b = document(&explicit);
    assert_eq!(a, b);
    assert_eq!(a["schema"], MANIFEST_SCHEMA);
    assert_eq!(a["client_api"], "responses");
    assert_eq!(a["lifecycle"], "host-supervised-process/v1");
    assert_eq!(
        a["configuration"]["routes"][0]["endpoint"],
        "https://example.test/v1/responses"
    );
    assert_eq!(
        a["configuration"]["routes"][0]["capability_profile"]["id"],
        "unqualified-passthrough"
    );
    assert!(a["configuration"]["routes"][0]["tested_codex_version"].is_null());
    assert_eq!(a["configuration"]["limits"]["max_response_bytes"], 16777216);
    assert_eq!(
        a["configuration"]["upstream_credential_references"]["mock"],
        "SYNTHETIC_KEY"
    );
    let digest = a["configuration_sha256"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
}
#[test]
fn effective_route_auth_credential_reference_and_limit_changes_change_digest() {
    let original = Config::parse(raw()).unwrap();
    let expected = original
        .manifest()
        .unwrap()
        .configuration_sha256()
        .to_owned();
    for change in 0..6 {
        let mut changed = original.clone();
        match change {
            0 => {
                changed.providers.get_mut("mock").unwrap().base_url =
                    "https://example.test/v2".into()
            }
            1 => {
                changed.models.get_mut("writer").unwrap().upstream_model = "different-model".into()
            }
            2 => {
                changed.models.get_mut("writer").unwrap().auth =
                    Some(agent_response_gateway::config::UpstreamAuth::ApiKey)
            }
            3 => changed.providers.get_mut("mock").unwrap().api_key_env = "OTHER_KEY".into(),
            4 => changed.limits.stream_idle_timeout_ms += 1,
            _ => changed.listen = "127.0.0.1:12345".parse().unwrap(),
        }
        assert_ne!(changed.manifest().unwrap().configuration_sha256(), expected);
    }
    let mut invalid = original;
    invalid.listen = "0.0.0.0:0".parse().unwrap();
    assert!(invalid.manifest().is_err());
}
#[test]
fn profile_support_and_alias_order_use_resolved_defaults() {
    let raw = include_str!("../config.messages.example.toml");
    let config = Config::parse(raw).unwrap();
    let mut explicit = config.clone();
    explicit
        .capability_profiles
        .get_mut("messages-profile")
        .unwrap()
        .support
        .insert(
            agent_response_gateway::ir::capability::Feature::Images,
            agent_response_gateway::config::DeclaredSupport::Unsupported,
        );
    assert_eq!(document(&config), document(&explicit));
    let value = document(&config);
    assert_eq!(
        value["configuration"]["routes"][0]["capability_profile"]["support"]["custom_grammar"],
        "bridged_codex_patch_grammar"
    );
    let original_digest = config.manifest().unwrap().configuration_sha256().to_owned();
    explicit
        .capability_profiles
        .get_mut("messages-profile")
        .unwrap()
        .version = "2".into();
    assert_ne!(
        explicit.manifest().unwrap().configuration_sha256(),
        original_digest
    );
    let mut multiple = config;
    let model = multiple.models["example/messages"].clone();
    multiple.models.insert("aaa".into(), model.clone());
    multiple.models.insert("zzz".into(), model);
    let manifest = document(&multiple);
    let aliases: Vec<_> = manifest["configuration"]["routes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["alias"].clone())
        .collect();
    assert_eq!(
        aliases,
        json!(["aaa", "example/messages", "zzz"])
            .as_array()
            .unwrap()
            .clone()
    );
}
