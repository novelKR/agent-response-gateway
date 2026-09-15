use super::*;
use crate::extensions::ExtensionPlan;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
const LOCK_SCHEMA: &str = crate::extensions::LOCK_SCHEMA;
const CODEC_PERMISSIONS: [&str; 2] = crate::extensions::CODEC_PERMISSIONS;
fn hash(bytes: &[u8]) -> String {
    crate::continuation::hex(&crate::digest::sha256(bytes))
}
fn canonical(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(value)
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
    include_str!("../config.chat.example.toml")
        .replace("api = \"chat_completions\"", "api = \"plugin\"")
        .replace("auth = \"bearer\"", "provider_plugin = \"synthetic\"\nprovider_protocol = \"synthetic.vendor/v1\"\nprovider_path = \"generate\"\nauth = \"bearer\"")
        .replace("https://your-chat-provider.example/v1", "http://127.0.0.1:12345/vendor/v1")
}
fn package(protocol: &str, bytes: &[u8]) -> Value {
    let target = if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    json!({"schema":"gateway-extension-package/v2","id":"synthetic","version":"1.0.0",
        "target":format!("{target}-{arch}"),"protocol":"gateway-provider/v1","provider_protocol":protocol,
        "permissions":CODEC_PERMISSIONS,"state_schema":"provider-request-memory/v1",
        "files":{"extension":hash(bytes),"LICENSE.txt":hash(b"synthetic notice")},
        "capabilities":{"schema":"gateway-plugin-capabilities/v1","apis":[],"features":["json"],
        "requires":["provider_ipc_v1","responses_output_validation"]}})
}

#[test]
fn provider_host_paths_never_escape_origin_or_prefix() {
    for base in [
        "http://127.0.0.1:1234/vendor/v1",
        "http://[::1]:1234/vendor/v1/",
        "https://synthetic.invalid/vendor/v1",
    ] {
        let provider = Provider {
            base_url: base.into(),
            api_key_env: "SYNTHETIC_KEY".into(),
        };
        let expected = format!("{}/models/a_1/generate-v2", base.trim_end_matches('/'));
        assert_eq!(
            provider
                .plugin_url("models/a_1/generate-v2")
                .unwrap()
                .as_str(),
            expected
        );
        assert!(provider.api_url(ApiProtocol::Plugin).is_err());
        for invalid in [
            "",
            "/generate",
            "//other.invalid/generate",
            "https://other.invalid",
            "../generate",
            "a/../b",
            "./a",
            "a//b",
            "a\\b",
            "%2e%2e/generate",
            "%2E/a",
            "a/%2f/b",
            "%252e%252e/a",
            "generate?q=1",
            "generate#x",
            "한글",
            "a b",
            "a\n",
            "a\0",
        ] {
            assert!(provider.plugin_url(invalid).is_err(), "{invalid:?}");
        }
    }
    for base in [
        "http://synthetic.invalid",
        "http://localhost",
        "https://user:pass@synthetic.invalid",
        "https://synthetic.invalid?q=1",
        "https://synthetic.invalid#x",
        "file:///tmp/a",
    ] {
        assert!(
            Provider {
                base_url: base.into(),
                api_key_env: "SYNTHETIC_KEY".into()
            }
            .plugin_url("generate")
            .is_err()
        );
    }
}

#[test]
fn provider_configuration_is_explicit_frozen_and_activated() {
    let value = package("synthetic.vendor/v1", b"inert fixture");
    let (_temp, lock) = fixture(&value);
    let plan = ExtensionPlan::load(&lock).unwrap();
    // An inert provider entry must not be launched as an Observer at host startup.
    drop(crate::extensions::ExtensionRuntime::start(&plan).unwrap());
    assert!(Config::parse_startup(&config(), None, None).is_err());
    let valid = Config::parse_startup(&config(), None, Some(&plan)).unwrap();
    let model = &valid.models["example/chat"];
    assert_eq!(valid.resolved_usage_profile(model), None);
    assert_eq!(model.resolved_usage_profile(), None);
    let mut legacy_usage = valid.clone();
    legacy_usage
        .models
        .get_mut("example/chat")
        .unwrap()
        .usage_profile = Some(gateway_usage_contract::Profile::ResponsesV1);
    assert!(legacy_usage.validate().is_err());
    let route = valid.resolve_route("example/chat").unwrap();
    assert_eq!(
        route.endpoint.as_str(),
        "http://127.0.0.1:12345/vendor/v1/generate"
    );
    assert_eq!(route.snapshot.api, ApiProtocol::Plugin);
    assert!(matches!(
        route
            .admit(
                json!({"model":"example/chat","input":"test"})
                    .as_object()
                    .unwrap()
                    .clone()
            )
            .unwrap(),
        crate::routing::AdmittedRequest::Translated { .. }
    ));
    let before = serde_json::to_value(valid.manifest().unwrap()).unwrap();
    assert_eq!(before["schema"], "gateway-embedded-manifest/v9");
    let extended = plan.manifest(&before).unwrap();
    assert_eq!(extended["schema"], "gateway-extended-manifest/v9");
    assert!(
        route
            .admit(
                json!({"model":"example/chat","input":"test","stream":true})
                    .as_object()
                    .unwrap()
                    .clone()
            )
            .is_err()
    );
    let mut changed_package = value.clone();
    changed_package["capabilities"]["features"] = json!(["json", "streaming"]);
    let (_other_temp, other_lock) = fixture(&changed_package);
    let other_plan = ExtensionPlan::load(&other_lock).unwrap();
    let other = Config::parse_startup(&config(), None, Some(&other_plan)).unwrap();
    assert_ne!(
        other
            .resolve_route("example/chat")
            .unwrap()
            .snapshot
            .adapter_version,
        route.snapshot.adapter_version
    );
    assert_ne!(
        serde_json::to_value(other.manifest().unwrap()).unwrap()["configuration_sha256"],
        before["configuration_sha256"]
    );
    let vendor = package("second.vendor/v2", b"inert fixture");
    let (_vendor_temp, vendor_lock) = fixture(&vendor);
    let vendor_plan = ExtensionPlan::load(&vendor_lock).unwrap();
    let vendor_config = Config::parse_startup(
        &config().replace("synthetic.vendor/v1", "second.vendor/v2"),
        None,
        Some(&vendor_plan),
    )
    .unwrap();
    assert_eq!(
        vendor_config
            .resolve_route("example/chat")
            .unwrap()
            .snapshot
            .api,
        ApiProtocol::Plugin
    );
    fs::write(&lock, b"not a new startup").unwrap();
    assert_eq!(
        serde_json::to_value(valid.manifest().unwrap()).unwrap(),
        before
    );
    let changed = Config::parse_startup(
        &config().replace("provider_path = \"generate\"", "provider_path = \"next\""),
        None,
        Some(&plan),
    )
    .unwrap();
    assert_ne!(
        serde_json::to_value(changed.manifest().unwrap()).unwrap()["configuration_sha256"],
        before["configuration_sha256"]
    );
    assert_ne!(
        changed
            .resolve_route("example/chat")
            .unwrap()
            .snapshot
            .adapter_version,
        route.snapshot.adapter_version
    );
    for line in [
        "provider_plugin = \"synthetic\"\n",
        "provider_protocol = \"synthetic.vendor/v1\"\n",
        "provider_path = \"generate\"\n",
        "auth = \"bearer\"\n",
        "capability_profile = \"chat-profile\"\n",
    ] {
        assert!(
            Config::parse_startup(&config().replace(line, ""), None, Some(&plan)).is_err(),
            "{line}"
        );
    }
    for replacement in [
        "provider_protocol = \"another/v1\"",
        "provider_plugin = \"missing\"",
        "provider_path = \"../escape\"",
        "usage_profile = \"responses/v1\"",
        "api_codec = \"synthetic\"",
        "continuation_mode = \"managed\"",
        "messages_version = \"date\"",
    ] {
        let mut raw = config();
        let key = replacement.split(" = ").next().unwrap();
        if let Some(line) = raw
            .lines()
            .find(|l| l.starts_with(&format!("{key} = ")))
            .map(str::to_owned)
        {
            raw = raw.replace(&line, replacement);
        } else {
            raw = raw.replace(
                "auth = \"bearer\"",
                &format!("auth = \"bearer\"\n{replacement}"),
            );
        }
        assert!(
            Config::parse_startup(&raw, None, Some(&plan)).is_err(),
            "{replacement}"
        );
    }
}
