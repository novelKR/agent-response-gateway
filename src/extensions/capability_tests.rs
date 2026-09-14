use super::*;
use gateway_plugin_contract::{CAPABILITIES_PROTOCOL, PROVIDER_PROTOCOL};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn capabilities(api: &str) -> Value {
    json!({"schema":"gateway-plugin-capabilities/v1", "apis":[api], "features":["json"],
        "requires":["codec_ipc_v3","responses_output_validation"]})
}
fn package() -> Value {
    json!({"schema":CAPABILITIES_PACKAGE_SCHEMA,"id":"synthetic","version":"1.0.0",
        "target":host_target().unwrap(),"protocol":CAPABILITIES_PROTOCOL,
        "permissions":CODEC_PERMISSIONS,"state_schema":"request-memory/v1",
        "files":{"extension":hash(b"inert fixture"),"LICENSE.txt":hash(b"synthetic notice")},
        "capabilities":capabilities("chat_completions")})
}
fn parse(value: &Value) -> Result<Package, ConfigError> {
    let mut raw = canonical(value)?;
    raw.push(b'\n');
    let package: Package = decode(&raw)?;
    package.validate()?;
    Ok(package)
}
fn private(root: &Path, path: &Path) {
    fs::create_dir_all(path).unwrap();
    for p in path.ancestors().take_while(|p| p.starts_with(root)) {
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
fn fixture(value: &Value) -> (tempfile::TempDir, std::path::PathBuf) {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join(".local/capability-tests");
    fs::create_dir_all(&parent).unwrap();
    let temp = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    let root = temp.path();
    let mut bytes = canonical(value).unwrap();
    bytes.push(b'\n');
    let digest = hash(&bytes);
    let directory = root.join("packages/synthetic/1.0.0").join(&digest);
    private(root, &directory);
    private(root, &root.join("state/synthetic").join(&digest));
    for (name, data) in [
        ("extension.json", bytes),
        ("extension", b"inert fixture".to_vec()),
        ("LICENSE.txt", b"synthetic notice".to_vec()),
    ] {
        let path = directory.join(name);
        fs::write(&path, data).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let activation = json!({"schema":LOCK_SCHEMA,"generation":1,"extensions":[{
        "id":"synthetic","version":"1.0.0","package_sha256":digest,"grants":CODEC_PERMISSIONS}]});
    let mut bytes = canonical(&activation).unwrap();
    bytes.push(b'\n');
    let lock = root.join("active.json");
    fs::write(&lock, bytes).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    (temp, lock)
}
fn config() -> String {
    include_str!("../../config.chat.example.toml").replace(
        "api = \"chat_completions\"\nauth",
        "api_codec = \"synthetic\"\napi = \"chat_completions\"\nauth",
    )
}

#[test]
fn legacy_package_null_and_new_fields_do_not_change_v1_acceptance() {
    let mut legacy = package();
    legacy["schema"] = json!(PACKAGE_SCHEMA);
    legacy["protocol"] = json!(gateway_plugin_contract::EDITING_PROTOCOL);
    legacy.as_object_mut().unwrap().remove("capabilities");
    assert!(parse(&legacy).is_ok());
    for field in ["capabilities", "provider_protocol"] {
        let mut changed = legacy.clone();
        changed[field] = Value::Null;
        assert!(parse(&changed).is_err());
    }
    legacy["capabilities"] = capabilities("messages");
    assert!(parse(&legacy).is_err());
    let mut changed = package();
    changed["protocol"] = json!(gateway_plugin_contract::EDITING_PROTOCOL);
    assert!(parse(&changed).is_err());
}

#[test]
fn unsupported_capabilities_and_role_confusion_fail_before_execution() {
    let valid = package();
    assert!(parse(&valid).is_ok());
    for (field, value) in [
        ("features", json!(["json", "network"])),
        (
            "requires",
            json!(["provider_ipc_v1", "responses_output_validation"]),
        ),
        ("apis", json!(["new_vendor"])),
        ("apis", json!([])),
        ("apis", json!(["messages", "messages"])),
        ("apis", json!(["responses", "messages"])),
        ("features", json!(["streaming"])),
        ("features", json!(["json", "json"])),
    ] {
        let mut invalid = valid.clone();
        invalid["capabilities"][field] = value;
        assert!(parse(&invalid).is_err());
    }
    let mut provider = package();
    provider["protocol"] = json!(PROVIDER_PROTOCOL);
    provider["state_schema"] = json!("provider-request-memory/v1");
    provider["provider_protocol"] = json!("synthetic/v1");
    provider["capabilities"]["apis"] = json!([]);
    provider["capabilities"]["requires"] =
        json!(["provider_ipc_v1", "responses_output_validation"]);
    assert!(parse(&provider).is_ok());
    let (_temp, lock) = fixture(&provider);
    assert!(ExtensionPlan::load(&lock).is_err());
}

#[test]
fn frozen_capabilities_bind_v8_manifest_and_route_membership() {
    let value = package();
    let (_temp, lock) = fixture(&value);
    let plan = ExtensionPlan::load(&lock).unwrap();
    let binding = &plan.codec_bindings()["synthetic"];
    assert_eq!(binding.projection()["capabilities"], value["capabilities"]);
    assert!(binding.supports_api(crate::ir::ApiProtocol::ChatCompletions));
    assert!(!binding.supports_api(crate::ir::ApiProtocol::Messages));
    assert!(binding.supports("json"));
    for feature in ["streaming", "editing", "managed_continuation"] {
        assert!(!binding.supports(feature));
    }
    assert_eq!(
        plan.configuration()["schema"],
        "gateway-extension-configuration/v3"
    );
    let config = crate::Config::parse_startup(&config(), None, Some(&plan)).unwrap();
    let base = config.manifest().unwrap();
    assert_eq!(base.schema(), "gateway-embedded-manifest/v8");
    assert_eq!(base.ready_schema(), "gateway-ready/v8");
    let extended = plan.manifest(&serde_json::to_value(base).unwrap()).unwrap();
    assert_eq!(extended["schema"], "gateway-extended-manifest/v8");
    assert_eq!(plan.ready_schema(), "gateway-extended-ready/v8");
    let mut changed = value.clone();
    changed["capabilities"]["features"] = json!(["json", "streaming"]);
    let (_changed_temp, changed_lock) = fixture(&changed);
    let changed_plan = ExtensionPlan::load(&changed_lock).unwrap();
    let changed_config =
        crate::Config::parse_startup(&self::config(), None, Some(&changed_plan)).unwrap();
    let changed_base = changed_config.manifest().unwrap();
    let changed_extended = changed_plan
        .manifest(&serde_json::to_value(changed_base).unwrap())
        .unwrap();
    assert_ne!(
        plan.configuration_sha256(),
        changed_plan.configuration_sha256()
    );
    assert_ne!(
        extended["execution_sha256"],
        changed_extended["execution_sha256"]
    );
    let before = plan.configuration_sha256().to_owned();
    fs::write(&lock, b"invalid next-start state").unwrap();
    assert!(ExtensionPlan::load(&lock).is_err());
    assert_eq!(plan.configuration_sha256(), before);
    assert_eq!(
        plan.codec_bindings()["synthetic"].projection()["capabilities"],
        value["capabilities"]
    );
    let mut wrong = value;
    wrong["capabilities"]["apis"] = json!(["messages"]);
    let (_temp, lock) = fixture(&wrong);
    let other = ExtensionPlan::load(&lock).unwrap();
    assert!(
        crate::Config::parse_startup(&super::capability_tests::config(), None, Some(&other))
            .is_err()
    );
}

#[test]
fn published_package_vectors_match_native_validation() {
    let vectors: Value = serde_json::from_str(include_str!(
        "../../schemas/plugin-capabilities-vectors.json"
    ))
    .unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        if case["schema"] != "gateway-extension-package-v2.schema.json" {
            continue;
        }
        let mut value = case["value"].clone();
        value["target"] = json!(host_target().unwrap());
        assert_eq!(
            parse(&value).is_ok(),
            case["valid"].as_bool().unwrap(),
            "{}",
            case["id"]
        );
    }
}
