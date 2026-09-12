use agent_response_gateway::{
    adapters::interactions::{PreparedInteractions, ProviderHistory},
    ir::{
        ApiProtocol,
        capability::{BridgeRule, CapabilityProfile, Feature, Support},
        continuity::{ContinuityBinding, RouteSnapshot},
        responses,
    },
};
use serde_json::{Value, json};
fn target() -> ContinuityBinding {
    use Feature::*;
    let mut support = [
        Instructions,
        InstructionHierarchy,
        Images,
        FunctionTools,
        StrictToolArguments,
        StructuredOutput,
        StrictStructuredOutput,
        ReasoningEffort,
        ReasoningSummary,
        ToolChoice,
        ParallelToolControl,
        MaxOutputTokens,
        Temperature,
        TopP,
    ]
    .into_iter()
    .map(|f| (f, Support::Native))
    .collect::<std::collections::BTreeMap<_, _>>();
    for (feature, rule) in [
        (CustomTools, BridgeRule::CustomToolJson),
        (CustomGrammar, BridgeRule::CodexPatchGrammar),
        (NamespacedTools, BridgeRule::ToolNamespace),
    ] {
        support.insert(feature, Support::Bridged(rule));
    }
    support.insert(
        InstructionHierarchy,
        Support::Bridged(BridgeRule::GeminiInstructionEnvelope),
    );
    ContinuityBinding {
        scope: "synthetic".into(),
        route: RouteSnapshot {
            provider_id: "mock".into(),
            model: "actual-model".into(),
            api: ApiProtocol::GeminiInteractions,
            credential_binding: "generation-1".into(),
            adapter_version: "1".into(),
            capabilities: CapabilityProfile {
                reasoning_contract: None,
                id: "chat".into(),
                version: "1".into(),
                protocol: ApiProtocol::GeminiInteractions,
                support,
            },
            context_window: Some(32000),
            max_output_tokens: Some(1024),
        },
    }
}
fn request() -> Value {
    json!({"model":"writer","input":"synthetic","tools":[{"type":"function","name":"echo","description":"Synthetic echo","parameters":{"type":"object","properties":{"text":{"type":"string"}}},"strict":false}]})
}
fn prepare(body: Value) -> Result<PreparedInteractions, agent_response_gateway::ir::IrError> {
    let r = responses::decode(body, None)?;
    let p = agent_response_gateway::ir::capability::plan_translation(&r, &target())?;
    PreparedInteractions::encode(&r, &p, &ProviderHistory::default())
}
fn response(steps: Value, status: &str) -> Value {
    json!({"id":"provider-id","object":"interaction","model":"actual-model","status":status,"steps":steps,"usage":{"total_input_tokens":10,"total_output_tokens":3,"total_thought_tokens":2,"total_cached_tokens":4,"total_tokens":15}})
}
#[test]
fn request_has_explicit_stateless_native_fields() {
    let p = prepare(request()).unwrap();
    assert_eq!(p.payload["store"], false);
    assert_eq!(p.payload["background"], false);
    assert_eq!(p.payload["tools"][0]["type"], "function");
    assert_eq!(p.payload["input"][0]["type"], "user_input");
}
#[test]
fn original_steps_and_signature_survive_output_projection() {
    let p = prepare(request()).unwrap();
    let steps = json!([{"type":"thought","signature":"synthetic-signature"},{"type":"function_call","id":"c1","name":"echo","arguments":{"text":"hello"}}]);
    let out = p
        .decode(response(steps.clone(), "requires_action"), "resp_local")
        .unwrap();
    assert_eq!(out.steps, steps.as_array().unwrap().clone());
    assert_eq!(out.response["output"][0]["call_id"], "c1");
    assert_eq!(out.response["usage"]["output_tokens"], 5);
    assert_eq!(out.provider_status, "requires_action");
}
#[test]
fn unsupported_wire_features_fail_before_send() {
    for (field, value) in [
        ("temperature", json!(0.2)),
        ("parallel_tool_calls", json!(false)),
        ("reasoning", json!({"effort":"xhigh"})),
        (
            "input",
            json!([{"role":"user","content":[{"type":"input_image","image_url":"https://example.test/a.png"}]}]),
        ),
    ] {
        let mut r = request();
        r[field] = value;
        assert!(prepare(r).is_err());
    }
    let mut r = request();
    r["tools"][0]["strict"] = json!(true);
    assert!(prepare(r).is_err());
}
#[test]
fn instruction_bridge_preserves_roles_and_rejects_late_instructions() {
    let mut r = request();
    r["input"] = json!([{"role":"developer","content":"before"},{"role":"user","content":"hello"}]);
    let p = prepare(r.clone()).unwrap();
    assert!(
        p.payload["system_instruction"]
            .as_str()
            .unwrap()
            .contains("developer")
    );
    r["input"] = json!([{"role":"user","content":"hello"},{"role":"developer","content":"late"}]);
    assert!(prepare(r).is_err());
}
#[test]
fn terminal_tool_identity_and_unknown_steps_are_validated() {
    let p = prepare(request()).unwrap();
    for (steps, status) in [
        (
            json!([{"type":"function_call","id":"c1","name":"other","arguments":{}}]),
            "requires_action",
        ),
        (
            json!([{"type":"function_call","id":"c1","name":"echo","arguments":{}}]),
            "completed",
        ),
        (json!([{"type":"google_search_call"}]), "completed"),
    ] {
        assert!(p.decode(response(steps, status), "resp_test").is_err());
    }
}
fn frame(kind: &str, v: Value) -> agent_response_gateway::adapters::sse::SseEvent {
    let mut v = v;
    v["event_type"] = json!(kind);
    agent_response_gateway::adapters::sse::SseEvent {
        event: kind.into(),
        data: v.to_string(),
    }
}
#[test]
fn stream_reassembles_tools_and_rejects_missing_terminal() {
    let p = prepare(request()).unwrap();
    let mut s = p.stream(4096, "resp_test".into());
    s.event(frame("interaction.created",json!({"interaction":{"id":"provider-id","model":"actual-model","object":"interaction","status":"in_progress"}}))).unwrap();
    s.event(frame(
        "step.start",
        json!({"index":0,"step":{"type":"function_call","id":"c1","name":"echo"}}),
    ))
    .unwrap();
    for a in ["{\"text\":", "\"한글\"}"] {
        s.event(frame(
            "step.delta",
            json!({"index":0,"delta":{"type":"arguments_delta","arguments":a}}),
        ))
        .unwrap();
    }
    s.event(frame("step.stop", json!({"index":0}))).unwrap();
    s.event(frame("interaction.completed",json!({"interaction":{"id":"provider-id","model":"actual-model","object":"interaction","status":"requires_action"}}))).unwrap();
    assert!(!s.is_complete());
    s.event(agent_response_gateway::adapters::sse::SseEvent {
        event: "done".into(),
        data: "[DONE]".into(),
    })
    .unwrap();
    let out = s.finish().unwrap();
    assert_eq!(out.steps[0]["arguments"]["text"], "한글");
    assert!(p.stream(4096, "resp_other".into()).finish().is_err());
}

#[test]
fn structured_output_and_function_arguments_must_match_declared_schema() {
    let mut r = request();
    r["text"] = json!({"format":{"type":"json_schema","name":"result","strict":true,"schema":{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}}});
    let p = prepare(r).unwrap();
    for text in [
        "not JSON",
        "{}",
        "{\"answer\":2}",
        "{\"answer\":\"yes\",\"extra\":0}",
    ] {
        assert!(
            p.decode(
                response(
                    json!([{"type":"model_output","content":[{"type":"text","text":text}]}]),
                    "completed"
                ),
                "resp_invalid"
            )
            .is_err()
        );
    }
    assert!(p.decode(response(json!([{"type":"model_output","content":[{"type":"text","text":"{\"answer\":\"yes\"}"}]}]),"completed"),"resp_valid").is_ok());
    assert!(prepare(request()).unwrap().decode(response(json!([{"type":"function_call","id":"call_1","name":"echo","arguments":{"text":42}}]),"requires_action"),"resp_args").is_err());
}

#[test]
fn missing_usage_stays_unknown_and_inconsistent_totals_fail() {
    let p = prepare(request()).unwrap();
    let mut r = response(
        json!([{"type":"thought","signature":"synthetic"}]),
        "completed",
    );
    r.as_object_mut().unwrap().remove("usage");
    assert!(
        p.decode(r.clone(), "resp_opaque").unwrap().response["usage"]["output_tokens"].is_null()
    );
    r["usage"] = json!({"total_input_tokens":10,"total_output_tokens":3,"total_thought_tokens":2,"total_tokens":13});
    assert!(p.decode(r, "resp_bad_usage").is_err());
}

#[test]
fn truncated_json_and_out_of_order_stream_steps_fail() {
    let p = prepare(request()).unwrap();
    assert!(p.decode_bytes(b"{\"id\":", "resp_truncated").is_err());
    let mut s = p.stream(4096, "resp_order".into());
    assert!(
        s.event(frame(
            "step.start",
            json!({"index":0,"step":{"type":"thought"}})
        ))
        .is_err()
    );
    let mut s = p.stream(4096, "resp_order2".into());
    s.event(frame("interaction.created",json!({"interaction":{"id":"provider-id","model":"actual-model","object":"interaction","status":"in_progress"}}))).unwrap();
    assert!(
        s.event(frame(
            "step.start",
            json!({"index":1,"step":{"type":"thought"}})
        ))
        .is_err()
    );
}

#[test]
fn json_and_sse_reconstruct_identical_provider_steps_and_public_output() {
    let p = prepare(request()).unwrap();
    let steps = json!([{"type":"thought","signature":"synthetic-split"},{"type":"model_output","content":[{"type":"text","text":"한글 text"}]}]);
    let expected = p
        .decode(response(steps.clone(), "completed"), "resp_equivalent")
        .unwrap();
    let mut s = p.stream(16384, "resp_equivalent".into());
    s.event(frame("interaction.created",json!({"interaction":{"id":"provider-id","model":"actual-model","object":"interaction","status":"in_progress"}}))).unwrap();
    s.event(frame(
        "step.start",
        json!({"index":0,"step":{"type":"thought"}}),
    ))
    .unwrap();
    for signature in ["synthetic-", "split"] {
        s.event(frame(
            "step.delta",
            json!({"index":0,"delta":{"type":"thought_signature","signature":signature}}),
        ))
        .unwrap();
    }
    s.event(frame("step.stop", json!({"index":0}))).unwrap();
    s.event(frame(
        "step.start",
        json!({"index":1,"step":{"type":"model_output"}}),
    ))
    .unwrap();
    for text in ["한글 ", "text"] {
        s.event(frame(
            "step.delta",
            json!({"index":1,"delta":{"type":"text","text":text}}),
        ))
        .unwrap();
    }
    s.event(frame("step.stop", json!({"index":1}))).unwrap();
    s.event(frame(
        "interaction.completed",
        json!({"interaction":response(steps,"completed")}),
    ))
    .unwrap();
    s.event(agent_response_gateway::adapters::sse::SseEvent {
        event: "done".into(),
        data: "[DONE]".into(),
    })
    .unwrap();
    let actual = s.finish().unwrap();
    assert_eq!(actual.steps, expected.steps);
    assert_eq!(actual.response["output"], expected.response["output"]);
    assert_eq!(actual.response["usage"], expected.response["usage"]);
    assert_eq!(actual.provider_status, expected.provider_status);
}

#[test]
fn colliding_namespace_names_restore_original_tool_identities() {
    let mut r = request();
    r["tools"] = json!([
        {"type":"namespace","name":"left","tools":[{"type":"function","name":"echo","parameters":{"type":"object","properties":{}}}]},
        {"type":"namespace","name":"right","tools":[{"type":"function","name":"echo","parameters":{"type":"object","properties":{}}}]}
    ]);
    let p = prepare(r).unwrap();
    let tools = p.payload["tools"].as_array().unwrap();
    assert_ne!(tools[0]["name"], tools[1]["name"]);
    let steps=tools.iter().enumerate().map(|(i,t)| json!({"type":"function_call","id":format!("call_{i}"),"name":t["name"],"arguments":{}})).collect::<Vec<_>>();
    let decoded = p
        .decode(response(json!(steps), "requires_action"), "resp_namespaces")
        .unwrap();
    let mut namespaces = decoded.response["output"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert_eq!(item["name"], "echo");
            item["namespace"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    namespaces.sort_unstable();
    assert_eq!(namespaces, ["left", "right"]);
}

#[test]
fn v1_partial_stream_metadata_and_cumulative_usage_are_supported() {
    let p = prepare(request()).unwrap();
    let mut s = p.stream(16384, "resp_partial".into());
    s.event(frame(
        "interaction.created",
        json!({"interaction":{"id":"provider-id","status":"in_progress"}}),
    ))
    .unwrap();
    s.event(frame(
        "step.start",
        json!({"index":0,"step":{"type":"model_output"}}),
    ))
    .unwrap();
    s.event(frame("step.delta",json!({"index":0,"delta":{"type":"text","text":"synthetic"},"metadata":{"total_usage":{"total_input_tokens":10,"total_output_tokens":2,"total_thought_tokens":1,"total_tokens":13}}}))).unwrap();
    s.event(frame("step.stop",json!({"index":0,"step_usage":{"total_output_tokens":2},"usage":{"total_input_tokens":10,"total_output_tokens":3,"total_thought_tokens":1,"total_tokens":14}}))).unwrap();
    s.event(frame(
        "interaction.status_update",
        json!({"interaction_id":"provider-id","status":"completed"}),
    ))
    .unwrap();
    s.event(frame(
        "interaction.completed",
        json!({"interaction":{"id":"provider-id","status":"completed"}}),
    ))
    .unwrap();
    s.event(agent_response_gateway::adapters::sse::SseEvent {
        event: "done".into(),
        data: "[DONE]".into(),
    })
    .unwrap();
    assert_eq!(s.finish().unwrap().response["usage"]["output_tokens"], 4);
    let mut r = response(
        json!([{"type":"thought","signature":"synthetic"}]),
        "completed",
    );
    r.as_object_mut().unwrap().remove("object");
    r.as_object_mut().unwrap().remove("model");
    assert!(p.decode(r.clone(), "resp_optional").is_ok());
    r["service_tier"] = json!("invented");
    assert!(p.decode(r, "resp_unknown").is_err());
}
