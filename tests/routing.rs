use agent_response_gateway::{
    Config,
    ir::{
        ApiProtocol, IrError,
        capability::{Feature, Support},
    },
    routing::AdmittedRequest,
};
use serde_json::json;

fn declaration(api: &str) -> String {
    format!(
        r#"
[providers.mock]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "SYNTHETIC_KEY"
[models.writer]
provider = "mock"
upstream_model = "actual-model"
api = "{api}"
auth = "api_key"
capability_profile = "tested"
{}
[capability_profiles.tested]
version = "1"
provider = "mock"
upstream_model = "actual-model"
api = "{api}"
context_window = 32000
max_output_tokens = 1024
tested_codex_version = "0.154.0"
[capability_profiles.tested.support]
instructions = "native"
max_output_tokens = "native"
function_tools = "native"
custom_tools = "bridged_custom_tool_json"
"#,
        if api == "messages" {
            "messages_version = \"2023-06-01\""
        } else {
            ""
        }
    )
}

#[test]
fn legacy_default_and_explicit_auth_and_profile_remain_compatible() {
    let config = Config::parse(&declaration("responses")).unwrap();
    let route = config.resolve_route("writer").unwrap();
    assert_eq!(route.endpoint.as_str(), "http://127.0.0.1:1/v1/responses");
    assert_eq!(route.snapshot.context_window, Some(32000));
    assert_eq!(
        route.snapshot.capabilities.support(Feature::Images),
        Support::Unsupported
    );
    assert_eq!(route.tested_codex_version.as_deref(), Some("0.154.0"));
    assert!(config.resolve_route("missing").is_err());
}

#[test]
fn invalid_profiles_fail_startup() {
    let valid = declaration("responses");
    for candidate in [
        valid.replace(
            "capability_profile = \"tested\"",
            "capability_profile = \"missing\"",
        ),
        valid.replace("context_window = 32000", "context_window = 0"),
        valid.replace("max_output_tokens = 1024", "max_output_tokens = 32001"),
        valid.replace(
            "tested_codex_version = \"0.154.0\"",
            "tested_codex_version = \"\"",
        ),
        valid.replace(
            "instructions = \"native\"",
            "instructions = \"bridged_custom_tool_json\"",
        ),
        valid.replace("instructions = \"native\"", "unknown_feature = \"native\""),
        valid.replacen(
            "upstream_model = \"actual-model\"",
            "upstream_model = \"other-model\"",
            1,
        ),
        valid.replacen("api = \"responses\"", "api = \"messages\"", 1),
        valid.replace("auth = \"api_key\"", "auth = \"arbitrary-header\""),
        declaration("messages").replace("auth = \"api_key\"", ""),
        declaration("messages").replace("messages_version = \"2023-06-01\"", ""),
    ] {
        assert!(Config::parse(&candidate).is_err());
    }
}

#[test]
fn resolved_route_does_not_change_with_later_alias_mutation() {
    let mut config = Config::parse(&declaration("responses")).unwrap();
    let route = config.resolve_route("writer").unwrap();
    config.models.get_mut("writer").unwrap().upstream_model = "new-model".into();
    config.providers.get_mut("mock").unwrap().api_key_env = "OTHER_KEY".into();
    assert_eq!(route.snapshot.model, "actual-model");
    assert_eq!(route.snapshot.credential_binding, "SYNTHETIC_KEY");
    assert!(config.validate().is_err());
}

#[test]
fn converted_admission_is_pure_and_rejects_semantic_loss() {
    // Deliberately parse declarations without enabling HTTP. Config::parse refuses dispatch.
    let config: Config = toml::from_str(&declaration("messages")).unwrap();
    let route = config.resolve_route("writer").unwrap();
    assert_eq!(route.snapshot.api, ApiProtocol::Messages);
    assert_eq!(route.endpoint.path(), "/v1/messages");
    let body = json!({"model":"writer", "store":false, "input":"synthetic", "max_output_tokens":1024,
        "client_metadata":{"synthetic":"test"}, "prompt_cache_key":"synthetic-cache",
        "include":["reasoning.encrypted_content"]});
    let AdmittedRequest::Translated { request, plan } =
        route.admit(body.as_object().unwrap().clone()).unwrap()
    else {
        panic!("expected translation plan")
    };
    assert!(request.extensions.fields.is_empty());
    assert_eq!(request.model, "actual-model");
    assert!(plan.required.contains(Feature::MaxOutputTokens));
    for extra in [
        json!({"unknown":"extension"}),
        json!({"max_output_tokens":1025}),
        json!({"max_output_tokens":0}),
        json!({"max_output_tokens":1.2}),
        json!({"include":["unknown"]}),
        json!({"client_metadata":"wrong-type"}),
        json!({"prompt_cache_key":17}),
        json!({"reasoning":{"effort":"high"}}),
        json!({"tools":[{"type":"web_search"}]}),
        json!({"input":[{"type":"function_call_output","call_id":"missing","output":"result"}]}),
        json!({"input":[{"type":"reasoning","encrypted_content":"opaque"}]}),
    ] {
        let mut invalid = body.as_object().unwrap().clone();
        invalid.extend(extra.as_object().unwrap().clone());
        assert!(route.admit(invalid).is_err());
    }
    let chat: Config = toml::from_str(&declaration("chat_completions")).unwrap();
    assert_eq!(
        chat.resolve_route("writer").unwrap().endpoint.path(),
        "/v1/chat/completions"
    );
}

#[test]
fn native_admission_preserves_extensions_but_checks_declared_output_limit() {
    let config = Config::parse(&declaration("responses")).unwrap();
    let route = config.resolve_route("writer").unwrap();
    let body = json!({"model":"writer", "max_output_tokens":1024, "future_extension":{"exact":18446744073709551616_u128}});
    let AdmittedRequest::Native(mut actual) =
        route.admit(body.as_object().unwrap().clone()).unwrap()
    else {
        panic!("expected native passthrough")
    };
    actual.insert("model".into(), json!("writer"));
    assert_eq!(actual, body.as_object().unwrap().clone());
    let mut invalid = body.as_object().unwrap().clone();
    invalid.insert("max_output_tokens".into(), json!(1025));
    assert!(matches!(
        route.admit(invalid),
        Err(IrError::UnsupportedFeature)
    ));
}

#[test]
fn chat_routes_are_explicit_and_require_matching_profiles() {
    let config = Config::parse(&declaration("chat_completions")).unwrap();
    assert_eq!(
        config.resolve_route("writer").unwrap().endpoint.as_str(),
        "http://127.0.0.1:1/v1/chat/completions"
    );
    assert!(
        Config::parse(
            &declaration("chat_completions").replace("capability_profile = \"tested\"", "")
        )
        .is_err()
    );
}
