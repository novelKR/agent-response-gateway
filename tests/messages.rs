use agent_response_gateway::{
    adapters::messages::encode,
    ir::{
        ApiProtocol, IrError,
        capability::{CapabilityProfile, Feature, Support},
        continuity::{ContinuityBinding, RouteSnapshot},
        responses,
    },
};
use serde_json::{Value, json};

fn target() -> ContinuityBinding {
    use Feature::*;
    ContinuityBinding {
        scope: "synthetic-scope".into(),
        route: RouteSnapshot {
            provider_id: "mock".into(),
            model: "synthetic-model".into(),
            api: ApiProtocol::Messages,
            credential_binding: "synthetic-generation".into(),
            adapter_version: "1".into(),
            capabilities: CapabilityProfile {
                id: "synthetic".into(),
                version: "1".into(),
                protocol: ApiProtocol::Messages,
                support: [
                    Instructions,
                    InstructionHierarchy,
                    Images,
                    FunctionTools,
                    StrictToolArguments,
                    CustomTools,
                    NamespacedTools,
                    ToolChoice,
                    ParallelToolControl,
                    MaxOutputTokens,
                    Temperature,
                    TopP,
                    StructuredOutput,
                    StrictStructuredOutput,
                    ReasoningEffort,
                ]
                .into_iter()
                .map(|f| (f, Support::Native))
                .collect(),
            },
            context_window: Some(32000),
            max_output_tokens: Some(1024),
        },
    }
}
fn request() -> Value {
    json!({"model":"writer","input":"synthetic","tools":[
        {"type":"function","name":"echo","description":"Synthetic echo","parameters":{"type":"object","properties":{"text":{"type":"string"}}}}
    ]})
}
fn response() -> Value {
    json!({"id":"msg_synthetic","type":"message","role":"assistant","model":"synthetic-model",
    "content":[{"type":"text","text":"합성 응답"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":3}})
}

#[test]
fn messages_preserve_text_order_parameters_and_supported_tool_selection() {
    let mut body = request();
    body["instructions"] = json!("Synthetic instruction");
    body["input"] = json!([{"role":"user","content":"first"},{"role":"user","content":[{"type":"input_text","text":"second"}]},
        {"role":"assistant","content":"third"},{"role":"user","content":"fourth"}]);
    body["parallel_tool_calls"] = json!(false);
    body["tool_choice"] = json!({"type":"function","name":"echo"});
    body["max_output_tokens"] = json!(900);
    body["temperature"] = json!(0.2);
    body["top_p"] = json!(0.9);
    let ir = responses::decode(body, None).unwrap();
    let prepared = encode(&ir, &target()).unwrap();
    assert_eq!(prepared.payload["model"], "synthetic-model");
    assert_eq!(prepared.payload["max_tokens"], 900);
    assert_eq!(prepared.payload["temperature"], json!(0.2));
    assert_eq!(prepared.payload["top_p"], json!(0.9));
    assert_eq!(
        prepared.payload["system"],
        json!([{"type":"text","text":"Synthetic instruction"}])
    );
    assert_eq!(
        prepared.payload["messages"][0]["content"],
        json!([{"type":"text","text":"first"},{"type":"text","text":"second"}])
    );
    assert_eq!(prepared.payload["messages"][1]["role"], "assistant");
    assert_eq!(
        prepared.payload["messages"][2]["content"][0]["text"],
        "fourth"
    );
    assert_eq!(
        prepared.payload["tool_choice"],
        json!({"type":"tool","name":"echo","disable_parallel_tool_use":true})
    );
    assert_eq!(
        prepared.payload["tools"][0]["input_schema"]["properties"]["text"]["type"],
        "string"
    );
    assert!(prepared.payload.get("store").is_none());
}

#[test]
fn parallel_tool_history_keeps_call_and_result_identity() {
    let mut body = request();
    body["input"] = json!([{"role":"user","content":"begin"},
        {"type":"function_call","call_id":"call_a","name":"echo","arguments":"{\"text\":\"a\"}"},
        {"type":"function_call","call_id":"call_b","name":"echo","arguments":"{\"text\":\"b\"}"},
        {"type":"function_call_output","call_id":"call_b","output":"result b"},
        {"type":"function_call_output","call_id":"call_a","output":"result a"}]);
    let prepared = encode(&responses::decode(body.clone(), None).unwrap(), &target()).unwrap();
    assert_eq!(
        prepared.payload["messages"][1]["content"][0]["id"],
        "call_a"
    );
    assert_eq!(
        prepared.payload["messages"][1]["content"][1]["id"],
        "call_b"
    );
    assert_eq!(
        prepared.payload["messages"][2]["content"][0]["tool_use_id"],
        "call_b"
    );
    assert_eq!(
        prepared.payload["messages"][2]["content"][1]["tool_use_id"],
        "call_a"
    );
    let items = body["input"].as_array_mut().unwrap();
    items.insert(3, json!({"role":"user","content":"interrupted results"}));
    assert!(encode(&responses::decode(body, None).unwrap(), &target()).is_err());
}

#[test]
fn declarations_cannot_enable_unimplemented_semantics_or_hidden_extensions() {
    for extra in [
        json!({"input":[{"role":"developer","content":"required role"},{"role":"user","content":"input"}]}),
        json!({"tools":[{"type":"custom","name":"custom"}]}),
        json!({"text":{"format":{"type":"json_schema","name":"out","schema":{"type":"object"},"strict":false}}}),
        json!({"reasoning":{"effort":"minimal"}}),
        json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.test/image.png","detail":"high"}]}]}),
        json!({"unmapped":"required"}),
    ] {
        let mut body = request();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let ir = responses::decode(body, None).unwrap();
        assert!(encode(&ir, &target()).is_err());
    }
}

#[test]
fn response_usage_text_and_function_calls_are_validated_and_restored() {
    let prepared = encode(&responses::decode(request(), None).unwrap(), &target()).unwrap();
    let mut upstream = response();
    upstream["usage"]["cache_read_input_tokens"] = json!(5);
    upstream["usage"]["cache_creation_input_tokens"] = json!(2);
    let decoded = prepared.decode(upstream).unwrap();
    assert_eq!(decoded["status"], "completed");
    assert_eq!(decoded["output"][0]["content"][0]["text"], "합성 응답");
    assert_eq!(
        decoded["usage"],
        json!({"input_tokens":17,"output_tokens":3,"total_tokens":20,"input_tokens_details":{"cached_tokens":5,"cache_write_tokens":2}})
    );
    assert!(decoded["created_at"].as_u64().unwrap() > 0);
    let mut upstream = response();
    upstream["stop_reason"] = json!("tool_use");
    upstream["content"] =
        json!([{"type":"tool_use","id":"call_original","name":"echo","input":{"text":"x\\n한글"}}]);
    let decoded = prepared.decode(upstream.clone()).unwrap();
    assert_eq!(decoded["output"][0]["call_id"], "call_original");
    assert_eq!(decoded["output"][0]["name"], "echo");
    assert_eq!(
        serde_json::from_str::<Value>(decoded["output"][0]["arguments"].as_str().unwrap()).unwrap(),
        json!({"text":"x\\n한글"})
    );
    upstream["stop_reason"] = json!("max_tokens");
    let decoded = prepared.decode(upstream).unwrap();
    assert_eq!(decoded["status"], "incomplete");
    assert_eq!(decoded["output"][0]["status"], "incomplete");
}

#[test]
fn malformed_unknown_and_inconsistent_provider_output_never_becomes_success() {
    let prepared = encode(&responses::decode(request(), None).unwrap(), &target()).unwrap();
    for extra in [
        json!({"model":"different-model"}),
        json!({"role":"user"}),
        json!({"stop_reason":"pause_turn"}),
        json!({"stop_reason":"refusal"}),
        json!({"stop_reason":"tool_use"}),
        json!({"stop_reason":"model_context_window_exceeded"}),
        json!({"content":[{"type":"thinking","thinking":"opaque","signature":"opaque"}]}),
        json!({"content":[{"type":"text","text":"x","citations":[{"unmapped":"citation"}]}]}),
        json!({"content":[{"type":"tool_use","id":"call_bad","name":"missing","input":{}}],"stop_reason":"tool_use"}),
        json!({"content":[{"type":"tool_use","id":"call_a","name":"echo","input":{}}]}),
        json!({"content":[{"type":"tool_use","id":"call_a","name":"echo","input":null}],"stop_reason":"tool_use"}),
        json!({"usage":{"input_tokens":-1,"output_tokens":3}}),
        json!({"usage":{"input_tokens":18446744073709551615_u64,"output_tokens":3}}),
    ] {
        let mut body = response();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(prepared.decode(body).is_err());
    }
    let mut body = request();
    body["parallel_tool_calls"] = json!(false);
    let prepared = encode(&responses::decode(body, None).unwrap(), &target()).unwrap();
    let mut upstream = response();
    upstream["stop_reason"] = json!("tool_use");
    upstream["content"] = json!([{"type":"tool_use","id":"call_a","name":"echo","input":{}},{"type":"tool_use","id":"call_b","name":"echo","input":{}}]);
    assert!(matches!(
        prepared.decode(upstream),
        Err(IrError::InvalidToolMapping)
    ));
}

#[test]
fn approved_instruction_bridge_preserves_prefix_provenance_and_excludes_user_text() {
    use agent_response_gateway::ir::capability::BridgeRule;
    let mut target = target();
    target.route.capabilities.support.insert(
        Feature::InstructionHierarchy,
        Support::Bridged(BridgeRule::MessagesInstructionEnvelope),
    );
    let mut body = request();
    body["instructions"] = json!("base \"quoted\"\nline");
    body["input"] = json!([
        {"role":"system","content":"system fixture"},
        {"role":"developer","content":[{"type":"input_text","text":"developer first"},{"type":"input_text","text":"developer second"}]},
        {"role":"user","content":"USER_MUST_STAY_OUTSIDE_ENVELOPE"}]);
    let ir = responses::decode(body.clone(), None).unwrap();
    let prepared = encode(&ir, &target).unwrap();
    let records: Value =
        serde_json::from_str(prepared.payload["system"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        records,
        json!([
            {"role":"protocol_default","position":"request","text":"base \"quoted\"\nline"},
            {"role":"system","position":0,"content":[{"type":"text","text":"system fixture"}]},
            {"role":"developer","position":1,"content":[{"type":"text","text":"developer first"},{"type":"text","text":"developer second"}]}
        ])
    );
    assert!(
        !prepared.payload["system"]
            .to_string()
            .contains("USER_MUST_STAY_OUTSIDE_ENVELOPE")
    );
    assert_eq!(
        prepared.payload["messages"][0]["content"][0]["text"],
        "USER_MUST_STAY_OUTSIDE_ENVELOPE"
    );
    body["input"]
        .as_array_mut()
        .unwrap()
        .push(json!({"role":"developer","content":"late instruction"}));
    assert!(encode(&responses::decode(body, None).unwrap(), &target).is_err());
    target.route.api = ApiProtocol::ChatCompletions;
    target.route.capabilities.protocol = ApiProtocol::ChatCompletions;
    assert!(target.validate().is_err());
}

#[test]
fn tool_choice_and_nonportable_provider_state_remain_explicit_errors() {
    for choice in [json!("none"), json!("required")] {
        let mut body = request();
        body["tool_choice"] = choice.clone();
        let prepared = encode(&responses::decode(body, None).unwrap(), &target()).unwrap();
        let mut upstream = response();
        if choice == "none" {
            upstream["stop_reason"] = json!("tool_use");
            upstream["content"] =
                json!([{"type":"tool_use","id":"call_a","name":"echo","input":{}}]);
        }
        assert!(prepared.decode(upstream).is_err());
    }
    let prepared = encode(&responses::decode(request(), None).unwrap(), &target()).unwrap();
    for field in ["container", "context_management", "stop_details"] {
        let mut upstream = response();
        upstream[field] = json!({"unmapped":"state"});
        assert!(prepared.decode(upstream).is_err());
    }
    let mut wrong = target();
    wrong
        .route
        .capabilities
        .support
        .remove(&Feature::FunctionTools);
    assert!(encode(&responses::decode(request(), None).unwrap(), &wrong).is_err());
}

fn bridged_target() -> ContinuityBinding {
    use agent_response_gateway::ir::capability::BridgeRule;
    let mut target = target();
    for (feature, rule) in [
        (Feature::CustomTools, BridgeRule::CustomToolJson),
        (Feature::NamespacedTools, BridgeRule::ToolNamespace),
        (Feature::CustomGrammar, BridgeRule::CodexPatchGrammar),
    ] {
        target
            .route
            .capabilities
            .support
            .insert(feature, Support::Bridged(rule));
    }
    target
}
fn namespaced_request() -> Value {
    json!({"model":"writer", "input":"synthetic", "tools":[
        {"type":"function","name":"echo","parameters":{"type":"object"}},
        {"type":"namespace","name":"group","description":"Namespace fixture", "tools":[
            {"type":"function","name":"echo","description":"Member fixture","parameters":{"type":"object"}},
            {"type":"custom","name":"echo","format":{"type":"text"}}
        ]}
    ]})
}

#[test]
fn namespace_registry_preserves_definitions_choices_history_and_output() {
    let mut body = namespaced_request();
    // Distinct custom name within one namespace, same leaf name across flat/group definitions.
    body["tools"][1]["tools"][1]["name"] = json!("patch");
    body["tool_choice"] = json!({"type":"function","namespace":"group","name":"echo"});
    body["input"] = json!([
        {"role":"user","content":"begin"},
        {"type":"function_call","name":"echo","call_id":"flat","arguments":"{}"},
        {"type":"function_call","namespace":"group","name":"echo","call_id":"member","arguments":"{}"},
        {"type":"custom_tool_call","namespace":"group","name":"patch","call_id":"custom","input":"quoted \"한글\"\n\\"},
        {"type":"custom_tool_call_output","call_id":"custom","output":"custom result"},
        {"type":"function_call_output","call_id":"member","output":"member result"},
        {"type":"function_call_output","call_id":"flat","output":"flat result"}
    ]);
    let prepared = encode(&responses::decode(body, None).unwrap(), &bridged_target()).unwrap();
    let tools = prepared.payload["tools"].as_array().unwrap();
    assert_eq!(tools[0]["name"], "echo");
    assert_ne!(tools[1]["name"], tools[0]["name"]);
    assert_ne!(tools[2]["name"], tools[1]["name"]);
    let description: Value =
        serde_json::from_str(tools[1]["description"].as_str().unwrap()).unwrap();
    assert_eq!(description["namespace_description"], "Namespace fixture");
    assert_eq!(description["tool_description"], "Member fixture");
    assert_eq!(prepared.payload["tool_choice"]["name"], tools[1]["name"]);
    for (i, tool) in tools.iter().enumerate() {
        assert_eq!(
            prepared.payload["messages"][1]["content"][i]["name"],
            tool["name"]
        );
    }
    assert_eq!(
        prepared.payload["messages"][1]["content"][2]["input"]["input"],
        "quoted \"한글\"\n\\"
    );
    let mut upstream = response();
    upstream["stop_reason"] = json!("tool_use");
    upstream["content"] = json!([{"type":"tool_use","id":"original_call","name":tools[1]["name"],"input":{"n":9007199254740993123_u64}}]);
    let decoded = prepared.decode(upstream).unwrap();
    assert_eq!(decoded["output"][0]["namespace"], "group");
    assert_eq!(decoded["output"][0]["name"], "echo");
    assert_eq!(decoded["output"][0]["call_id"], "original_call");
    assert!(
        decoded["output"][0]["arguments"]
            .as_str()
            .unwrap()
            .contains("9007199254740993123")
    );
    assert!(responses::decode(namespaced_request(), None).is_err());
}

fn stream_events(function: &str, custom: &str) -> Vec<Value> {
    let mut result = vec![
        json!({"type":"message_start","message":{"id":"msg_synthetic","type":"message","role":"assistant","model":"synthetic-model","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":1}}}),
        json!({"type":"ping"}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"합성 🧪"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"function_call","name":function,"input":{}}}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"custom_call","name":custom,"input":{}}}),
    ];
    for (index, text) in [
        (1, "{\"x\":"),
        (2, "{\"input\":\"한글"),
        (1, "9007199254740993123}"),
        (2, "\\n\\\"quoted\\\"\"}"),
    ] {
        result.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":text}}));
    }
    result.extend([
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":12}}),
        json!({"type":"message_stop"}),
    ]);
    result
}
fn stream_prepared() -> agent_response_gateway::adapters::messages::PreparedMessages {
    let mut body = namespaced_request();
    body["tools"][1]["tools"][1]["name"] = json!("patch");
    encode(&responses::decode(body, None).unwrap(), &bridged_target()).unwrap()
}
fn event(value: &Value) -> agent_response_gateway::adapters::sse::SseEvent {
    agent_response_gateway::adapters::sse::SseEvent {
        event: value["type"].as_str().unwrap().into(),
        data: value.to_string(),
    }
}

#[test]
fn incremental_stream_preserves_parallel_tools_utf8_and_custom_input_at_every_split() {
    use agent_response_gateway::adapters::sse::SseDecoder;
    let prepared = stream_prepared();
    let events = stream_events(
        prepared.payload["tools"][1]["name"].as_str().unwrap(),
        prepared.payload["tools"][2]["name"].as_str().unwrap(),
    );
    let wire: Vec<u8> = events
        .iter()
        .map(|v| {
            format!(
                "event: {}\r\ndata: {}\r\n\r\n",
                v["type"].as_str().unwrap(),
                v
            )
        })
        .collect::<String>()
        .into_bytes();
    // A full pass for every byte split includes cuts inside UTF-8, JSON escapes and SSE separators.
    for split in 0..=wire.len() {
        let mut framing = SseDecoder::new(1024 * 1024).unwrap();
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        let mut output = Vec::new();
        for chunk in [&wire[..split], &wire[split..]] {
            let mut rest = chunk;
            while let Some(event) = framing.next_event(&mut rest).unwrap() {
                output.extend(stream.event(event).unwrap());
            }
        }
        framing.finish().unwrap();
        stream.finish().unwrap();
        for (sequence, value) in output.iter().enumerate() {
            assert_eq!(value["sequence_number"], sequence);
        }
        let complete = &output.last().unwrap()["response"];
        assert_eq!(complete["status"], "completed");
        assert_eq!(complete["output"][0]["content"][0]["text"], "합성 🧪");
        assert_eq!(complete["output"][1]["namespace"], "group");
        assert_eq!(complete["output"][1]["name"], "echo");
        assert_eq!(
            complete["output"][1]["arguments"],
            "{\"x\":9007199254740993123}"
        );
        assert_eq!(complete["output"][2]["type"], "custom_tool_call");
        assert_eq!(complete["output"][2]["input"], "한글\n\"quoted\"");
        assert_eq!(complete["output"][2]["call_id"], "custom_call");
        assert_eq!(complete["usage"]["total_tokens"], 22);
        assert_eq!(
            output
                .iter()
                .filter(|v| v["type"] == "response.custom_tool_call_input.delta")
                .count(),
            1
        );
        let text_delta = output
            .iter()
            .position(|v| v["type"] == "response.output_text.delta")
            .unwrap();
        let tool_added = output
            .iter()
            .position(|v| v["type"] == "response.output_item.added" && v["output_index"] == 1)
            .unwrap();
        assert!(text_delta < tool_added);
    }
}

#[test]
fn incomplete_error_and_invalid_streams_never_manufacture_completion() {
    let prepared = stream_prepared();
    let events = stream_events(
        prepared.payload["tools"][1]["name"].as_str().unwrap(),
        prepared.payload["tools"][2]["name"].as_str().unwrap(),
    );
    for end in 0..events.len() {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        for value in &events[..end] {
            assert!(
                stream
                    .event(event(value))
                    .unwrap()
                    .iter()
                    .all(|v| v["type"] != "response.completed")
            );
        }
        assert!(stream.finish().is_err());
    }
    for bad in [
        json!({"type":"error","error":{"type":"overloaded_error","message":"synthetic"}}),
        json!({"type":"unknown"}),
        json!({"type":"message_stop"}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"orphan"}}),
        json!({"type":"content_block_start","index":7,"content_block":{"type":"text","text":""}}),
    ] {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        stream.event(event(&events[0])).unwrap();
        assert!(stream.event(event(&bad)).is_err());
        assert!(stream.event(event(&events[2])).is_err());
        assert!(stream.finish().is_err());
    }
    // Repeated valid small deltas exceed the aggregate limit; single-event limits alone are insufficient.
    let mut stream = prepared.stream(512).unwrap();
    stream.event(event(&events[0])).unwrap();
    stream.event(event(&events[2])).unwrap();
    let chunk = json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x".repeat(100)}});
    for _ in 0..5 {
        stream.event(event(&chunk)).unwrap();
    }
    assert!(matches!(
        stream.event(event(&chunk)),
        Err(IrError::SizeLimit)
    ));
    assert!(stream.finish().is_err());
}

#[test]
fn invalid_custom_envelopes_and_duplicate_provider_json_fail_before_tool_completion() {
    let prepared = stream_prepared();
    let alias = prepared.payload["tools"][2]["name"].as_str().unwrap();
    let start = stream_events("unused", alias)[0].clone();
    for raw in [
        "{",
        "{}",
        "{\"input\":4}",
        "{\"input\":\"a\",\"input\":\"b\"}",
        "{\"input\":\"a\",\"extra\":true}",
    ] {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        stream.event(event(&start)).unwrap();
        stream.event(event(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"c","name":alias,"input":{}}}))).unwrap();
        let deltas = stream.event(event(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":raw}}))).unwrap();
        assert!(deltas.is_empty());
        assert!(
            stream
                .event(event(&json!({"type":"content_block_stop","index":0})))
                .is_err()
        );
        assert!(stream.finish().is_err());
    }
    let raw = format!(
        r#"{{"id":"msg_synthetic","type":"message","role":"assistant","model":"synthetic-model","content":[{{"type":"tool_use","id":"c","name":"{alias}","input":{{"input":"a","input":"b"}}}}],"stop_reason":"tool_use","usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    );
    assert!(prepared.decode_bytes(raw.as_bytes()).is_err());
}

#[test]
fn completed_call_status_roundtrips_but_unfinished_history_cannot_dispatch() {
    let mut body = request();
    body["store"] = json!(false);
    body["input"] = json!([
        {"role":"user","content":"start"},
        {"type":"function_call","name":"echo","call_id":"call_status","status":"completed","arguments":"{}"},
        {"type":"function_call_output","call_id":"call_status","output":"result"}
    ]);
    let ir = responses::decode(body.clone(), None).unwrap();
    assert_eq!(responses::encode(&ir, None).unwrap(), body);
    encode(&ir, &target()).unwrap();
    for status in ["in_progress", "incomplete", "unknown"] {
        body["input"][1]["status"] = json!(status);
        assert!(
            responses::decode(body.clone(), None)
                .and_then(|ir| encode(&ir, &target()))
                .is_err()
        );
    }
}

#[test]
fn messages_native_controls_preserve_schema_and_reject_unrepresentable_options() {
    let mut body = request();
    body["tools"][0]["strict"] = json!(true);
    body["reasoning"] = json!({"effort":"high"});
    let schema = json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false});
    body["text"] = json!({"format":{"type":"json_schema","name":"synthetic_answer","schema":schema,"strict":true}});
    let prepared = encode(&responses::decode(body.clone(), None).unwrap(), &target()).unwrap();
    assert_eq!(prepared.payload["tools"][0]["strict"], true);
    assert_eq!(
        prepared.payload["output_config"],
        json!({"effort":"high","format":{"type":"json_schema","schema":schema}})
    );
    assert_eq!(prepared.decode(response()).unwrap()["text"], body["text"]);
    for feature in [
        Feature::ReasoningEffort,
        Feature::StructuredOutput,
        Feature::StrictToolArguments,
    ] {
        let mut limited = target();
        limited.route.capabilities.support.remove(&feature);
        assert!(encode(&responses::decode(body.clone(), None).unwrap(), &limited).is_err());
    }
    for effort in ["none", "minimal", "ultra"] {
        body["reasoning"]["effort"] = json!(effort);
        assert!(encode(&responses::decode(body.clone(), None).unwrap(), &target()).is_err());
    }
}

#[test]
fn messages_replay_preserves_tool_then_text_without_crossing_result_boundary() {
    let mut body = request();
    let prepared = encode(&responses::decode(body.clone(), None).unwrap(), &target()).unwrap();
    let mut upstream = response();
    upstream["stop_reason"] = json!("tool_use");
    upstream["content"] = json!([
        {"type":"tool_use","id":"c1","name":"echo","input":{"text":"x"}},
        {"type":"text","text":"after tool"},
        {"type":"tool_use","id":"c2","name":"echo","input":{"text":"y"}}
    ]);
    let decoded = prepared.decode(upstream.clone()).unwrap();
    let mut items = vec![json!({"role":"user","content":"begin"})];
    items.extend(decoded["output"].as_array().unwrap().clone());
    items.push(json!({"type":"function_call_output","call_id":"c1","output":"one"}));
    items.push(json!({"type":"function_call_output","call_id":"c2","output":"two"}));
    body["input"] = json!(items);
    let replay = encode(&responses::decode(body.clone(), None).unwrap(), &target()).unwrap();
    assert_eq!(
        replay.payload["messages"][1]["content"],
        upstream["content"]
    );
    body["input"].as_array_mut().unwrap().insert(
        5,
        json!({"role":"assistant","content":"after partial results"}),
    );
    assert!(encode(&responses::decode(body, None).unwrap(), &target()).is_err());
}

#[test]
fn messages_nonstream_large_text_uses_bounded_ir_deltas_without_truncation() {
    let prepared = encode(&responses::decode(request(), None).unwrap(), &target()).unwrap();
    let text = "합성🧪".repeat(150_000);
    let mut upstream = response();
    upstream["content"][0]["text"] = json!(text);
    assert_eq!(
        prepared.decode(upstream).unwrap()["output"][0]["content"][0]["text"],
        text
    );
}
