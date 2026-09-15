use super::{conversion::*, engine, verification::Progress};
use serde_json::{Value, json};
use std::io::{Cursor, Read};

fn frame(sequence: u64, operation: Value) -> Vec<u8> {
    let value = json!({"protocol":PROTOCOL,"sequence":sequence,"operation":operation});
    let raw = serde_json::to_vec(&value).unwrap();
    [&(raw.len() as u32).to_be_bytes()[..], &raw].concat()
}
fn prepare() -> Value {
    json!({"operation":"prepare","value":{"request":{"model":"synthetic","input":"test","stream":false},"route":{"api":"responses","model":"synthetic","profile_id":"fixture","profile_version":"1","reasoning_contract":null,"support":{},"context_window":8192,"max_output_tokens":2048},"managed":false,"pending_tools":false,"history":[],"max_output_bytes":65536}})
}

#[test]
fn legacy_codec_rejects_new_editing_fields_including_null() {
    for editing in [Value::Null, json!({"version":1})] {
        let mut operation = prepare();
        operation["value"]["editing"] = editing;
        assert!(engine::serve(&mut Cursor::new(frame(1, operation)), &mut vec![]).is_err());
    }
}

#[test]
fn editing_codec_requires_its_version_and_valid_policy() {
    assert!(engine::serve_editing(&mut Cursor::new(frame(1, prepare())), &mut vec![]).is_err());
    let mut operation = prepare();
    operation["value"]["editing"] = json!({"version":2,"client_contract":"codex-direct-custom/v1","representation":"context-lines/v1","patch_dialect":"codex-patch/1","normalization":"none"});
    let raw = serde_json::to_vec(
        &json!({"protocol":EDITING_PROTOCOL,"sequence":1,"operation":operation}),
    )
    .unwrap();
    let framed = [&(raw.len() as u32).to_be_bytes()[..], &raw].concat();
    assert!(engine::serve_editing(&mut Cursor::new(framed), &mut vec![]).is_err());
}
fn replies(raw: Vec<u8>) -> Vec<Value> {
    let mut cursor = Cursor::new(raw);
    let mut values = vec![];
    while (cursor.position() as usize) < cursor.get_ref().len() {
        let mut n = [0; 4];
        cursor.read_exact(&mut n).unwrap();
        let mut raw = vec![0; u32::from_be_bytes(n) as usize];
        cursor.read_exact(&mut raw).unwrap();
        values.push(serde_json::from_slice(&raw).unwrap());
    }
    values
}
#[test]
fn reference_full_json_preserves_semantic_output_without_host_credentials() {
    let body = json!({"id":"resp_fixture","object":"response","created_at":1,"model":"synthetic","status":"completed","output":[{"id":"message_fixture","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"result","annotations":[]}]}],"usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}});
    let mut input = frame(1, prepare());
    input.extend(frame(
        2,
        json!({"operation":"json","body":body.to_string(),"response_id":""}),
    ));
    let mut output = vec![];
    engine::serve(&mut Cursor::new(input), &mut output).unwrap();
    let values = replies(output);
    assert_eq!(values.len(), 3);
    assert_eq!(values[0]["value"]["result"], "ready");
    assert_eq!(values[1]["value"]["payload"]["model"], "synthetic");
    assert_eq!(values[2]["value"]["response"], body);
}
#[test]
fn version_sequence_unknown_operations_and_duplicate_keys_fail() {
    for input in [
        frame(2, prepare()),
        frame(1, json!({"operation":"get_credentials"})),
        {
            let raw=br#"{"protocol":"gateway-api-codec/v1","sequence":1,"sequence":1,"operation":"finish"}"#;
            [&(raw.len() as u32).to_be_bytes()[..], raw].concat()
        },
        ((MAX_FRAME + 1) as u32).to_be_bytes().to_vec(),
    ] {
        assert!(engine::serve(&mut Cursor::new(input), &mut vec![]).is_err());
    }
    let mut value = prepare();
    value["value"]["route"]["credential"] = json!("forbidden");
    assert!(engine::serve(&mut Cursor::new(frame(1, value)), &mut vec![]).is_err());
}
#[test]
fn replay_dto_rejects_overlapping_out_of_range_and_unmanaged_spans() {
    let mut value = prepare()["value"].clone();
    value["request"]["input"] = json!([{"type":"message","role":"user","content":"test"}]);
    value["history"] =
        json!([{"start":0,"end":1,"native":{"format":"gemini_steps","version":1,"steps":[{}]}}]);
    let parsed: Prepare = serde_json::from_value(value.clone()).unwrap();
    assert!(parsed.history().is_err());
    value["managed"] = json!(true);
    value["history"][0]["end"] = json!(2);
    let parsed: Prepare = serde_json::from_value(value.clone()).unwrap();
    assert!(parsed.history().is_err());
    value["history"][0]["end"] = json!(1);
    let span = value["history"][0].clone();
    value["history"].as_array_mut().unwrap().push(span);
    let parsed: Prepare = serde_json::from_value(value).unwrap();
    assert!(parsed.history().is_err());
}
#[test]
fn independent_progress_gate_rejects_tool_publication_and_changed_final_text() {
    let events = vec![
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"m","type":"message","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.content_part.added","output_index":0,"item_id":"m","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"m","content_index":0,"delta":"shown"}),
    ];
    let mut gate = Progress::default();
    gate.observe(&events, 4096).unwrap();
    assert!(
        gate.finish(
            &json!({"output":[{"id":"m","type":"message","content":[{"text":"changed"}]}]})
        )
        .is_err()
    );
    gate.finish(&json!({"output":[{"id":"m","type":"message","content":[{"text":"shown"}]}]}))
        .unwrap();
    assert!(Progress::default().observe(&[json!({"type":"response.output_item.added","output_index":0,"item":{"id":"t","type":"function_call"}})],4096).is_err());
    assert!(Progress::default().observe(&events, 1).is_err());
}

#[test]
fn core_output_verifier_rejects_unregistered_tools_duplicate_arguments_and_opaque_output() {
    use crate::ir::{capability::plan_translation, continuity::ContinuityBinding, responses};
    let request=responses::decode(json!({"model":"synthetic","input":"test","tools":[{"type":"function","name":"approved","parameters":{"type":"object"}}]}),None).unwrap();
    let mut route: Route = serde_json::from_value(prepare()["value"]["route"].clone()).unwrap();
    route.support.insert(
        crate::ir::capability::Feature::FunctionTools,
        "native".into(),
    );
    let plan = plan_translation(
        &request,
        &ContinuityBinding {
            route: route.snapshot().unwrap(),
            scope: "test".into(),
        },
    )
    .unwrap();
    let verifier = crate::adapters::responses::output_verifier(&request, &plan).unwrap();
    verifier.verify_progress(&[json!({"type":"response.created","response":{"id":"r","object":"response","status":"in_progress","output":[]}})]).unwrap();
    assert!(verifier.verify_progress(&[json!({"type":"response.created","response":{"id":"r","object":"response","status":"in_progress","output":[],"opaque":"forbidden"}})]).is_err());
    let valid = json!({"id":"response","object":"response","created_at":1,"model":"synthetic","status":"completed","output":[{"id":"tool","type":"function_call","call_id":"call","name":"approved","arguments":"{}","status":"completed"}]});
    verifier.decode_bytes(valid.to_string().as_bytes()).unwrap();
    for (field, value) in [
        ("name", json!("unregistered")),
        ("arguments", json!("{\"x\":1,\"x\":2}")),
        ("status", json!("in_progress")),
    ] {
        let mut changed = valid.clone();
        changed["output"][0][field] = value;
        assert!(
            verifier
                .decode_bytes(changed.to_string().as_bytes())
                .is_err()
        );
    }
    let mut changed = valid.clone();
    changed["output"]
        .as_array_mut()
        .unwrap()
        .push(valid["output"][0].clone());
    assert!(
        verifier
            .decode_bytes(changed.to_string().as_bytes())
            .is_err()
    );
    let mut changed = valid;
    changed["output"] = json!([{"id":"reasoning","type":"reasoning","summary":[],"encrypted_content":"untrusted-opaque"}]);
    assert!(
        verifier
            .decode_bytes(changed.to_string().as_bytes())
            .is_err()
    );
}

#[test]
fn portable_wire_boundary_preserves_simple_request_and_rejects_nested_core_contract_drift() {
    let value = json!({"protocol": PROTOCOL, "sequence": 1, "operation": prepare()});
    let typed: Request = serde_json::from_value(value.clone()).unwrap();
    let wire: gateway_plugin_contract::Request = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        encode_request(&typed).unwrap(),
        serde_json::to_vec(&wire).unwrap()
    );
    assert_eq!(
        encode_request(&typed).unwrap(),
        serde_json::to_vec(&typed).unwrap()
    );
    assert!(decode_request(value.clone()).is_ok());
    for (field, replacement) in [
        ("api", json!("unknown_api")),
        ("support", json!({"unknown_feature":"native"})),
        ("reasoning_contract", json!({"unknown_field":true})),
    ] {
        let mut changed = value.clone();
        changed["operation"]["value"]["route"][field] = replacement;
        assert!(decode_request(changed).is_err());
    }
    let reply = json!({"protocol":PROTOCOL,"sequence":2,"value":{
        "result":"managed","value":{"response":{},
        "native":{"format":"unrecognized_state","version":1},
        "outcome":"completed","accounting":{}}}});
    assert!(serde_json::from_value::<gateway_plugin_contract::Reply>(reply.clone()).is_ok());
    assert!(decode_reply(reply).is_err());
}

#[test]
fn portable_wire_boundary_keeps_optional_absence_and_large_unsigned_limits() {
    let mut value = json!({"protocol":PROTOCOL,"sequence":u64::MAX,"operation":prepare()});
    value["operation"]["value"]["route"]["context_window"] = json!(u64::MAX);
    let typed = decode_request(value).unwrap();
    let wire: Value = serde_json::from_slice(&encode_request(&typed).unwrap()).unwrap();
    assert_eq!(wire["sequence"], json!(u64::MAX));
    assert_eq!(
        wire["operation"]["value"]["route"]["context_window"],
        json!(u64::MAX)
    );
    assert!(wire["operation"]["value"].get("editing").is_none());
    assert_eq!(
        wire["operation"]["value"]["route"]["reasoning_contract"],
        Value::Null
    );
}

#[test]
fn portable_managed_reply_keeps_semantics_without_widening_core_validation() {
    let reply = Reply {
        protocol: EDITING_PROTOCOL.into(),
        sequence: 4,
        value: ResultValue::Managed {
            value: Box::new(ManagedResult {
                response: json!({"output":[]}),
                native: crate::ir::continuity::NativeReplay::Gemini {
                    version: 1,
                    steps: vec![json!({"synthetic":true})],
                },
                outcome: crate::ir::continuity::Outcome::Completed,
                accounting: crate::adapters::managed::Accounting::new(
                    gateway_usage_contract::Profile::ResponsesV1,
                    Some(
                        &json!({"input_tokens":9007199254740993_u64,"output_tokens":1,"total_tokens":9007199254740994_u64}),
                    ),
                    &json!({"model":"synthetic","id":"r"}),
                    gateway_usage_contract::Outcome::Completed,
                ),
            }),
        },
    };
    let expected = serde_json::to_value(&reply).unwrap();
    let actual: Value = serde_json::from_slice(&encode_reply(&reply).unwrap()).unwrap();
    assert_eq!(actual, expected);
    let decoded = decode_reply(actual.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    let mut changed = actual;
    changed["value"]["value"]["accounting"]["host_secret"] = json!(true);
    assert!(decode_reply(changed).is_err());
}

#[test]
fn generic_provider_api_cannot_enter_legacy_codec_wire() {
    for protocol in [
        PROTOCOL,
        EDITING_PROTOCOL,
        gateway_plugin_contract::CAPABILITIES_PROTOCOL,
    ] {
        let mut operation = prepare();
        operation["value"]["route"]["api"] = json!("plugin");
        let value = json!({"protocol":protocol,"sequence":1,"operation":operation});
        assert!(decode_request(value.clone()).is_err());
        // Core deserialization can represent Plugin, but the old wire cannot.
        let internal: Request = serde_json::from_value(value).unwrap();
        assert!(encode_request(&internal).is_err());
        let value = json!({"protocol":protocol,"sequence":0,"value":{"result":"ready","apis":["plugin"],"replay_versions":[1]}});
        assert!(decode_reply(value.clone()).is_err());
        let internal: Reply = serde_json::from_value(value).unwrap();
        assert!(encode_reply(&internal).is_err());
    }
}
