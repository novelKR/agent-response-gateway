use super::{contract::*, engine, verification::Progress};
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
