use super::{contract::*, usage::*};
use serde_json::{Value, json};
fn snapshot(values: Value) -> UsageSnapshot {
    let mut counters = serde_json::Map::new();
    for field in gateway_usage_contract::FIELDS {
        counters.insert(
            field.into(),
            values
                .get(field)
                .map(|v| json!({"source":"reported","value":v}))
                .unwrap_or_else(|| json!({"source":"not_reported"})),
        );
    }
    serde_json::from_value(json!({"kind":"observed","counters":counters})).unwrap()
}
#[test]
fn unknown_zero_exact_numbers_and_invalid_arithmetic_are_distinct() {
    let unknown = validate(&UsageSnapshot::Unobserved).unwrap();
    assert_eq!(unknown.value("input_tokens"), None);
    let zero = validate(&snapshot(json!({"input_tokens":0,"output_tokens":0}))).unwrap();
    assert_eq!(zero.value("input_tokens"), Some(0));
    assert_eq!(
        zero.counters["total_tokens"].source,
        gateway_usage_contract::Source::Derived
    );
    assert!(
        verify_response(
            &json!({"usage":{"input_tokens":0,"output_tokens":0,"total_tokens":0}}),
            &unknown
        )
        .is_err()
    );
    assert!(verify_response(&json!({"usage":null}), &zero).is_err());
    assert_eq!(
        validate(&snapshot(json!({"input_tokens":9007199254740993_u64})))
            .unwrap()
            .value("input_tokens"),
        Some(9007199254740993)
    );
    for values in [
        json!({"input_tokens":u64::MAX,"output_tokens":1}),
        json!({"input_tokens":2,"output_tokens":1,"total_tokens":4}),
        json!({"input_tokens":2,"cache_read_input_tokens":3}),
        json!({"output_tokens":1,"reasoning_output_tokens":2}),
        json!({"input_tokens":10,"input_regular_tokens":8,"cache_read_input_tokens":8}),
        json!({"input_tokens":10,"cache_read_input_tokens":8,"cache_write_input_tokens":8}),
    ] {
        assert!(validate(&snapshot(values)).is_err());
    }
    assert!(cumulative(&zero, &unknown).is_err());
    assert!(
        cumulative(
            &validate(&snapshot(json!({"input_tokens":3}))).unwrap(),
            &validate(&snapshot(json!({"input_tokens":2}))).unwrap()
        )
        .is_err()
    );
    for value in [json!(-1), json!(1.5), json!("1"), json!(null)] {
        assert!(
            serde_json::from_value::<Counter>(json!({"source":"reported","value":value})).is_err()
        );
    }
    assert!(serde_json::from_value::<Counter>(json!({"source":"derived","value":1})).is_err());
}
#[cfg(unix)]
mod native {
    use super::*;
    use crate::{
        ir::{
            ApiProtocol,
            capability::{CapabilityProfile, Feature, Support, plan_translation},
            continuity::{ContinuityBinding, RouteSnapshot},
            responses,
        },
        provider_plugins::{Binding, execution::PreparedProvider},
    };
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
    };
    fn binding() -> Binding {
        Binding {protocol:gateway_plugin_contract::PROVIDER_PROTOCOL.into(),provider_protocol:"synthetic/v1".into(),id:"synthetic".into(),version:"1.0.0".into(),package_sha256:"a".repeat(64),executable_sha256:"b".repeat(64),executable:"/nonexistent".into(),directory:"/nonexistent".into(),owner:0,
            capabilities:serde_json::from_value(json!({"schema":"gateway-plugin-capabilities/v1","apis":[],"features":["json","streaming"],"requires":["provider_ipc_v1","responses_output_validation"]})).unwrap()}
    }
    fn plan(
        streaming: bool,
        tools: bool,
    ) -> (
        crate::ir::request::RequestIR,
        crate::ir::capability::TranslationPlan,
    ) {
        let mut raw = json!({"model":"synthetic","input":"test","stream":streaming});
        if tools {
            raw["tools"] =
                json!([{"type":"function","name":"approved","parameters":{"type":"object"}}]);
        }
        let request = responses::decode(raw, None).unwrap();
        let mut support = BTreeMap::new();
        if tools {
            support.insert(Feature::FunctionTools, Support::Native);
        }
        let route = RouteSnapshot {
            provider_id: "synthetic".into(),
            model: "synthetic".into(),
            api: ApiProtocol::Plugin,
            credential_binding: "host-only".into(),
            adapter_version: gateway_plugin_contract::PROVIDER_PROTOCOL.into(),
            capabilities: CapabilityProfile {
                id: "synthetic".into(),
                version: "1".into(),
                protocol: ApiProtocol::Plugin,
                reasoning_contract: None,
                support,
            },
            context_window: Some(8192),
            max_output_tokens: Some(1024),
        };
        let plan = plan_translation(
            &request,
            &ContinuityBinding {
                route,
                scope: "test".into(),
            },
        )
        .unwrap();
        (request, plan)
    }
    fn response(tools: bool) -> Value {
        json!({"id":"response_synthetic","object":"response","created_at":1,"model":"synthetic","status":"completed","output":if tools {json!([{"id":"call_synthetic","type":"function_call","call_id":"call_one","name":"approved","arguments":"{}","status":"completed"}])} else {json!([{"id":"message_synthetic","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hello","annotations":[]}]}])},"usage":null})
    }
    fn completed(response: Value, tools: bool) -> Value {
        json!({"result":"completed","value":{"response":response,"outcome":if tools {"awaiting_tools"} else {"completed"},"usage":{"kind":"unobserved"},"state":{"kind":"none"}}})
    }
    fn scripted(
        mut peer: std::os::unix::net::UnixStream,
        replies: Vec<Value>,
    ) -> std::thread::JoinHandle<Vec<Value>> {
        std::thread::spawn(move || {
            let mut requests = vec![];
            for (sequence, value) in replies.into_iter().enumerate() {
                let mut n = [0; 4];
                peer.read_exact(&mut n).unwrap();
                let mut bytes = vec![0; u32::from_be_bytes(n) as usize];
                peer.read_exact(&mut bytes).unwrap();
                let request: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    request["protocol"],
                    gateway_plugin_contract::PROVIDER_PROTOCOL
                );
                requests.push(request);
                let reply=serde_json::to_vec(&json!({"protocol":gateway_plugin_contract::PROVIDER_PROTOCOL,"sequence":sequence+1,"value":value})).unwrap();
                peer.write_all(&(reply.len() as u32).to_be_bytes()).unwrap();
                peer.write_all(&reply).unwrap();
            }
            requests
        })
    }
    #[test]
    fn ready_is_exact_and_has_no_envelope_transport_controls() {
        let binding = binding();
        let ready = json!({"protocol":binding.protocol,"sequence":0,"value":{"result":"ready","provider_protocol":binding.provider_protocol,"capabilities":binding.capabilities}});
        assert!(super::super::process::validate_ready(&binding, ready.clone()).is_ok());
        for pointer in ["/value/provider_protocol", "/protocol"] {
            let mut value = ready.clone();
            *value.pointer_mut(pointer).unwrap() = json!("another/v1");
            assert!(super::super::process::validate_ready(&binding, value).is_err());
        }
        let mut value = ready;
        value["headers"] = json!({});
        assert!(super::super::process::validate_ready(&binding, value).is_err());
    }
    #[tokio::test]
    async fn json_unknown_vendor_shape_preserves_host_identity_and_rejects_wrong_output() {
        for wrong_model in [false, true] {
            let (request, plan) = plan(false, false);
            let binding = binding();
            let (mut prepared, peer) = PreparedProvider::test_prepared(&binding, &request, &plan);
            let mut response = response(false);
            if wrong_model {
                response["model"] = json!("other");
            }
            let thread = scripted(peer, vec![completed(response, false)]);
            let result = prepared
                .json(
                    br#"{"answer":"hello","headers":{"ignored":"data"}}"#,
                    "response_synthetic",
                )
                .await;
            if wrong_model {
                assert!(result.is_err());
            } else {
                let output = result.unwrap();
                assert_eq!(
                    output.observation.identity.package_sha256,
                    binding.package_sha256
                );
                assert_eq!(output.observation.usage.value("input_tokens"), None);
                assert_eq!(output.response["model"], "synthetic");
                assert!(output.terminal_events.is_empty());
            }
            assert_eq!(thread.join().unwrap()[0]["operation"]["operation"], "json");
        }
    }
    #[tokio::test]
    async fn stream_terminal_builder_forms_complete_text_and_tool_lifecycles() {
        for tools in [false, true] {
            let (request, plan) = plan(true, tools);
            let binding = binding();
            let (mut prepared, peer) = PreparedProvider::test_prepared(&binding, &request, &plan);
            let response = response(tools);
            let events = if tools {
                vec![]
            } else {
                vec![
                    json!({"type":"response.created","response":{"id":"response_synthetic","object":"response","created_at":1,"model":"synthetic","status":"in_progress","output":[]}}),
                    json!({"type":"response.output_item.added","output_index":0,"item":{"id":"message_synthetic","type":"message","role":"assistant","status":"in_progress","content":[]}}),
                    json!({"type":"response.content_part.added","output_index":0,"item_id":"message_synthetic","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
                    json!({"type":"response.output_text.delta","output_index":0,"item_id":"message_synthetic","content_index":0,"delta":"hello"}),
                ]
            };
            let thread = scripted(
                peer,
                vec![
                    json!({"result":"progress","events":[],"complete":false,"usage":{"kind":"unobserved"}}),
                    json!({"result":"progress","events":events,"complete":true,"usage":{"kind":"unobserved"}}),
                    completed(response.clone(), tools),
                ],
            );
            let mut stream = prepared.stream("response_synthetic".into()).await.unwrap();
            let progress = stream
                .event(crate::adapters::sse::SseEvent {
                    event: "end".into(),
                    data: "arbitrary vendor terminal".into(),
                })
                .await
                .unwrap();
            assert!(progress.semantic_complete);
            let output = stream.finish().await.unwrap();
            let verifier = crate::adapters::responses::output_verifier(&request, &plan).unwrap();
            let mut checked = verifier.stream(65536).unwrap();
            let mut all = progress.events;
            all.extend(output.terminal_events);
            for (sequence, mut event) in all.into_iter().enumerate() {
                event["sequence_number"] = json!(sequence);
                checked
                    .event(crate::adapters::sse::SseEvent {
                        event: event["type"].as_str().unwrap().into(),
                        data: event.to_string(),
                    })
                    .unwrap();
            }
            checked.finish().unwrap();
            thread.join().unwrap();
        }
    }
    #[tokio::test]
    async fn independent_source_runs_native_json_and_sse_without_gateway_imports() {
        use crate::provider_plugins::execution::{AuthorizedProviderHistory, ProviderLimits};
        use std::{
            fs,
            os::unix::fs::{MetadataExt, PermissionsExt},
            process::Command,
        };
        let python = std::env::var("MANAGEMENT_TEST_PYTHON").unwrap_or_else(|_| "python3".into());
        let resolved = Command::new(python)
            .args(["-I", "-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(resolved.status.success());
        let interpreter = String::from_utf8(resolved.stdout).unwrap();
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".local/provider-runtime-tests");
        fs::create_dir_all(&root).unwrap();
        let temp = tempfile::tempdir_in(root.canonicalize().unwrap()).unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let executable = temp.path().join("extension");
        let mut bytes = format!("#!{}\n", interpreter.trim()).into_bytes();
        bytes.extend(include_bytes!(
            "../../tools/plugin-conformance/examples/provider/provider.py"
        ));
        fs::write(&executable, &bytes).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let mut binding = binding();
        binding.executable = executable;
        binding.directory = temp.path().into();
        binding.owner = fs::metadata(temp.path()).unwrap().uid();
        binding.executable_sha256 = crate::continuation::hex(&crate::digest::sha256(&bytes));
        binding.provider_protocol = "synthetic-provider/v1".into();
        binding.capabilities.features = vec![
            "json".into(),
            "managed_continuation".into(),
            "streaming".into(),
        ];
        for streaming in [false, true] {
            let (request, plan) = plan(streaming, false);
            let mut prepared = PreparedProvider::prepare(
                &binding,
                &request,
                &plan,
                &AuthorizedProviderHistory::default(),
                None,
                ProviderLimits {
                    request_bytes: 65536,
                    output_bytes: 65536,
                },
            )
            .await
            .unwrap();
            let payload = prepared.take_payload();
            assert_eq!(payload["query"]["model"], "synthetic");
            assert!(payload.get("model").is_none());
            let native =
                json!({"answer":[{"text":"hello"}],"meter":{"input_tokens":2,"output_tokens":1}});
            let output = if streaming {
                let mut stream = prepared.stream("response_synthetic".into()).await.unwrap();
                let progress = stream
                    .event(crate::adapters::sse::SseEvent {
                        event: "piece".into(),
                        data: json!({"text":"hello"}).to_string(),
                    })
                    .await
                    .unwrap();
                assert!(!progress.semantic_complete);
                stream
                    .event(crate::adapters::sse::SseEvent {
                        event: "end".into(),
                        data: native.to_string(),
                    })
                    .await
                    .unwrap();
                stream.finish().await.unwrap()
            } else {
                prepared
                    .json(&serde_json::to_vec(&native).unwrap(), "response_synthetic")
                    .await
                    .unwrap()
            };
            assert_eq!(output.response["output"][0]["content"][0]["text"], "hello");
            assert_eq!(output.observation.usage.value("total_tokens"), Some(3));
        }
    }
    #[tokio::test]
    async fn undeclared_modes_fail_before_native_execution() {
        use crate::provider_plugins::execution::{AuthorizedProviderHistory, ProviderLimits};
        let (request, plan) = plan(true, false);
        let mut binding = binding();
        binding.capabilities.features = vec!["json".into()];
        for managed in [None, Some(false)] {
            let result = PreparedProvider::prepare(
                &binding,
                &request,
                &plan,
                &AuthorizedProviderHistory::default(),
                managed,
                ProviderLimits {
                    request_bytes: 65536,
                    output_bytes: 65536,
                },
            )
            .await;
            assert!(matches!(
                result,
                Err(crate::ir::IrError::UnsupportedFeature)
            ));
        }
    }
    #[tokio::test]
    async fn premature_terminal_and_unregistered_tools_never_escape() {
        let (request, plan) = plan(true, false);
        let (mut prepared, peer) = PreparedProvider::test_prepared(&binding(), &request, &plan);
        let thread = scripted(
            peer,
            vec![
                json!({"result":"progress","events":[],"complete":false,"usage":{"kind":"unobserved"}}),
                json!({"result":"progress","events":[{"type":"response.completed","response":response(false)}],"complete":true,"usage":{"kind":"unobserved"}}),
            ],
        );
        let mut stream = prepared.stream("response_synthetic".into()).await.unwrap();
        assert!(
            stream
                .event(crate::adapters::sse::SseEvent {
                    event: "end".into(),
                    data: "end".into()
                })
                .await
                .is_err()
        );
        thread.join().unwrap();
        drop(stream);
        let (request, plan) = self::plan(false, true);
        let (mut prepared, peer) = PreparedProvider::test_prepared(&binding(), &request, &plan);
        let mut forged = response(true);
        forged["output"][0]["name"] = json!("unregistered");
        let thread = scripted(peer, vec![completed(forged, true)]);
        assert!(
            prepared
                .json(br#"{"answer":"synthetic"}"#, "response_synthetic")
                .await
                .is_err()
        );
        thread.join().unwrap();
    }
    #[tokio::test]
    async fn restored_request_echoes_must_fit_output_limit() {
        let (request, plan) = plan(false, true);
        let mut raw = responses::encode(&request, None).unwrap();
        raw["tools"][0]["description"] = json!("x".repeat(4096));
        let request = responses::decode(raw, None).unwrap();
        let (mut prepared, peer) = PreparedProvider::test_prepared(&binding(), &request, &plan);
        prepared.test_output_limit(1024);
        let mut response = response(false);
        response["tools"] = Value::Null;
        assert!(serde_json::to_vec(&response).unwrap().len() < 1024);
        let thread = scripted(peer, vec![completed(response, false)]);
        assert!(matches!(
            prepared
                .json(br#"{"answer":"synthetic"}"#, "response_synthetic")
                .await,
            Err(crate::ir::IrError::SizeLimit)
        ));
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn extra_event_after_semantic_completion_poisoned_before_finish() {
        let (request, plan) = plan(true, false);
        let (mut prepared, peer) = PreparedProvider::test_prepared(&binding(), &request, &plan);
        let thread = scripted(
            peer,
            vec![
                json!({"result":"progress","events":[],"complete":false,"usage":{"kind":"unobserved"}}),
                json!({"result":"progress","events":[],"complete":true,"usage":{"kind":"unobserved"}}),
            ],
        );
        let mut stream = prepared.stream("response_synthetic".into()).await.unwrap();
        stream
            .event(crate::adapters::sse::SseEvent {
                event: "end".into(),
                data: "end".into(),
            })
            .await
            .unwrap();
        assert!(
            stream
                .event(crate::adapters::sse::SseEvent {
                    event: "extra".into(),
                    data: "extra".into()
                })
                .await
                .is_err()
        );
        assert!(stream.finish().await.is_err());
        thread.join().unwrap();
    }
    #[tokio::test]
    async fn invalid_usage_failure_retains_host_identity_without_inventing_zero() {
        use gateway_usage_contract::Source;
        for kind in ["typed_invalid", "negative", "arithmetic", "missing_shape"] {
            let (request, plan) = plan(false, false);
            let binding = binding();
            let (mut prepared, peer) = PreparedProvider::test_prepared(&binding, &request, &plan);
            let mut reply = completed(response(false), false);
            let mut usage = serde_json::to_value(snapshot(
                json!({"input_tokens":2,"output_tokens":1,"total_tokens":4}),
            ))
            .unwrap();
            match kind {
                "typed_invalid" => usage["counters"]["input_tokens"] = json!({"source":"invalid"}),
                "negative" => {
                    usage["counters"]["input_tokens"] = json!({"source":"reported","value":-1})
                }
                "missing_shape" => usage = json!({"kind":"observed"}),
                _ => {}
            }
            reply["value"]["usage"] = usage;
            let thread = scripted(peer, vec![reply]);
            assert!(
                prepared
                    .json(br#"{"answer":"synthetic"}"#, "response_synthetic")
                    .await
                    .is_err()
            );
            let attempted = prepared.observation();
            assert_eq!(attempted.identity.package_sha256, binding.package_sha256);
            let field = if kind == "arithmetic" {
                "total_tokens"
            } else {
                "input_tokens"
            };
            assert_eq!(attempted.usage.counters[field].source, Source::Invalid);
            assert_eq!(attempted.usage.value(field), None);
            assert!(!attempted.usage.violations.is_empty());
            assert!(attempted.usage.validate());
            thread.join().unwrap();
        }
    }
    #[tokio::test]
    async fn failed_finish_retains_decreased_counter_on_borrowed_stream() {
        use gateway_usage_contract::Source;
        let (request, plan) = plan(true, false);
        let (mut prepared, peer) = PreparedProvider::test_prepared(&binding(), &request, &plan);
        let mut final_reply = completed(response(false), false);
        final_reply["value"]["usage"] =
            serde_json::to_value(snapshot(json!({"input_tokens":2}))).unwrap();
        let thread = scripted(
            peer,
            vec![
                json!({"result":"progress","events":[],"complete":false,"usage":{"kind":"unobserved"}}),
                json!({"result":"progress","events":[],"complete":true,"usage":snapshot(json!({"input_tokens":3}))}),
                final_reply,
            ],
        );
        let mut stream = prepared.stream("response_synthetic".into()).await.unwrap();
        stream
            .event(crate::adapters::sse::SseEvent {
                event: "end".into(),
                data: "end".into(),
            })
            .await
            .unwrap();
        assert!(stream.finish().await.is_err());
        assert_eq!(
            stream.observation().usage.counters["input_tokens"].source,
            Source::Invalid
        );
        assert!(
            stream
                .observation()
                .usage
                .violations
                .iter()
                .any(|value| value == "counter_decreased")
        );
        assert!(stream.finish().await.is_err());
        assert_eq!(
            stream.observation().usage.counters["input_tokens"].source,
            Source::Invalid
        );
        thread.join().unwrap();
    }
    #[tokio::test]
    async fn wrong_sequence_or_protocol_cannot_contribute_usage_evidence() {
        for wrong_sequence in [false, true] {
            let (mut session, mut peer) = super::super::process::Session::test_pair();
            let thread = std::thread::spawn(move || {
                let mut n = [0; 4];
                peer.read_exact(&mut n).unwrap();
                let mut bytes = vec![0; u32::from_be_bytes(n) as usize];
                peer.read_exact(&mut bytes).unwrap();
                let reply = json!({"protocol":if wrong_sequence {gateway_plugin_contract::PROVIDER_PROTOCOL} else {"another/v1"},
                    "sequence":if wrong_sequence {2} else {1},"value":{"result":"progress","events":[],"complete":false,"usage":snapshot(json!({"input_tokens":10}))}});
                let reply = serde_json::to_vec(&reply).unwrap();
                peer.write_all(&(reply.len() as u32).to_be_bytes()).unwrap();
                peer.write_all(&reply).unwrap();
            });
            assert!(session.call(Operation::Finish).await.is_err());
            assert!(session.attempted_usage().is_none());
            assert!(session.call(Operation::Finish).await.is_err());
            thread.join().unwrap();
        }
    }
    #[tokio::test]
    async fn rejected_stream_initialization_keeps_invalid_usage_observation() {
        for malformed in [false, true] {
            let (request, plan) = plan(true, false);
            let (mut prepared, peer) = PreparedProvider::test_prepared(&binding(), &request, &plan);
            let mut usage = serde_json::to_value(snapshot(json!({}))).unwrap();
            usage["counters"]["input_tokens"] = if malformed {
                json!({"source":"reported","value":-1})
            } else {
                json!({"source":"invalid"})
            };
            let thread = scripted(
                peer,
                vec![json!({"result":"progress","events":[],"complete":false,"usage":usage})],
            );
            assert!(prepared.stream("response_synthetic".into()).await.is_err());
            assert_eq!(
                prepared.observation().usage.counters["input_tokens"].source,
                gateway_usage_contract::Source::Invalid
            );
            assert!(
                prepared
                    .observation()
                    .usage
                    .violations
                    .iter()
                    .any(|value| value == "invalid_counter")
            );
            thread.join().unwrap();
        }
    }
}
