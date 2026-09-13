use agent_response_gateway::profile_packs::manager as packs;
use gateway_management::{
    Action, Actor, Digest, Error, Grant, Id, Identity, Journal, Request, State,
};
use gateway_management_extensions::{
    Command, Driver, EffectiveSelection, LocalSource, Manager, Registration, Selection,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn temp() -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/package-tests");
    fs::create_dir_all(&parent).unwrap();
    let directory = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    directory
}
fn private(path: &Path) {
    fs::create_dir(path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
fn actor() -> Actor {
    Actor::new(
        Identity {
            subject: id("operator"),
            credential: id("local-management"),
        },
        [
            Action::PackageInstall,
            Action::PackageEnable,
            Action::PackageDisable,
            Action::PackageSelect,
            Action::Reconcile,
        ]
        .map(|action| Grant {
            action,
            target: id("packages"),
        }),
    )
    .unwrap()
}
fn request(manager: &Manager, command: &Command, key: &str) -> Request {
    Request {
        target: id("packages"),
        action: command.action(),
        expected: manager.snapshot().unwrap(),
        idempotency_key: id(key),
        parameters_sha256: command.digest().unwrap(),
    }
}
fn apply(
    manager: &mut Manager,
    journal: &mut Journal,
    command: Command,
    key: &str,
) -> gateway_management::Operation {
    let request = request(manager, &command, key);
    journal
        .execute(&actor(), &request, &mut manager.bind(command))
        .unwrap()
}
fn profile(directory: &Path, version: &str) -> LocalSource {
    let source = directory.join(format!("source-{version}.json"));
    let package = directory.join(format!("package-{version}.json"));
    let value = json!({"schema":"gateway-profile-pack/v1","id":"synthetic","version":version,
        "capabilities":{"functions":{"api":"responses","context_window":8192,"max_output_tokens":2048,
            "tested_codex_version":"synthetic","support":{"function_tools":"native"}}},
        "policies":{},"evidence":[],"notices":{"LICENSE":"Synthetic fixture, no external material."}});
    fs::write(&source, serde_json::to_vec(&value).unwrap()).unwrap();
    let report = packs::run(packs::Command::Package {
        source,
        output: package.clone(),
    })
    .unwrap();
    LocalSource {
        path: package,
        package_sha256: Digest::try_from(report["package_sha256"].as_str().unwrap().to_owned())
            .unwrap(),
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    store: PathBuf,
    evidence: PathBuf,
    journal_path: PathBuf,
    manager: Manager,
    journal: Journal,
    first: Selection,
    second: Selection,
}
impl Fixture {
    fn profile() -> Self {
        let root = temp();
        let store = root.path().join("store");
        let evidence = root.path().join("evidence");
        let journal_path = root.path().join("journal");
        for path in [&store, &evidence, &journal_path] {
            private(path);
        }
        let first = profile(root.path(), "1.0.0");
        let second = profile(root.path(), "2.0.0");
        let one = Selection {
            id: id("synthetic"),
            version: "1.0.0".into(),
            package_sha256: first.package_sha256.clone(),
        };
        let two = Selection {
            id: id("synthetic"),
            version: "2.0.0".into(),
            package_sha256: second.package_sha256.clone(),
        };
        let manager = Manager::initialize(Registration {
            target: id("packages"),
            directory: evidence.clone(),
            store: store.clone(),
            driver: Driver::ProfilePack,
            sources: BTreeMap::from([(id("first"), first), (id("second"), second)]),
            recorder_bindings: BTreeMap::new(),
        })
        .unwrap();
        let journal = Journal::initialize(&journal_path, 8 * 1024 * 1024).unwrap();
        Self {
            _root: root,
            store,
            evidence,
            journal_path,
            manager,
            journal,
            first: one,
            second: two,
        }
    }
    fn enable(&mut self, selection: Selection, key: &str) {
        assert_eq!(
            apply(
                &mut self.manager,
                &mut self.journal,
                Command::Enable {
                    package: selection,
                    grants: vec![],
                    recorder: None
                },
                key
            )
            .state,
            State::Succeeded
        );
    }
}
#[test]
fn profile_selection_preserves_installed_and_effective_versions_and_data() {
    let mut f = Fixture::profile();
    for source in ["first", "second"] {
        assert_eq!(
            apply(
                &mut f.manager,
                &mut f.journal,
                Command::Install { source: id(source) },
                source
            )
            .state,
            State::Succeeded
        );
    }
    f.enable(f.first.clone(), "enable-first");
    let effective = EffectiveSelection {
        instance: id("synthetic-run"),
        observed_at_ms: 1,
        configuration_sha256: Digest::of(b"config"),
        execution_sha256: None,
        packages: vec![f.first.clone()],
    };
    let replacement = Command::Enable {
        package: f.second.clone(),
        grants: vec![],
        recorder: None,
    };
    let req = request(&f.manager, &replacement, "implicit-replacement");
    assert!(matches!(
        f.journal
            .execute(&actor(), &req, &mut f.manager.bind(replacement)),
        Err(Error::Conflict)
    ));
    let select = Command::Select {
        package: f.second.clone(),
        grants: vec![],
        recorder: None,
    };
    let req = request(&f.manager, &select, "select");
    assert!(matches!(
        f.journal
            .execute(&actor(), &req, &mut f.manager.bind(select)),
        Err(Error::Unsupported)
    ));
    fs::write(f.store.join("synthetic-usage"), b"retained usage fixture").unwrap();
    fs::write(
        f.store.join("synthetic-continuation"),
        b"retained continuation fixture",
    )
    .unwrap();
    assert_eq!(
        apply(
            &mut f.manager,
            &mut f.journal,
            Command::Disable {
                package: id("synthetic")
            },
            "disable"
        )
        .state,
        State::Succeeded
    );
    f.enable(f.second.clone(), "enable-second");
    let status = f.manager.status(Some(effective)).unwrap();
    assert_eq!(
        status.store.inventory["activation"]["packs"][0]["version"],
        "2.0.0"
    );
    assert_eq!(status.effective.unwrap().packages[0].version, "1.0.0");
    assert_eq!(
        status.store.inventory["installed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(status.version_change_requires_disable);
    assert!(!status.removal_supported);
    assert_eq!(
        fs::read(f.store.join("synthetic-usage")).unwrap(),
        b"retained usage fixture"
    );
    assert_eq!(
        fs::read(f.store.join("synthetic-continuation")).unwrap(),
        b"retained continuation fixture"
    );
}
#[test]
fn external_mutation_rejects_stale_intent_without_discarding_new_inventory() {
    let mut f = Fixture::profile();
    apply(
        &mut f.manager,
        &mut f.journal,
        Command::Install {
            source: id("first"),
        },
        "install",
    );
    let enable = Command::Enable {
        package: f.first.clone(),
        grants: vec![],
        recorder: None,
    };
    let req = request(&f.manager, &enable, "stale");
    packs::run(packs::Command::Enable {
        store: f.store.clone(),
        id: "synthetic".into(),
        version: "1.0.0".into(),
        sha256: f.first.package_sha256.as_str().into(),
    })
    .unwrap();
    assert!(f.manager.status(None).unwrap().external_change);
    assert!(matches!(
        f.journal
            .execute(&actor(), &req, &mut f.manager.bind(enable)),
        Err(Error::Conflict)
    ));
    assert!(f.manager.status(None).unwrap().effective.is_none());
}
#[test]
fn package_audit_faults_block_effects_and_reconcile_only_complete_evidence() {
    let mut f = Fixture::profile();
    let db = rusqlite::Connection::open(f.journal_path.join("management.sqlite3")).unwrap();
    let command = Command::Install {
        source: id("first"),
    };
    let req = request(&f.manager, &command, "install");
    db.execute_batch("CREATE TRIGGER fail_admit BEFORE INSERT ON operations BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(matches!(
        f.journal
            .execute(&actor(), &req, &mut f.manager.bind(command.clone())),
        Err(Error::Storage)
    ));
    assert_eq!(
        f.manager.status(None).unwrap().store.inventory["installed"],
        json!([])
    );
    db.execute_batch("DROP TRIGGER fail_admit; CREATE TRIGGER fail_finish BEFORE INSERT ON events WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    let Error::Uncertain(op) = f
        .journal
        .execute(&actor(), &req, &mut f.manager.bind(command.clone()))
        .unwrap_err()
    else {
        panic!("must return operation identity")
    };
    db.execute_batch("DROP TRIGGER fail_finish").unwrap();
    let observed = f.manager.snapshot().unwrap();
    let result = f
        .journal
        .reconcile(
            &actor(),
            &id("packages"),
            &op,
            &mut f.manager.bind(command.clone()),
        )
        .unwrap();
    assert_eq!(result.state, State::Succeeded);
    assert_eq!(result.events[2].phase, gateway_management::Phase::Recovery);
    assert_eq!(f.manager.snapshot().unwrap(), observed);
    // Retained receipt contains IDs/digests only, never the package document or notices.
    let evidence = f.evidence.join(format!(
        "{}.json",
        Digest::of(op.as_str().as_bytes()).as_str()
    ));
    let text = fs::read_to_string(evidence).unwrap();
    assert!(!text.contains("Synthetic fixture"));
    assert!(!text.contains("context_window"));
    assert_eq!(
        f.journal
            .execute(&actor(), &req, &mut f.manager.bind(command))
            .unwrap()
            .id,
        op
    );
    db.execute_batch("CREATE TRIGGER fail_finish BEFORE INSERT ON events WHEN NEW.sequence=3 BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    let command = Command::Install {
        source: id("second"),
    };
    let req = request(&f.manager, &command, "lost-evidence");
    let Error::Uncertain(unproven) = f
        .journal
        .execute(&actor(), &req, &mut f.manager.bind(command.clone()))
        .unwrap_err()
    else {
        panic!("missing outcome must remain identifiable")
    };
    let evidence = f.evidence.join(format!(
        "{}.json",
        Digest::of(unproven.as_str().as_bytes()).as_str()
    ));
    fs::remove_file(evidence).unwrap();
    db.execute_batch("DROP TRIGGER fail_finish").unwrap();
    assert_eq!(
        f.journal
            .reconcile(
                &actor(),
                &id("packages"),
                &unproven,
                &mut f.manager.bind(command.clone())
            )
            .unwrap()
            .state,
        State::Uncertain
    );
    assert_eq!(
        f.journal
            .execute(&actor(), &req, &mut f.manager.bind(command))
            .unwrap()
            .state,
        State::Uncertain
    );
    assert_eq!(
        f.manager.status(None).unwrap().store.inventory["installed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn adapter_ownership_and_reopen_invalidate_old_preflight_without_repair() {
    let f = Fixture::profile();
    let registration = || Registration {
        target: id("packages"),
        directory: f.evidence.clone(),
        store: f.store.clone(),
        driver: Driver::ProfilePack,
        sources: BTreeMap::new(),
        recorder_bindings: BTreeMap::new(),
    };
    assert!(matches!(
        Manager::open(registration()),
        Err(Error::AlreadyOwned)
    ));
    let before = f.manager.snapshot().unwrap();
    let reg = registration();
    drop(f.manager);
    let reopened = Manager::open(reg).unwrap();
    assert_ne!(reopened.snapshot().unwrap(), before);
    assert!(reopened.status(None).unwrap().effective.is_none());
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_local_driver_checks_real_packages_and_fresh_grants_without_execution() {
    use gateway_management_extensions::NativeDriver;
    use std::process::Command as Process;
    let root = temp();
    let store = root.path().join("store");
    let evidence = root.path().join("evidence");
    let journal_dir = root.path().join("journal");
    for path in [&store, &evidence, &journal_dir] {
        private(path);
    }
    for name in ["packages", "state"] {
        private(&store.join(name));
    }
    let interpreter =
        std::env::var_os("MANAGEMENT_TEST_PYTHON").unwrap_or_else(|| "python3".into());
    let located = Process::new(interpreter)
        .args([
            "-I",
            "-c",
            "import sys; assert sys.version_info >= (3, 11); print(sys.executable)",
        ])
        .output()
        .unwrap();
    assert!(
        located.status.success(),
        "native fixtures require Python 3.11+; set MANAGEMENT_TEST_PYTHON explicitly"
    );
    let python = PathBuf::from(String::from_utf8(located.stdout).unwrap().trim())
        .canonicalize()
        .unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/extension_manager.py")
        .canonicalize()
        .unwrap();
    let native = NativeDriver {
        python_sha256: Digest::of(&fs::read(&python).unwrap()),
        manager_sha256: Digest::of(&fs::read(&script).unwrap()),
        python: python.clone(),
        manager: script.clone(),
        timeout_ms: 10000,
    };
    let binary = root.path().join("inert-binary");
    let notice = root.path().join("notice");
    fs::write(&binary, b"deliberately not executable package code").unwrap();
    fs::write(&notice, b"Synthetic fixture notice").unwrap();
    let mut sources = BTreeMap::new();
    let mut selections = vec![];
    for (source, version) in [("first", "1.0.0"), ("second", "2.0.0")] {
        let package = root.path().join(source);
        let output = Process::new(&python)
            .args(["-I", "-B"])
            .arg(&script)
            .args(["package", "--binary"])
            .arg(&binary)
            .arg("--license-file")
            .arg(&notice)
            .arg("--output")
            .arg(&package)
            .args([
                "--id",
                "synthetic",
                "--version",
                version,
                "--role",
                "api_codec",
                "--codec-protocol",
                "gateway-api-codec/v2",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let digest =
            Digest::try_from(result["package_sha256"].as_str().unwrap().to_owned()).unwrap();
        sources.insert(
            id(source),
            LocalSource {
                path: package,
                package_sha256: digest.clone(),
            },
        );
        selections.push(Selection {
            id: id("synthetic"),
            version: version.into(),
            package_sha256: digest,
        });
    }
    let mut manager = Manager::initialize(Registration {
        target: id("packages"),
        directory: evidence,
        store: store.clone(),
        driver: Driver::Native(native),
        sources,
        recorder_bindings: BTreeMap::new(),
    })
    .unwrap();
    let mut journal = Journal::initialize(&journal_dir, 8 * 1024 * 1024).unwrap();
    for source in ["first", "second"] {
        assert_eq!(
            apply(
                &mut manager,
                &mut journal,
                Command::Install { source: id(source) },
                source
            )
            .state,
            State::Succeeded
        );
    }
    let bad = Command::Enable {
        package: selections[0].clone(),
        grants: vec![],
        recorder: None,
    };
    let req = request(&manager, &bad, "no-grants");
    assert!(matches!(
        journal.execute(&actor(), &req, &mut manager.bind(bad)),
        Err(Error::Forbidden)
    ));
    let grants = vec![
        "read_model_payload".into(),
        "transform_model_protocol".into(),
    ];
    let select = Command::Select {
        package: selections[0].clone(),
        grants: grants.clone(),
        recorder: None,
    };
    let req = request(&manager, &select, "select-is-not-enable");
    assert!(matches!(
        journal.execute(&actor(), &req, &mut manager.bind(select)),
        Err(Error::NotFound)
    ));
    assert_eq!(
        apply(
            &mut manager,
            &mut journal,
            Command::Enable {
                package: selections[0].clone(),
                grants: grants.clone(),
                recorder: None
            },
            "enable"
        )
        .state,
        State::Succeeded
    );
    let replacement = Command::Enable {
        package: selections[1].clone(),
        grants: grants.clone(),
        recorder: None,
    };
    let req = request(&manager, &replacement, "enable-is-not-select");
    assert!(matches!(
        journal.execute(&actor(), &req, &mut manager.bind(replacement)),
        Err(Error::Conflict)
    ));
    assert_eq!(
        apply(
            &mut manager,
            &mut journal,
            Command::Select {
                package: selections[1].clone(),
                grants: grants.clone(),
                recorder: None
            },
            "upgrade"
        )
        .state,
        State::Succeeded
    );
    assert_eq!(
        apply(
            &mut manager,
            &mut journal,
            Command::Select {
                package: selections[0].clone(),
                grants,
                recorder: None
            },
            "previous"
        )
        .state,
        State::Succeeded
    );
    assert_eq!(
        apply(
            &mut manager,
            &mut journal,
            Command::Disable {
                package: id("synthetic")
            },
            "disable"
        )
        .state,
        State::Succeeded
    );
    let status = manager.status(None).unwrap();
    assert!(status.effective.is_none());
    assert_eq!(
        status.store.inventory["installed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for selection in selections {
        assert!(
            store
                .join("state/synthetic")
                .join(selection.package_sha256.as_str())
                .exists()
        );
    }
}
