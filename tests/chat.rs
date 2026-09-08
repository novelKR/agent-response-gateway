use agent_response_gateway::{
    adapters::chat::encode,
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
    ContinuityBinding {
        scope: "synthetic".into(),
        route: RouteSnapshot {
            provider_id: "mock".into(),
            model: "actual-model".into(),
            api: ApiProtocol::ChatCompletions,
            credential_binding: "generation-1".into(),
            adapter_version: "1".into(),
            capabilities: CapabilityProfile {
                id: "chat".into(),
                version: "1".into(),
                protocol: ApiProtocol::ChatCompletions,
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
fn response() -> Value {
    json!({"id":"chat_synthetic","object":"chat.completion","created":1234567890,"model":"actual-model","choices":[{"index":0,"message":{"role":"assistant","content":"합성 🧪","refusal":null,"annotations":[]},"finish_reason":"stop","logprobs":null}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})
}
#[test]
fn chat_preserves_roles_instruction_position_options_and_parameter_values() {
    let mut body = request();
    body["instructions"] = json!("top instruction");
    body["input"] = json!([
        {"role":"system","content":"system"}, {"role":"user","content":"first"},
        {"role":"developer","content":[{"type":"input_text","text":"late explicit developer"}]},
        {"role":"assistant","content":"earlier assistant"},
        {"role":"user","content":[{"type":"input_image","image_url":"https://example.test/image.png","detail":"high"},{"type":"input_text","text":"final user"}]}
    ]);
    body["max_output_tokens"] = json!(900);
    body["temperature"] = json!(1.7);
    body["top_p"] = json!(0.5);
    body["parallel_tool_calls"] = json!(false);
    body["stream"] = json!(true);
    body["tool_choice"] = json!({"type":"function","name":"echo"});
    let prepared = encode(&responses::decode(body, None).unwrap(), &target()).unwrap();
    assert_eq!(prepared.payload["model"], "actual-model");
    assert_eq!(prepared.payload["max_completion_tokens"], 900);
    assert!(prepared.payload.get("max_tokens").is_none());
    assert_eq!(prepared.payload["temperature"], json!(1.7));
    assert_eq!(prepared.payload["parallel_tool_calls"], false);
    assert_eq!(
        prepared.payload["stream_options"],
        json!({"include_usage":true})
    );
    assert_eq!(
        prepared.payload["messages"][0],
        json!({"role":"system","content":"top instruction"})
    );
    let roles: Vec<_> = prepared.payload["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(
        roles,
        vec!["system", "system", "user", "developer", "assistant", "user"]
    );
    assert_eq!(
        prepared.payload["messages"][3]["content"][0]["text"],
        "late explicit developer"
    );
    assert_eq!(
        prepared.payload["messages"][5]["content"][0]["image_url"]["detail"],
        "high"
    );
    assert_eq!(
        prepared.payload["tool_choice"],
        json!({"type":"function","function":{"name":"echo"}})
    );
    assert_eq!(prepared.payload["tools"][0]["function"]["strict"], false);
    assert_eq!(prepared.payload["store"], false);
}
#[test]
fn chat_namespace_custom_choice_and_parallel_history_share_one_identity_map() {
    let mut body = request();
    body["tools"].as_array_mut().unwrap().push(json!({"type":"namespace","name":"group","description":"Group fixture","tools":[{"type":"function","name":"echo"},{"type":"custom","name":"patch","format":{"type":"text"}}]}));
    body["tool_choice"] = json!({"type":"custom","namespace":"group","name":"patch"});
    body["input"] = json!([
        {"role":"user","content":"begin"}, {"role":"assistant","content":"tool preface"},
        {"type":"function_call","name":"echo","namespace":"group","call_id":"function_id","arguments":"{ \"n\":9007199254740993123 }"},
        {"type":"custom_tool_call","name":"patch","namespace":"group","call_id":"custom_id","status":"completed","input":"exact\n\"한글\""},
        {"type":"custom_tool_call_output","call_id":"custom_id","output":"custom result"},
        {"type":"function_call_output","call_id":"function_id","output":"function result"}
    ]);
    let prepared = encode(&responses::decode(body, None).unwrap(), &target()).unwrap();
    let tools = prepared.payload["tools"].as_array().unwrap();
    let alias = tools[2]["function"]["name"].as_str().unwrap();
    assert_eq!(prepared.payload["tool_choice"]["function"]["name"], alias);
    let calls = &prepared.payload["messages"][1]["tool_calls"];
    assert_eq!(calls[0]["function"]["name"], tools[1]["function"]["name"]);
    assert_eq!(
        calls[0]["function"]["arguments"],
        "{ \"n\":9007199254740993123 }"
    );
    assert_eq!(calls[1]["id"], "custom_id");
    let envelope: Value =
        serde_json::from_str(calls[1]["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["input"], "exact\n\"한글\"");
    assert_eq!(prepared.payload["messages"][2]["tool_call_id"], "custom_id");
    assert_eq!(
        prepared.payload["messages"][3]["tool_call_id"],
        "function_id"
    );
    let mut upstream = response();
    upstream["choices"][0]["message"] = json!({"role":"assistant","content":null,"tool_calls":[{"id":"returned_custom_id","type":"function","function":{"name":alias,"arguments":"{\"input\":\"exact\\n\\\"한글\\\"\"}"}}]});
    upstream["choices"][0]["finish_reason"] = json!("tool_calls");
    let decoded = prepared.decode(upstream).unwrap();
    assert_eq!(decoded["output"][0]["type"], "custom_tool_call");
    assert_eq!(decoded["output"][0]["namespace"], "group");
    assert_eq!(decoded["output"][0]["name"], "patch");
    assert_eq!(decoded["output"][0]["call_id"], "returned_custom_id");
    assert_eq!(decoded["output"][0]["input"], "exact\n\"한글\"");
}
#[test]
fn chat_usage_optional_counts_creation_time_and_length_are_not_fabricated() {
    let prepared = encode(&responses::decode(request(), None).unwrap(), &target()).unwrap();
    let mut upstream = response();
    upstream["usage"]["prompt_tokens_details"] = json!({"cached_tokens":4});
    upstream["usage"]["completion_tokens_details"] = json!({"reasoning_tokens":2});
    let output = prepared.decode(upstream.clone()).unwrap();
    assert_eq!(output["created_at"], 1234567890);
    assert_eq!(
        output["usage"],
        json!({"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_tokens_details":{"cached_tokens":4},"output_tokens_details":{"reasoning_tokens":2}})
    );
    upstream["choices"][0]["finish_reason"] = json!("length");
    upstream.as_object_mut().unwrap().remove("usage");
    let output = prepared.decode(upstream).unwrap();
    assert_eq!(output["status"], "incomplete");
    assert_eq!(output["incomplete_details"]["reason"], "max_output_tokens");
    assert_eq!(output["output"][0]["status"], "incomplete");
    assert!(output["usage"].is_null());
}
#[test]
fn chat_rejects_unimplemented_semantics_even_when_the_profile_claims_native() {
    for change in [
        json!({"reasoning":{"summary":"detailed"}}),
        json!({"reasoning":{"effort":"unmapped-effort"}}),
        json!({"text":{"verbosity":"high"}}),
        json!({"unknown_required":true}),
        json!({"max_output_tokens":1025}),
        json!({"temperature":2.1}),
        json!({"input":[{"role":"assistant","content":[{"type":"input_image","image_url":"https://example.test/image.png"}]}]}),
    ] {
        let mut body = request();
        body.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(encode(&responses::decode(body, None).unwrap(), &target()).is_err());
    }
    let mut body = request();
    body["tools"] = json!([{"type":"custom","name":"native-custom"}]);
    let mut target = target();
    target
        .route
        .capabilities
        .support
        .insert(Feature::CustomTools, Support::Native);
    assert!(encode(&responses::decode(body, None).unwrap(), &target).is_err());
}
#[test]
fn chat_rejects_unknown_output_refusal_legacy_tools_wrong_usage_and_duplicate_keys() {
    let prepared = encode(&responses::decode(request(), None).unwrap(), &target()).unwrap();
    for change in [
        json!({"model":"wrong"}),
        json!({"created":-1}),
        json!({"choices":[]}),
        json!({"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":99}}),
        json!({"usage":{"prompt_tokens":18446744073709551615_u64,"completion_tokens":1,"total_tokens":0}}),
        json!({"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":11}}}),
    ] {
        let mut value = response();
        value
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(prepared.decode(value).is_err());
    }
    for change in [
        json!({"refusal":"refused"}),
        json!({"annotations":[{"unmapped":"citation"}]}),
        json!({"reasoning_content":"hidden"}),
        json!({"function_call":{"name":"echo","arguments":"{}"}}),
        json!({"tool_calls":[{"id":"c","type":"function","function":{"name":"missing","arguments":"{}"}}]}),
    ] {
        let mut value = response();
        value["choices"][0]["message"]
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(prepared.decode(value).is_err());
    }
    for finish in ["content_filter", "function_call", "tool_calls", "unknown"] {
        let mut value = response();
        value["choices"][0]["finish_reason"] = json!(finish);
        assert!(prepared.decode(value).is_err());
    }
    assert!(
        prepared
            .decode_bytes(b"{\"object\":\"chat.completion\",\"object\":\"other\"}")
            .is_err()
    );
}
#[test]
fn parameterless_function_schema_forbids_undeclared_properties_on_both_adapters() {
    let body =
        json!({"model":"writer","input":"synthetic","tools":[{"type":"function","name":"empty"}]});
    let ir = responses::decode(body, None).unwrap();
    let chat = encode(&ir, &target()).unwrap();
    assert_eq!(
        chat.payload["tools"][0]["function"]["parameters"],
        json!({"type":"object","properties":{},"additionalProperties":false})
    );
    let mut messages_target = target();
    messages_target.route.api = ApiProtocol::Messages;
    messages_target.route.capabilities.protocol = ApiProtocol::Messages;
    let messages =
        agent_response_gateway::adapters::messages::encode(&ir, &messages_target).unwrap();
    assert_eq!(
        messages.payload["tools"][0]["input_schema"],
        chat.payload["tools"][0]["function"]["parameters"]
    );
}

#[test]
fn chat_preserves_explicit_effort_structured_schema_and_strict_tools() {
    let mut body = request();
    let schema = json!({"type":"object","properties":{"schema":{"const":"synthetic.result/v1"},"answer":{"type":"string"}},"required":["schema","answer"],"additionalProperties":false});
    body["reasoning"] = json!({"effort":"xhigh"});
    body["text"] = json!({"format":{"type":"json_schema","name":"synthetic_result","schema":schema,"strict":true}});
    body["tools"][0]["strict"] = json!(true);
    let ir = responses::decode(body, None).unwrap();
    let prepared = encode(&ir, &target()).unwrap();
    assert_eq!(prepared.payload["reasoning_effort"], "xhigh");
    assert_eq!(
        prepared.payload["response_format"],
        json!({"type":"json_schema","json_schema":{"name":"synthetic_result","schema":schema,"strict":true}})
    );
    assert_eq!(prepared.payload["tools"][0]["function"]["strict"], true);
    for feature in [
        Feature::ReasoningEffort,
        Feature::StructuredOutput,
        Feature::StrictStructuredOutput,
        Feature::StrictToolArguments,
    ] {
        let mut target = target();
        target.route.capabilities.support.remove(&feature);
        assert!(encode(&ir, &target).is_err());
    }
}

fn chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"chat_synthetic","object":"chat.completion.chunk","created":1234567890,"model":"actual-model","choices":[{"index":0,"delta":delta,"finish_reason":finish,"logprobs":null}],"usage":null})
}
fn stream_event(value: &Value) -> agent_response_gateway::adapters::sse::SseEvent {
    agent_response_gateway::adapters::sse::SseEvent {
        event: "message".into(),
        data: value.to_string(),
    }
}
fn done_event() -> agent_response_gateway::adapters::sse::SseEvent {
    agent_response_gateway::adapters::sse::SseEvent {
        event: "message".into(),
        data: "[DONE]".into(),
    }
}
fn custom_prepared() -> agent_response_gateway::adapters::chat::PreparedChat {
    let mut body = request();
    body["tools"].as_array_mut().unwrap().push(json!({"type":"namespace","name":"group","tools":[{"type":"custom","name":"patch","format":{"type":"text"}}]}));
    encode(&responses::decode(body, None).unwrap(), &target()).unwrap()
}
fn parallel_chunks(alias: &str) -> Vec<Value> {
    let cut = alias.len() / 2;
    let mut events = vec![
        chunk(
            json!({"role":"assistant","content":null,"tool_calls":[{"index":1,"id":"custom_","type":"function","function":{"name":&alias[..cut],"arguments":"{\"input\":\"한글"}}]}),
            Value::Null,
        ),
        chunk(
            json!({"tool_calls":[{"index":0,"id":"function_","type":"function","function":{"name":"ec","arguments":"{\"text\":"}}]}),
            Value::Null,
        ),
        chunk(json!({"content":"합성 🧪"}), Value::Null),
        chunk(
            json!({"tool_calls":[{"index":1,"id":"id","function":{"name":&alias[cut..],"arguments":"\\n\\\"quoted\\\"\"}"}},{"index":0,"id":"id","function":{"name":"ho","arguments":"\"synthetic\"}"}}]}),
            Value::Null,
        ),
        chunk(json!({}), json!("tool_calls")),
    ];
    let mut usage = chunk(json!({}), Value::Null);
    usage["choices"] = json!([]);
    usage["usage"] = json!({"prompt_tokens":10,"completion_tokens":5,"total_tokens":15});
    events.push(usage);
    events
}

#[test]
fn chat_stream_handles_every_byte_split_late_text_parallel_ids_names_and_arguments() {
    use agent_response_gateway::adapters::sse::SseDecoder;
    let prepared = custom_prepared();
    let alias = prepared.payload["tools"][1]["function"]["name"]
        .as_str()
        .unwrap();
    let chunks = parallel_chunks(alias);
    let wire = (chunks
        .iter()
        .map(|v| format!("data: {v}\r\n\r\n"))
        .collect::<String>()
        + "data: [DONE]\r\n\r\n")
        .into_bytes();
    for split in 0..=wire.len() {
        let mut framing = SseDecoder::new(1024 * 1024).unwrap();
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        let mut events = Vec::new();
        for bytes in [&wire[..split], &wire[split..]] {
            let mut rest = bytes;
            while let Some(event) = framing.next_event(&mut rest).unwrap() {
                events.extend(stream.event(event).unwrap());
            }
        }
        framing.finish().unwrap();
        stream.finish().unwrap();
        for (sequence, event) in events.iter().enumerate() {
            assert_eq!(event["sequence_number"], sequence);
        }
        let output = &events.last().unwrap()["response"]["output"];
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["content"][0]["text"], "합성 🧪");
        assert_eq!(output[1]["type"], "function_call");
        assert_eq!(output[1]["call_id"], "function_id");
        assert_eq!(output[1]["name"], "echo");
        assert_eq!(output[1]["arguments"], "{\"text\":\"synthetic\"}");
        assert_eq!(output[2]["call_id"], "custom_id");
        assert_eq!(output[2]["namespace"], "group");
        assert_eq!(output[2]["name"], "patch");
        assert_eq!(output[2]["input"], "한글\n\"quoted\"");
        assert_eq!(
            events.last().unwrap()["response"]["usage"]["total_tokens"],
            15
        );
        let first_text = events
            .iter()
            .position(|v| v["type"] == "response.output_text.delta")
            .unwrap();
        let first_tool = events
            .iter()
            .position(|v| {
                v["type"] == "response.output_item.added" && v["item"]["type"] == "function_call"
            })
            .unwrap();
        assert!(first_text < first_tool);
        let mut whole = response();
        whole["choices"][0]["message"] = json!({"role":"assistant","content":"합성 🧪","tool_calls":[
            {"id":"function_id","type":"function","function":{"name":"echo","arguments":"{\"text\":\"synthetic\"}"}},
            {"id":"custom_id","type":"function","function":{"name":alias,"arguments":"{\"input\":\"한글\\n\\\"quoted\\\"\"}"}}
        ]});
        whole["choices"][0]["finish_reason"] = json!("tool_calls");
        assert_eq!(
            events.last().unwrap()["response"],
            prepared.decode(whole).unwrap()
        );
    }
}

#[test]
fn chat_stream_needs_both_finish_and_done_and_never_completes_on_eof() {
    let prepared = custom_prepared();
    let alias = prepared.payload["tools"][1]["function"]["name"]
        .as_str()
        .unwrap();
    let chunks = parallel_chunks(alias);
    for end in 0..=chunks.len() {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        for value in &chunks[..end] {
            assert!(
                stream
                    .event(stream_event(value))
                    .unwrap()
                    .iter()
                    .all(|v| v["type"] != "response.completed")
            );
        }
        assert!(stream.finish().is_err());
        if end < 5 {
            assert!(stream.event(done_event()).is_err());
        } else {
            stream.event(done_event()).unwrap();
            stream.finish().unwrap();
        }
    }
    let mut stream = prepared.stream(1024 * 1024).unwrap();
    stream
        .event(stream_event(&chunk(
            json!({"role":"assistant","content":"partial"}),
            Value::Null,
        )))
        .unwrap();
    let events = stream
        .event(stream_event(&chunk(json!({}), json!("length"))))
        .unwrap();
    assert!(!events.iter().any(|v| v["type"] == "response.incomplete"));
    let events = stream.event(done_event()).unwrap();
    assert_eq!(events[0]["type"], "response.incomplete");
    assert!(events[0]["response"]["usage"].is_null());
    assert_eq!(
        events[0]["response"]["incomplete_details"]["reason"],
        "max_output_tokens"
    );
}

#[test]
fn chat_stream_rejects_changed_identity_unknown_deltas_and_post_finish_output() {
    let prepared = custom_prepared();
    for change in [
        json!({"id":"changed"}),
        json!({"model":"changed"}),
        json!({"created":2}),
        json!({"object":"chat.completion"}),
        json!({"choices":[{"index":1,"delta":{},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"reasoning_content":"unmapped"},"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{"refusal":"refused"},"finish_reason":null}]}),
    ] {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        stream
            .event(stream_event(&chunk(
                json!({"role":"assistant"}),
                Value::Null,
            )))
            .unwrap();
        let mut value = chunk(json!({}), Value::Null);
        value
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(stream.event(stream_event(&value)).is_err());
        assert!(stream.event(done_event()).is_err());
        assert!(stream.finish().is_err());
    }
    let mut stream = prepared.stream(1024 * 1024).unwrap();
    stream
        .event(stream_event(&chunk(
            json!({"content":"done"}),
            json!("stop"),
        )))
        .unwrap();
    assert!(
        stream
            .event(stream_event(&chunk(json!({"content":"late"}), Value::Null)))
            .is_err()
    );
}

#[test]
fn chat_stream_invalid_tool_wrappers_and_incomplete_json_emit_no_tool_completion() {
    let prepared = custom_prepared();
    let alias = prepared.payload["tools"][1]["function"]["name"]
        .as_str()
        .unwrap();
    for raw in [
        "{",
        "{}",
        "{\"input\":4}",
        "{\"input\":\"a\",\"input\":\"b\"}",
        "{\"input\":\"a\",\"extra\":true}",
    ] {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        let events=stream.event(stream_event(&chunk(json!({"tool_calls":[{"index":0,"id":"custom_id","type":"function","function":{"name":alias,"arguments":raw}}]}),Value::Null))).unwrap();
        assert!(
            !events
                .iter()
                .any(|v| v["type"] == "response.output_item.added")
        );
        assert!(
            stream
                .event(stream_event(&chunk(json!({}), json!("tool_calls"))))
                .is_err()
        );
        assert!(stream.finish().is_err());
    }
    let mut stream = prepared.stream(1024 * 1024).unwrap();
    stream.event(stream_event(&chunk(json!({"tool_calls":[{"index":0,"id":"c","type":"function","function":{"name":"echo","arguments":"{"}}]}),Value::Null))).unwrap();
    assert!(
        stream
            .event(stream_event(&chunk(json!({}), json!("length"))))
            .is_err()
    );
}

#[test]
fn chat_stream_bounds_aggregate_fragments_and_checks_tool_identity_before_completion() {
    let prepared = custom_prepared();
    let mut stream = prepared.stream(1024).unwrap();
    for _ in 0..5 {
        stream
            .event(stream_event(&chunk(
                json!({"content":"x".repeat(200)}),
                Value::Null,
            )))
            .unwrap();
    }
    assert!(
        stream
            .event(stream_event(&chunk(
                json!({"content":"x".repeat(200)}),
                Value::Null
            )))
            .is_err()
    );
    for calls in [
        json!([{"index":1,"id":"c","type":"function","function":{"name":"echo","arguments":"{}"}}]),
        json!([{"index":0,"id":"c","type":"function","function":{"name":"missing","arguments":"{}"}}]),
        json!([{"index":0,"id":"c","type":"function","function":{"name":"echo","arguments":"{}"}},{"index":1,"id":"c","type":"function","function":{"name":"echo","arguments":"{}"}}]),
    ] {
        let mut stream = prepared.stream(1024 * 1024).unwrap();
        stream
            .event(stream_event(&chunk(
                json!({"tool_calls":calls}),
                Value::Null,
            )))
            .unwrap();
        assert!(
            stream
                .event(stream_event(&chunk(json!({}), json!("tool_calls"))))
                .is_err()
        );
        assert!(stream.finish().is_err());
    }
}

#[test]
fn chat_stream_accepts_large_accumulated_text_without_oversized_ir_deltas() {
    let prepared = custom_prepared();
    let part = "한".repeat(220000);
    let mut stream = prepared.stream(2 * 1024 * 1024).unwrap();
    for _ in 0..2 {
        stream
            .event(stream_event(&chunk(json!({"content":part}), Value::Null)))
            .unwrap();
    }
    stream
        .event(stream_event(&chunk(json!({}), json!("stop"))))
        .unwrap();
    let events = stream.event(done_event()).unwrap();
    assert_eq!(
        events[0]["response"]["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .len(),
        part.len() * 2
    );
    stream.finish().unwrap();
}

#[test]
fn chat_stream_bounds_total_argument_fragments_before_tool_completion() {
    let prepared = custom_prepared();
    let mut stream = prepared.stream(12 * 1024 * 1024).unwrap();
    stream.event(stream_event(&chunk(json!({"tool_calls":[{"index":0,"id":"c","type":"function","function":{"name":"echo","arguments":"{\"text\":\""}}]}),Value::Null))).unwrap();
    let delta = chunk(
        json!({"tool_calls":[{"index":0,"function":{"arguments":"a".repeat(1024*1024)}}]}),
        Value::Null,
    );
    for _ in 0..7 {
        assert!(stream.event(stream_event(&delta)).unwrap().is_empty());
    }
    assert!(stream.event(stream_event(&delta)).is_err());
    assert!(stream.finish().is_err());
}
