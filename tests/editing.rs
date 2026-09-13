use agent_response_gateway::editing::ContextEdit;
use serde_json::json;

const POLICY: &str = r#"
[editing_policies.context]
version=1
client_contract="codex-direct-custom/v1"
representation="context-lines/v1"
patch_dialect="codex-patch/1"
normalization="none"
"#;

#[test]
fn explicit_policy_binds_manifest_and_request_plan_without_adding_absent_tools() {
    use agent_response_gateway::{Config, routing::AdmittedRequest};
    let source = include_str!("../config.chat.example.toml");
    let plain = Config::parse(source).unwrap();
    let defined = Config::parse(&format!("{source}{POLICY}")).unwrap();
    assert_eq!(
        plain.manifest().unwrap().configuration_sha256(),
        defined.manifest().unwrap().configuration_sha256()
    );
    let enabled = Config::parse(&format!(
        "{}{POLICY}",
        source.replacen(
            "capability_profile =",
            "editing_policy = \"context\"\ncapability_profile =",
            1
        )
    ))
    .unwrap();
    let mut typed = plain.clone();
    typed.editing_policies = defined.editing_policies.clone();
    typed.models.get_mut("example/chat").unwrap().editing_policy = Some("context".into());
    typed.validate().unwrap();
    assert_eq!(
        typed.manifest().unwrap().configuration_sha256(),
        enabled.manifest().unwrap().configuration_sha256()
    );
    let manifest = enabled.manifest().unwrap();
    assert_eq!(manifest.schema(), "gateway-embedded-manifest/v7");
    assert_ne!(
        plain.manifest().unwrap().configuration_sha256(),
        manifest.configuration_sha256()
    );
    let route = enabled.resolve_route("example/chat").unwrap();
    let AdmittedRequest::Translated { request, plan } = route
        .admit(
            json!({"model":"example/chat","input":"synthetic"})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap()
    else {
        panic!("converted request")
    };
    assert!(request.tools.is_none());
    assert!(plan.editing.is_some());
    assert!(
        Config::parse(&format!(
            "{source}{}",
            POLICY.replace("version=1", "version=2")
        ))
        .is_err()
    );
}

fn edit() -> ContextEdit {
    ContextEdit {
        path: "file.txt".into(),
        before_context: vec!["context".into()],
        old_lines: vec!["old".into()],
        new_lines: vec!["new".into()],
        after_context: vec!["after".into()],
    }
}

#[test]
fn helper_wrapper_serializes_data_and_only_inverts_the_exact_program() {
    use agent_response_gateway::editing::{helper_patch, helper_program};
    let value = ContextEdit {
        new_lines: vec!["\"; throw new Error('must remain data'); // ${x} `한글`".into()],
        ..edit()
    };
    let patch = value.compile().unwrap();
    let program = helper_program(&patch).unwrap();
    assert_eq!(helper_patch(&program).unwrap(), patch);
    for ordinary in [
        format!("// comment\n{program}"),
        format!("{program}\n"),
        "const value = '*** Begin Patch'; text(value);".into(),
        program.replace("tools.apply_patch", "tools.other"),
    ] {
        assert!(helper_patch(&ordinary).is_err());
    }
}

#[test]
fn code_mode_results_preserve_order_and_never_infer_helper_status() {
    use agent_response_gateway::editing::{code_mode_result, code_mode_result_parts};
    let parts = json!([{"type":"input_text","text":"program metadata"},{"type":"input_text","text":"{\"status\":\"failed\"}\n한글"}]);
    let encoded = code_mode_result(&parts).unwrap();
    assert_eq!(code_mode_result_parts(&encoded).unwrap(), parts);
    for bad in [
        json!("completed"),
        json!([{"type":"input_image","image_url":"https://example.invalid"}]),
        json!([{"type":"input_text","text":"ok","success":true}]),
    ] {
        assert!(code_mode_result(&bad).is_err());
    }
    assert!(code_mode_result_parts(&format!(" {encoded}")).is_err());
}

#[test]
fn code_mode_requires_an_explicit_host_descriptor() {
    use agent_response_gateway::editing::Policy;
    let raw = json!({"version":1,"client_contract":"codex-code-mode/v1","representation":"context-lines/v1","patch_dialect":"codex-patch/1","normalization":"none"});
    let policy: Policy = serde_json::from_value(raw.clone()).unwrap();
    assert!(policy.validate().is_err());
    let mut valid = raw;
    valid["client_descriptor_sha256"] = json!("a".repeat(64));
    let policy: Policy = serde_json::from_value(valid.clone()).unwrap();
    policy.validate().unwrap();
    valid["client_contract"] = json!("codex-direct-custom/v1");
    assert!(
        serde_json::from_value::<Policy>(valid)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn canonical_context_preserves_unicode_whitespace_and_round_trips() {
    for before in [vec![], vec!["".into(), " 한글\t".into()]] {
        for new_lines in [
            vec![],
            vec!["".into()],
            vec!["`quoted` \\".into(), "  new\t".into()],
        ] {
            let value = ContextEdit {
                before_context: before.clone(),
                new_lines,
                ..edit()
            };
            let patch = value.compile().unwrap();
            assert_eq!(ContextEdit::from_patch(&patch).unwrap(), value);
            assert_eq!(
                ContextEdit::from_json(&serde_json::to_string(&value).unwrap()).unwrap(),
                value
            );
        }
    }
}

#[test]
fn rejects_duplicate_fields_unknown_fields_delimiters_and_noops() {
    let mut value = serde_json::to_value(edit()).unwrap();
    value["extra"] = json!(true);
    assert!(ContextEdit::from_json(&value.to_string()).is_err());
    let duplicate =
        serde_json::to_string(&edit())
            .unwrap()
            .replacen('{', "{\"path\":\"other\",", 1);
    assert!(ContextEdit::from_json(&duplicate).is_err());
    for path in ["", "file\n*** Delete File: other", " file", "file\0"] {
        assert!(
            ContextEdit {
                path: path.into(),
                ..edit()
            }
            .compile()
            .is_err()
        );
    }
    assert!(
        ContextEdit {
            new_lines: vec!["old".into()],
            ..edit()
        }
        .compile()
        .is_err()
    );
    assert!(
        ContextEdit {
            old_lines: vec![],
            ..edit()
        }
        .compile()
        .is_err()
    );
    for line in ["a\nb", "a\rb", "a\0b"] {
        assert!(
            ContextEdit {
                new_lines: vec![line.into()],
                ..edit()
            }
            .compile()
            .is_err()
        );
    }
}

#[test]
fn inverse_never_accepts_noncanonical_or_multi_file_history() {
    let patch = edit().compile().unwrap();
    for malformed in [
        format!("{patch}\n"),
        patch.replace("@@\n", "@@ context\n"),
        patch.replace("*** End Patch", "*** Delete File: other\n*** End Patch"),
        patch.replace("+new", "+new\n-old"),
    ] {
        assert!(ContextEdit::from_patch(&malformed).is_err());
    }
}
