use agent_response_gateway::{Config, profile_packs::ProfilePackPlan};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn scratch() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-state");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::tempdir_in(root).unwrap()
}

fn source() -> Value {
    json!({"schema":"gateway-profile-pack/v1","id":"synthetic","version":"1.0.0",
        "capabilities":{"functions":{"api":"responses","context_window":8192,"max_output_tokens":2048,
            "tested_codex_version":"synthetic","support":{"function_tools":"native"}}},
        "policies":{"wrap":{"version":1,"tools":{"custom_input":"function_json","namespaces":"flatten"}}},
        "evidence":[{"description":"Synthetic test declarations only","source_url":null,"artifact_sha256":null}],
        "notices":{"LICENSE":"Synthetic test notice; no external source material."}})
}

fn run(args: &[&str], paths: &[&Path], success: bool) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-response-gateway"));
    command.args(args);
    for path in paths {
        command.arg(path);
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if success {
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    } else {
        assert!(output.stdout.is_empty());
        Value::Null
    }
}

struct Fixture {
    root: tempfile::TempDir,
    store: PathBuf,
    package: PathBuf,
    sha: String,
}
impl Fixture {
    fn new() -> Self {
        let root = scratch();
        let source_path = root.path().join("source.json");
        std::fs::write(&source_path, serde_json::to_vec_pretty(&source()).unwrap()).unwrap();
        let package = root.path().join("pack.json");
        let report = run(
            &[
                "profile-pack",
                "package",
                "--source",
                source_path.to_str().unwrap(),
                "--output",
            ],
            &[&package],
            true,
        );
        let sha = report["package_sha256"].as_str().unwrap().to_string();
        let store = root.path().join("store");
        run(
            &[
                "profile-pack",
                "install",
                "--package",
                package.to_str().unwrap(),
                "--store",
            ],
            &[&store],
            true,
        );
        Self {
            root,
            store,
            package,
            sha,
        }
    }
    fn enable(&self, success: bool) -> Value {
        run(
            &[
                "profile-pack",
                "enable",
                "--store",
                self.store.to_str().unwrap(),
                "--id",
                "synthetic",
                "--version",
                "1.0.0",
                "--sha256",
                &self.sha,
            ],
            &[],
            success,
        )
    }
    fn lock(&self) -> PathBuf {
        self.store.join("active.json")
    }
    fn config(&self) -> Config {
        Config::parse_with_profile_packs(&config(), ProfilePackPlan::load(&self.lock()).unwrap())
            .unwrap()
    }
    fn installed(&self) -> PathBuf {
        self.store
            .join("packages/synthetic/1.0.0")
            .join(format!("{}.json", self.sha))
    }
}

fn config() -> String {
    r#"
[providers.mock]
base_url = "http://127.0.0.1:1/v1"
api_key_env = "PACK_TEST_KEY"
[models.writer]
provider = "mock"
upstream_model = "host-selected-model"
auth = "bearer"
capability_profile = "host-functions"
compatibility_policy = "host-wrap"
[capability_profile_imports.host-functions]
pack = "synthetic"
export = "functions"
provider = "mock"
upstream_model = "host-selected-model"
[compatibility_policy_imports.host-wrap]
pack = "synthetic"
export = "wrap"
"#
    .into()
}

#[test]
fn offline_lifecycle_and_host_binding_are_explicit() {
    let fixture = Fixture::new();
    assert!(!fixture.lock().exists(), "install remains inactive");
    assert!(Config::parse(&config()).is_err());
    fixture.enable(true);
    let idempotent = fixture.enable(true);
    assert_eq!(idempotent["changed"], false);
    assert_eq!(idempotent["activation"]["generation"], 1);
    let inspected = run(
        &["profile-pack", "inspect", "--package"],
        &[&fixture.package],
        true,
    );
    assert_eq!(inspected["package_sha256"], fixture.sha);
    let loaded = fixture.config();
    let route = loaded.resolve_route("writer").unwrap();
    assert_eq!(route.snapshot.model, "host-selected-model");
    assert_eq!(route.endpoint.as_str(), "http://127.0.0.1:1/v1/responses");
    assert!(route.snapshot.adapter_version.contains("profile-packs/1/"));
    let manifest = loaded.manifest().unwrap();
    assert_eq!(manifest.schema(), "gateway-embedded-manifest/v5");
    assert_eq!(manifest.ready_schema(), "gateway-ready/v5");
    assert_eq!(
        manifest.configuration()["profile_packs"]["evidence_status"],
        "publisher_claims_not_attestation"
    );
    let config_path = fixture.root.path().join("gateway.toml");
    std::fs::write(&config_path, config()).unwrap();
    let report = run(
        &[
            "check-config",
            "--config",
            config_path.to_str().unwrap(),
            "--profile-packs-lock",
        ],
        &[&fixture.lock()],
        true,
    );
    assert_eq!(report["profile_packs_executed"], false);
    assert_eq!(report["credentials_checked"], false);
    run(
        &["profile-pack", "disable", "--id", "synthetic", "--store"],
        &[&fixture.store],
        true,
    );
    assert!(fixture.installed().exists());
    assert!(
        Config::parse_with_profile_packs(
            &config(),
            ProfilePackPlan::load(&fixture.lock()).unwrap()
        )
        .is_err()
    );
    // A running process retains its frozen plan even after the administrative lock changes.
    assert_eq!(
        loaded.resolve_route("writer").unwrap().snapshot,
        route.snapshot
    );
}

#[test]
fn imports_never_override_or_relax_host_bindings() {
    let fixture = Fixture::new();
    fixture.enable(true);
    for raw in [
        config().replace("export = \"functions\"", "export = \"missing\""),
        config().replace("pack = \"synthetic\"", "pack = \"inactive\""),
        config().replace("export = \"functions\"", "export = \"wrap\""),
        config().replace(
            "upstream_model = \"host-selected-model\"\n[compatibility",
            "upstream_model = \"different-model\"\n[compatibility",
        ),
        config() + "\n[compatibility_policies.host-wrap]\nversion=1\n",
        config()
            + "\n[capability_profile_imports.host-functions.support]\nfunction_tools=\"native\"\n",
    ] {
        assert!(
            Config::parse_with_profile_packs(&raw, ProfilePackPlan::load(&fixture.lock()).unwrap())
                .is_err()
        );
    }
    let mut loaded = fixture.config();
    loaded
        .capability_profiles
        .get_mut("host-functions")
        .unwrap()
        .context_window += 1;
    assert!(loaded.validate().is_err());
    assert!(loaded.resolve_route("writer").is_err());
}

#[test]
fn tampered_and_noncanonical_bytes_fail_before_startup() {
    let fixture = Fixture::new();
    fixture.enable(true);
    let original = std::fs::read(fixture.installed()).unwrap();
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["capabilities"]["functions"]["max_output_tokens"] = json!(4096);
    std::fs::write(fixture.installed(), serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(ProfilePackPlan::load(&fixture.lock()).is_err());
    std::fs::write(fixture.installed(), &original).unwrap();
    let lock = std::fs::read(fixture.lock()).unwrap();
    for raw in [
        String::from_utf8(lock.clone())
            .unwrap()
            .replace("\"generation\":1", "\"generation\":1,\"generation\":1"),
        String::from_utf8(lock.clone())
            .unwrap()
            .replace("\"1.0.0\"", "\"../escape\""),
        String::from_utf8(lock.clone())
            .unwrap()
            .replace("\"generation\":1", "\"generation\":1,\"grants\":[]"),
        String::from_utf8(lock.clone())
            .unwrap()
            .trim_end()
            .to_string(),
    ] {
        std::fs::write(fixture.lock(), raw).unwrap();
        assert!(ProfilePackPlan::load(&fixture.lock()).is_err());
    }
}

#[test]
fn version_and_digest_changes_bind_origin_but_generation_does_not() {
    let fixture = Fixture::new();
    fixture.enable(true);
    let before = fixture.config();
    let before_manifest = before.manifest().unwrap();
    let route = before.resolve_route("writer").unwrap();
    let mut lock: Value = serde_json::from_slice(&std::fs::read(fixture.lock()).unwrap()).unwrap();
    lock["generation"] = json!(2);
    std::fs::write(
        fixture.lock(),
        format!("{}\n", serde_json::to_string(&lock).unwrap()),
    )
    .unwrap();
    let after = fixture.config();
    assert_eq!(
        route.snapshot,
        after.resolve_route("writer").unwrap().snapshot
    );
    assert_ne!(
        before_manifest.configuration_sha256(),
        after.manifest().unwrap().configuration_sha256()
    );
    // Publisher metadata is also pinned: equal exported capabilities do not hide different bytes.
    let mut source = source();
    source["evidence"][0]["description"] = json!("Updated synthetic declaration");
    let input = fixture.root.path().join("changed-source.json");
    let output = fixture.root.path().join("changed.json");
    std::fs::write(&input, source.to_string()).unwrap();
    let report = run(
        &[
            "profile-pack",
            "package",
            "--source",
            input.to_str().unwrap(),
            "--output",
        ],
        &[&output],
        true,
    );
    let sha = report["package_sha256"].as_str().unwrap();
    run(
        &[
            "profile-pack",
            "install",
            "--package",
            output.to_str().unwrap(),
            "--store",
        ],
        &[&fixture.store],
        true,
    );
    run(
        &[
            "profile-pack",
            "enable",
            "--store",
            fixture.store.to_str().unwrap(),
            "--id",
            "synthetic",
            "--version",
            "1.0.0",
            "--sha256",
            sha,
        ],
        &[],
        false,
    );
    run(
        &["profile-pack", "disable", "--id", "synthetic", "--store"],
        &[&fixture.store],
        true,
    );
    run(
        &[
            "profile-pack",
            "enable",
            "--store",
            fixture.store.to_str().unwrap(),
            "--id",
            "synthetic",
            "--version",
            "1.0.0",
            "--sha256",
            sha,
        ],
        &[],
        true,
    );
    let rebound = fixture.config().resolve_route("writer").unwrap();
    assert_eq!(route.snapshot.capabilities, rebound.snapshot.capabilities);
    assert_ne!(route.snapshot, rebound.snapshot);
    let origin = agent_response_gateway::ir::continuity::ContinuityBinding {
        route: route.snapshot,
        scope: "session".into(),
    };
    let opaque = agent_response_gateway::ir::continuity::OpaqueState::new(
        origin.clone(),
        "fixture",
        vec![1],
    )
    .unwrap();
    let mut new_origin = origin;
    new_origin.route = rebound.snapshot;
    assert!(opaque.replay(&new_origin, "fixture").is_err());
}

#[test]
fn package_source_rejects_code_host_identity_and_ambiguous_json() {
    let fixture = Fixture::new();
    for (index, (key, value)) in [
        ("entrypoint", json!("extension")),
        ("base_url", json!("https://example.com")),
        ("auth", json!("bearer")),
        ("files", json!({"../escape":"x"})),
    ]
    .into_iter()
    .enumerate()
    {
        let mut source = source();
        source[key] = value;
        let path = fixture.root.path().join(format!("invalid-{index}.json"));
        std::fs::write(&path, source.to_string()).unwrap();
        run(
            &[
                "profile-pack",
                "package",
                "--source",
                path.to_str().unwrap(),
                "--output",
            ],
            &[&fixture.root.path().join("absent.json")],
            false,
        );
    }
    let path = fixture.root.path().join("duplicate.json");
    std::fs::write(
        &path,
        source().to_string().replacen(
            "\"id\":\"synthetic\"",
            "\"id\":\"synthetic\",\"id\":\"synthetic\"",
            1,
        ),
    )
    .unwrap();
    run(
        &[
            "profile-pack",
            "package",
            "--source",
            path.to_str().unwrap(),
            "--output",
        ],
        &[&fixture.root.path().join("absent.json")],
        false,
    );
    assert!(!fixture.root.path().join("absent.json").exists());
}

#[test]
fn interrupted_writer_and_oversized_data_fail_without_activation_change() {
    let fixture = Fixture::new();
    fixture.enable(true);
    let before = std::fs::read(fixture.lock()).unwrap();
    std::fs::write(
        fixture.store.join("activation.writer"),
        b"synthetic interrupted writer",
    )
    .unwrap();
    run(
        &["profile-pack", "disable", "--id", "synthetic", "--store"],
        &[&fixture.store],
        false,
    );
    assert_eq!(std::fs::read(fixture.lock()).unwrap(), before);
    std::fs::write(fixture.installed(), vec![b' '; 262_145]).unwrap();
    assert!(ProfilePackPlan::load(&fixture.lock()).is_err());
}

#[cfg(unix)]
#[test]
fn package_links_are_rejected() {
    let fixture = Fixture::new();
    fixture.enable(true);
    std::fs::remove_file(fixture.installed()).unwrap();
    std::os::unix::fs::symlink(&fixture.package, fixture.installed()).unwrap();
    assert!(ProfilePackPlan::load(&fixture.lock()).is_err());
}
