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
        json!({"tools":[{"type":"function","name":"echo","strict":true}]}),
        json!({"tools":[{"type":"custom","name":"custom"}]}),
        json!({"text":{"format":{"type":"json_schema","name":"out","schema":{"type":"object"},"strict":true}}}),
        json!({"reasoning":{"effort":"high"}}),
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
        json!({"input_tokens":17,"output_tokens":3,"total_tokens":20})
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
