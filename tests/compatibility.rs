use agent_response_gateway::{
    Config,
    adapters::{responses::PreparedResponses, sse::SseEvent},
    ir::{continuity::ContinuityBinding, responses},
};
use serde_json::{Value, json};

fn config(custom: &str, namespace: &str) -> Config {
    Config::parse(&format!(
        r#"
[providers.mock]
base_url="http://127.0.0.1:1/v1"
api_key_env="SYNTHETIC_KEY"
[models.writer]
provider="mock"
upstream_model="synthetic-model"
api="responses"
auth="bearer"
capability_profile="source"
compatibility_policy="checked"
[compatibility_policies.checked]
version=1
[compatibility_policies.checked.tools]
{custom}
{namespace}
[capability_profiles.source]
version="1"
provider="mock"
upstream_model="synthetic-model"
api="responses"
context_window=32768
max_output_tokens=1024
tested_codex_version="0.154.0"
[capability_profiles.source.support]
instructions="native"
instruction_hierarchy="native"
function_tools="native"
tool_choice="native"
parallel_tool_control="native"
max_output_tokens="native"
reasoning_items="native"
reasoning_summary="native"
"#
    ))
    .unwrap()
}

fn prepare(config: &Config, body: Value) -> PreparedResponses {
    let route = config.resolve_route("writer").unwrap();
    let agent_response_gateway::routing::AdmittedRequest::Translated { request, .. } =
        route.admit(body.as_object().unwrap().clone()).unwrap()
    else {
        panic!("checked")
    };
    PreparedResponses::encode(
        &request,
        &ContinuityBinding {
            route: route.snapshot,
            scope: "stateless-request".into(),
        },
    )
    .unwrap()
}

fn request() -> Value {
    json!({"model":"writer","input":"synthetic","stream":true,
        "tools":[{"type":"custom","name":"patch","format":{"type":"text"}},
                 {"type":"namespace","name":"group","description":"group description","tools":[
                    {"type":"function","name":"inspect","parameters":{"type":"object"}}]}]})
}
fn response(items: Vec<Value>) -> Value {
    json!({"id":"response_1","object":"response","model":"synthetic-model","created_at":1,
        "status":"completed","output":items,"usage":{"input_tokens":2,"output_tokens":3,"total_tokens":5}})
}
fn call(name: &str, args: &str) -> Value {
    json!({"id":"item_1","type":"function_call","name":name,"call_id":"call_1",
        "arguments":args,"status":"completed"})
}
fn frames(item: Value, final_response: Value) -> Vec<Value> {
    let mut start = final_response.clone();
    start["output"] = json!([]);
    start["status"] = json!("in_progress");
    start["usage"] = Value::Null;
    let mut head = item.clone();
    head["arguments"] = json!("");
    head["status"] = json!("in_progress");
    let mut frames = vec![
        json!({"type":"response.created","response":start}),
        json!({"type":"response.output_item.added","output_index":0,"item":head}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":item["id"],"delta":item["arguments"]}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":item["id"],"arguments":item["arguments"]}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":final_response}),
    ];
    for (n, event) in frames.iter_mut().enumerate() {
        event["sequence_number"] = json!(n);
    }
    frames
}
fn event(value: &Value) -> SseEvent {
    SseEvent {
        event: value["type"].as_str().unwrap().into(),
        data: value.to_string(),
    }
}

#[test]
fn policy_rejects_missing_prerequisites_conflicts_and_unknown_versions() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let route = c.resolve_route("writer").unwrap();
    assert!(
        route
            .snapshot
            .adapter_version
            .starts_with("compatibility/1/")
    );
    let mut missing = c.clone();
    missing
        .capability_profiles
        .get_mut("source")
        .unwrap()
        .support
        .remove(&agent_response_gateway::ir::capability::Feature::FunctionTools);
    assert!(missing.validate().is_err());
    let mut unknown = c.clone();
    unknown
        .compatibility_policies
        .get_mut("checked")
        .unwrap()
        .version = 2;
    assert!(unknown.validate().is_err());
    let mut conflict = c.clone();
    conflict
        .capability_profiles
        .get_mut("source")
        .unwrap()
        .support
        .insert(
            agent_response_gateway::ir::capability::Feature::CustomTools,
            agent_response_gateway::config::DeclaredSupport::Native,
        );
    assert!(conflict.validate().is_err());
    let mut missing = c.clone();
    missing
        .models
        .get_mut("writer")
        .unwrap()
        .compatibility_policy = Some("absent".into());
    assert!(missing.validate().is_err());
}

#[test]
fn checked_mapping_restores_definitions_choice_and_next_turn_history() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let mut body = request();
    body["tool_choice"] = json!({"type":"custom","name":"patch"});
    let p = prepare(&c, body.clone());
    let name = p.payload["tools"][0]["name"].as_str().unwrap();
    assert_eq!(p.payload["tools"][0]["type"], "function");
    assert_eq!(
        p.payload["tool_choice"],
        json!({"type":"function","name":name})
    );
    let text = "freeform 한글\nwith quotes \" and slash \\";
    let wire = call(name, &json!({"input":text}).to_string());
    let mut raw = response(vec![wire]);
    raw["tools"] = p.payload["tools"].clone();
    raw["tool_choice"] = p.payload["tool_choice"].clone();
    let restored = p.decode_bytes(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert_eq!(restored["tools"], body["tools"]);
    assert_eq!(restored["tool_choice"], body["tool_choice"]);
    let original = &restored["output"][0];
    assert_eq!(original["input"], text);
    assert_eq!(original["type"], "custom_tool_call");
    assert_eq!(original["name"], "patch");
    body["input"] = json!([original,{"type":"custom_tool_call_output","call_id":"call_1","output":"synthetic result"}]);
    let next = prepare(&c, body);
    assert_eq!(next.payload["input"][0]["type"], "function_call");
    assert_eq!(next.payload["input"][1]["type"], "function_call_output");
    assert_eq!(
        next.payload["input"][0]["call_id"],
        next.payload["input"][1]["call_id"]
    );
    assert_eq!(
        serde_json::from_str::<Value>(next.payload["input"][0]["arguments"].as_str().unwrap())
            .unwrap()["input"],
        text
    );
}

#[test]
fn independent_namespace_policy_retains_native_custom_wire_kind() {
    let mut c = config("custom_input=\"preserve\"", "namespaces=\"flatten\"");
    c.capability_profiles
        .get_mut("source")
        .unwrap()
        .support
        .insert(
            agent_response_gateway::ir::capability::Feature::CustomTools,
            agent_response_gateway::config::DeclaredSupport::Native,
        );
    let mut body = request();
    body["tools"] = json!([{"type":"namespace","name":"group","tools":[{"type":"custom","name":"patch","format":{"type":"text"}}]}]);
    let p = prepare(&c, body);
    assert_eq!(p.payload["tools"][0]["type"], "custom");
    let name = p.payload["tools"][0]["name"].clone();
    let raw = response(vec![
        json!({"type":"custom_tool_call","id":"item","call_id":"call","name":name,"input":"synthetic","status":"completed"}),
    ]);
    let actual = p.decode_bytes(&raw.to_string().into_bytes()).unwrap();
    assert_eq!(actual["output"][0]["namespace"], "group");
    assert_eq!(actual["output"][0]["name"], "patch");
    assert_eq!(actual["output"][0]["input"], "synthetic");
}

#[test]
fn independent_custom_policy_retains_native_namespaces() {
    let mut c = config("custom_input=\"function_json\"", "namespaces=\"preserve\"");
    c.capability_profiles
        .get_mut("source")
        .unwrap()
        .support
        .insert(
            agent_response_gateway::ir::capability::Feature::NamespacedTools,
            agent_response_gateway::config::DeclaredSupport::Native,
        );
    let p = prepare(&c, request());
    assert_eq!(p.payload["tools"][1]["type"], "namespace");
    assert_eq!(p.payload["tools"][1]["tools"][0]["name"], "inspect");
    let mut raw = call("inspect", "{}");
    raw["namespace"] = json!("group");
    let out = p
        .decode_bytes(response(vec![raw]).to_string().as_bytes())
        .unwrap();
    assert_eq!(out["output"][0]["namespace"], "group");
}

#[test]
fn executable_stream_events_are_held_until_validated_terminal() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let p = prepare(&c, request());
    let name = p.payload["tools"][0]["name"].as_str().unwrap();
    let wire = call(name, &json!({"input":"synthetic"}).to_string());
    let raw = response(vec![wire.clone()]);
    let expected = p.decode_bytes(raw.to_string().as_bytes()).unwrap();
    let frames = frames(wire, raw);
    let mut s = p.stream(65536).unwrap();
    let mut output = Vec::new();
    for frame in &frames[..frames.len() - 1] {
        let values = s.event(event(frame)).unwrap();
        assert!(values.iter().all(|v| !is_executable(v)));
        output.extend(values);
    }
    assert!(s.finish().is_err());
    output.extend(s.event(event(frames.last().unwrap())).unwrap());
    assert_eq!(output.last().unwrap()["response"], expected);
    assert!(output.iter().any(is_executable));
    assert!(s.finish().is_ok());
    assert!(s.event(event(frames.last().unwrap())).is_err());
    for (n, v) in output.iter().enumerate() {
        assert_eq!(v["sequence_number"], n);
    }
}
fn is_executable(value: &Value) -> bool {
    value["type"] == "response.output_item.done"
        && matches!(
            value["item"]["type"].as_str(),
            Some("function_call" | "custom_tool_call")
        )
}

#[test]
fn malformed_or_inconsistent_tool_stream_cannot_expose_completion() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let p = prepare(&c, request());
    let name = p.payload["tools"][0]["name"].as_str().unwrap();
    let wire = call(name, &json!({"input":"synthetic"}).to_string());
    let original = frames(wire.clone(), response(vec![wire]));
    for mode in 0..7 {
        let mut fs = original.clone();
        match mode {
            0 => fs[2]["item_id"] = json!("other"),
            1 => fs[3]["arguments"] = json!("{\"input\":\"changed\"}"),
            2 => fs[4]["item"]["call_id"] = json!("other"),
            3 => fs[5]["response"]["output"][0]["arguments"] = json!("{}"),
            4 => fs[5]["response"]["model"] = json!("other"),
            5 => fs[3]["sequence_number"] = json!(0),
            _ => fs[5]["response"]["usage"]["output_tokens"] = json!(-1),
        }
        let mut s = p.stream(65536).unwrap();
        let mut rejected = false;
        for frame in fs {
            match s.event(event(&frame)) {
                Ok(values) => assert!(!values.iter().any(is_executable)),
                Err(_) => {
                    rejected = true;
                    break;
                }
            }
        }
        assert!(rejected);
        assert!(s.finish().is_err());
    }
}

#[test]
fn native_passthrough_and_checked_unknowns_have_separate_contracts() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let mut body = request();
    body["future_extension"] = json!({"value":1});
    let route = c.resolve_route("writer").unwrap();
    let agent_response_gateway::routing::AdmittedRequest::Translated { request, .. } =
        route.admit(body.as_object().unwrap().clone()).unwrap()
    else {
        panic!("checked")
    };
    assert!(
        PreparedResponses::encode(
            &request,
            &ContinuityBinding {
                route: route.snapshot,
                scope: "stateless-request".into()
            }
        )
        .is_err()
    );
    let mut native = c;
    native
        .models
        .get_mut("writer")
        .unwrap()
        .compatibility_policy = None;
    assert!(matches!(
        native
            .resolve_route("writer")
            .unwrap()
            .admit(body.as_object().unwrap().clone())
            .unwrap(),
        agent_response_gateway::routing::AdmittedRequest::Native(_)
    ));
}

#[test]
fn policy_content_and_selection_bind_manifest_without_changing_old_mode() {
    let mut c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let checked = serde_json::to_value(c.manifest().unwrap()).unwrap();
    assert_eq!(checked["schema"], "gateway-embedded-manifest/v4");
    assert_eq!(
        checked["configuration"]["routes"][0]["compatibility"]["policy"]["version"],
        1
    );
    c.models.get_mut("writer").unwrap().compatibility_policy = None;
    let old = serde_json::to_value(c.manifest().unwrap()).unwrap();
    assert_eq!(old["schema"], "gateway-embedded-manifest/v1");
    c.compatibility_policies.clear();
    assert_eq!(serde_json::to_value(c.manifest().unwrap()).unwrap(), old);
}

#[test]
fn custom_envelope_errors_and_tool_contract_violations_fail() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let mut body = request();
    body["parallel_tool_calls"] = json!(false);
    let p = prepare(&c, body);
    let name = p.payload["tools"][0]["name"].as_str().unwrap();
    for args in [
        "{}",
        "{\"input\":1}",
        "{\"input\":\"a\",\"input\":\"b\"}",
        "{\"input\":\"a\",\"extra\":1}",
    ] {
        assert!(
            p.decode_bytes(response(vec![call(name, args)]).to_string().as_bytes())
                .is_err()
        );
    }
    let first = call(name, "{\"input\":\"a\"}");
    let mut second = first.clone();
    second["id"] = json!("item_2");
    second["call_id"] = json!("call_2");
    assert!(
        p.decode_bytes(response(vec![first, second]).to_string().as_bytes())
            .is_err()
    );
    let mut required = request();
    required["tool_choice"] = json!("required");
    let p = prepare(&c, required);
    assert!(
        p.decode_bytes(response(vec![]).to_string().as_bytes())
            .is_err()
    );
}

#[test]
fn registered_grammar_rejects_unknown_definitions_before_dispatch() {
    let c = config(
        "custom_input=\"function_json\"\ngrammar=\"registered_output_validation\"",
        "namespaces=\"flatten\"",
    );
    let mut body = request();
    body["tools"][0]["format"] =
        json!({"type":"grammar","syntax":"lark","definition":"start: /.+/"});
    let route = c.resolve_route("writer").unwrap();
    assert!(route.admit(body.as_object().unwrap().clone()).is_err());
    let decoded = responses::decode(json!({"model":"writer","input":"text"}), None).unwrap();
    assert!(
        PreparedResponses::encode(
            &decoded,
            &ContinuityBinding {
                route: route.snapshot,
                scope: "stateless-request".into()
            }
        )
        .is_ok()
    );
}

#[test]
fn public_text_and_reasoning_stream_incrementally_with_utf8_and_terminal_consistency() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let p = prepare(&c, request());
    for reasoning in [false, true] {
        let id = "public_item";
        let part =
            json!({"type":if reasoning {"summary_text"} else {"output_text"},"text":"합성 🧪"});
        let mut item = if reasoning {
            json!({"id":id,"type":"reasoning","status":"completed","summary":[part]})
        } else {
            json!({"id":id,"type":"message","role":"assistant","phase":"commentary","status":"completed","content":[part]})
        };
        let done = item.clone();
        item["status"] = json!("in_progress");
        item[if reasoning { "summary" } else { "content" }] = json!([]);
        let prefix = if reasoning {
            "response.reasoning_summary"
        } else {
            "response.content"
        };
        let text_prefix = if reasoning {
            "response.reasoning_summary_text"
        } else {
            "response.output_text"
        };
        let index_key = if reasoning {
            "summary_index"
        } else {
            "content_index"
        };
        let mut head = response(vec![]);
        head["status"] = json!("in_progress");
        head["usage"] = Value::Null;
        let mut fs = vec![
            json!({"type":"response.created","response":head}),
            json!({"type":"response.output_item.added","output_index":0,"item":item}),
            json!({"type":format!("{prefix}_part.added"),"item_id":id,"output_index":0,index_key:0,"part":{"type":part["type"],"text":""}}),
            json!({"type":format!("{text_prefix}.delta"),"item_id":id,"output_index":0,index_key:0,"delta":part["text"]}),
            json!({"type":format!("{text_prefix}.done"),"item_id":id,"output_index":0,index_key:0,"text":part["text"]}),
            json!({"type":format!("{prefix}_part.done"),"item_id":id,"output_index":0,index_key:0,"part":part}),
            json!({"type":"response.output_item.done","output_index":0,"item":done}),
            json!({"type":"response.completed","response":response(vec![done])}),
        ];
        for (n, f) in fs.iter_mut().enumerate() {
            f["sequence_number"] = json!(n);
        }
        let wire = fs
            .iter()
            .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
            .collect::<String>();
        let mut decoder = agent_response_gateway::adapters::sse::SseDecoder::new(65536).unwrap();
        let mut stream = p.stream(65536).unwrap();
        let mut text_seen = false;
        let mut terminal_seen = false;
        for byte in wire.as_bytes() {
            let bytes = [*byte];
            let mut remaining = bytes.as_slice();
            while let Some(event) = decoder.next_event(&mut remaining).unwrap() {
                for output in stream.event(event).unwrap() {
                    if output["type"] == format!("{text_prefix}.delta") {
                        assert!(!terminal_seen);
                        assert_eq!(output["delta"], "합성 🧪");
                        text_seen = true;
                    }
                    if output["type"] == "response.completed" {
                        assert!(text_seen);
                        terminal_seen = true;
                    }
                }
            }
        }
        decoder.finish().unwrap();
        stream.finish().unwrap();
        assert!(terminal_seen);
        for bad in [2, 3, 4, 5, 6, 7] {
            let mut changed = fs.clone();
            match bad {
                2 => changed[2]["part"]["text"] = json!("nonempty"),
                3 => changed[3]["item_id"] = json!("wrong"),
                4 => changed[4]["text"] = json!("different"),
                5 => changed[5]["part"]["text"] = json!("different"),
                6 => {
                    changed[6]["item"][if reasoning { "summary" } else { "content" }][0]["text"] =
                        json!("different")
                }
                _ => changed[7]["response"]["created_at"] = json!(5),
            }
            let mut s = p.stream(65536).unwrap();
            assert!(changed.iter().any(|v| s.event(event(v)).is_err()));
            assert!(s.finish().is_err());
        }
    }
}

#[test]
fn checked_semantic_extensions_and_inconsistent_envelopes_reject() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let route = c.resolve_route("writer").unwrap();
    for field in [
        json!({"text":{"format":{"type":"text","future":true}}}),
        json!({"tool_choice":{"type":"custom","name":"patch","future":true}}),
        json!({"reasoning":{"effort":"high","future":true}}),
    ] {
        let mut body = request();
        body.as_object_mut()
            .unwrap()
            .extend(field.as_object().unwrap().clone());
        let admitted = route.admit(body.as_object().unwrap().clone());
        if let Ok(agent_response_gateway::routing::AdmittedRequest::Translated {
            request,
            plan: _,
        }) = admitted
        {
            assert!(
                PreparedResponses::encode(
                    &request,
                    &ContinuityBinding {
                        route: route.snapshot.clone(),
                        scope: "stateless-request".into()
                    }
                )
                .is_err()
            );
        } else {
            assert!(admitted.is_err());
        }
    }
    let p = prepare(&c, request());
    for extra in [
        json!({"tools":[{"type":"function","name":"unexpected"}]}),
        json!({"error":{"message":"synthetic private error"}}),
        json!({"output_text":"unbacked text"}),
        json!({"incomplete_details":{"reason":"max_output_tokens"}}),
    ] {
        let mut body = response(vec![]);
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(p.decode_bytes(body.to_string().as_bytes()).is_err());
    }
}

#[test]
fn checked_message_phase_survives_history_without_admitting_unknown_metadata() {
    let c = config("custom_input=\"function_json\"", "namespaces=\"flatten\"");
    let mut body = request();
    body["input"] = json!([{"type":"message","role":"assistant","phase":"commentary","status":"completed","content":[{"type":"output_text","text":"synthetic","annotations":[]}]}]);
    let p = prepare(&c, body);
    assert_eq!(p.payload["input"][0]["phase"], "commentary");
}
