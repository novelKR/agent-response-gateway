use super::*;
use crate::{Config, ir::continuity::NativeReplay, routing::AdmittedRequest};
use serde_json::json;

fn config(manual: bool) -> Config {
    let directory = serde_json::to_string(&std::env::temp_dir()).unwrap();
    let contract = if manual {
        "kind='claude_manual'\nversion=1\nbudget_tokens=2048\neffort_budgets={low=1024,medium=2048,high=4096}\ninterleaved_beta=true"
    } else {
        "kind='claude_adaptive'\nversion=1\nefforts=['low','medium','high']\ndefault_effort='medium'"
    };
    Config::parse(&format!(
        r#"
[providers.mock]
base_url='http://127.0.0.1:1234/v1'
api_key_env='MOCK_KEY'
[models.test]
provider='mock'
upstream_model='synthetic-model'
api='messages'
auth='api_key'
messages_version='2023-06-01'
capability_profile='test'
continuation_mode='managed'
[capability_profiles.test]
version='1'
provider='mock'
upstream_model='synthetic-model'
api='messages'
context_window=32768
max_output_tokens=8192
tested_codex_version='0.154.0'
[capability_profiles.test.reasoning_contract]
{contract}
[capability_profiles.test.support]
function_tools='native'
max_output_tokens='native'
reasoning_effort='native'
reasoning_summary='native'
reasoning_items='native'
tool_choice='native'
[continuation]
directory={directory}
store_id='synthetic-store'
realm='synthetic'
generation='1'
key_id='key'
key_env='PROTECTION'
control_token_env='CONTROL'
max_store_bytes=16777216
"#
    ))
    .unwrap()
}
fn request() -> Value {
    json!({"model":"test","input":"synthetic question","reasoning":{"effort":"medium","summary":"auto"},
        "tools":[{"type":"function","name":"echo","parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}}]})
}
fn prepare(
    c: &Config,
    value: Value,
    history: &VerifiedProviderHistory,
) -> Result<ManagedAdapter, IrError> {
    let route = c.resolve_route("test").unwrap();
    let AdmittedRequest::Translated { request, plan } =
        route.admit_verified(value.as_object().unwrap().clone(), history)?
    else {
        panic!("translated")
    };
    ManagedAdapter::encode(&request, &plan, history)
}
fn message(blocks: Value, stop: &str) -> Value {
    json!({"id":"synthetic","type":"message","role":"assistant","model":"synthetic-model","content":blocks,"stop_reason":stop,"stop_sequence":null,
        "usage":{"input_tokens":32,"cache_read_input_tokens":5,"output_tokens":12}})
}
fn blocks() -> Value {
    json!([
        {"type":"thinking","thinking":"public α reasoning","signature":"private-signature"},
        {"type":"redacted_thinking","data":"private-redacted"},
        {"type":"text","text":"using the tool"},
        {"type":"tool_use","id":"call_one","name":"echo","input":{"text":"synthetic"}},
        {"type":"thinking","thinking":"","signature":"private-second"}
    ])
}
fn events(value: &Value) -> Vec<Value> {
    let mut start = value.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    start["usage"]
        .as_object_mut()
        .unwrap()
        .remove("output_tokens");
    let mut events = vec![json!({"type":"message_start","message":start})];
    for (i, block) in value["content"].as_array().unwrap().iter().enumerate() {
        let mut first = block.clone();
        let fields: Vec<(&str, &str)> = match block["type"].as_str().unwrap() {
            "thinking" => vec![
                ("thinking", "thinking_delta"),
                ("signature", "signature_delta"),
            ],
            "text" => vec![("text", "text_delta")],
            "tool_use" => vec![("input", "input_json_delta")],
            _ => vec![],
        };
        for (field, _) in &fields {
            first[field] = if *field == "input" {
                json!({})
            } else {
                json!("")
            };
        }
        events.push(json!({"type":"content_block_start","index":i,"content_block":first}));
        for (field, kind) in fields {
            let text = if field == "input" {
                block[field].to_string()
            } else {
                block[field].as_str().unwrap().to_owned()
            };
            for ch in text.chars() {
                let key = if field == "input" {
                    "partial_json"
                } else {
                    field
                };
                let mut delta = json!({"type":kind});
                delta[key] = json!(ch.to_string());
                events.push(json!({"type":"content_block_delta","index":i,"delta":delta}));
            }
        }
        events.push(json!({"type":"content_block_stop","index":i}));
    }
    events.push(json!({"type":"message_delta","delta":{"stop_reason":value["stop_reason"],"stop_sequence":null},"usage":{"output_tokens":12}}));
    events.push(json!({"type":"message_stop"}));
    events
}
fn push(stream: &mut ManagedStream<'_>, v: &Value) -> Result<(), IrError> {
    stream.event(SseEvent {
        event: v["type"].as_str().unwrap().into(),
        data: v.to_string(),
    })
}

#[test]
fn explicit_claude_controls_do_not_weaken_budget_effort_or_summary() {
    for manual in [false, true] {
        let c = config(manual);
        let history = VerifiedProviderHistory::default();
        let p = prepare(&c, request(), &history).unwrap();
        assert_eq!(p.payload()["thinking"]["display"], "summarized");
        if manual {
            assert_eq!(p.payload()["thinking"]["budget_tokens"], 2048);
        } else {
            assert_eq!(p.payload()["output_config"]["effort"], "medium");
        }
        for summary in ["concise", "detailed"] {
            let mut v = request();
            v["reasoning"]["summary"] = json!(summary);
            assert!(prepare(&c, v, &history).is_err());
        }
        for effort in ["minimal", "ultra", "none"] {
            let mut v = request();
            v["reasoning"]["effort"] = json!(effort);
            assert!(prepare(&c, v, &history).is_err());
        }
        let mut v = request();
        v["tool_choice"] = json!("required");
        assert!(prepare(&c, v, &history).is_err());
        let mut v = request();
        v["max_output_tokens"] = json!(1024);
        if manual {
            assert!(prepare(&c, v, &history).is_err());
        }
        let mut invalid = c.clone();
        invalid.models.get_mut("test").unwrap().continuation_mode = None;
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn messages_native_json_and_split_stream_keep_order_signatures_and_public_reasoning() {
    let c = config(false);
    let p = prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
    let raw = message(blocks(), "tool_use");
    let json = p
        .decode_bytes(raw.to_string().as_bytes(), "attempt")
        .unwrap();
    let mut stream = p.stream(100000, "attempt".into());
    let mut progress = vec![];
    let frames = events(&raw)
        .into_iter()
        .map(|v| format!("event: {}\ndata: {}\n\n", v["type"].as_str().unwrap(), v))
        .collect::<String>();
    let mut decoder = crate::adapters::sse::SseDecoder::new(100000).unwrap();
    for byte in frames.bytes() {
        let chunk = [byte];
        let mut rest = chunk.as_slice();
        while let Some(event) = decoder.next_event(&mut rest).unwrap() {
            stream.event(event).unwrap();
            progress.extend(stream.take_progress());
        }
    }
    assert!(stream.is_complete());
    let sse = stream.finish().unwrap();
    assert!(sse.native == json.native);
    assert_eq!(sse.response["output"], json.response["output"]);
    assert_eq!(sse.response["usage"], json.response["usage"]);
    assert_eq!(json.response["usage"]["input_tokens"], 37);
    assert_eq!(json.response["usage"]["output_tokens"], 12);
    assert!(
        progress
            .iter()
            .any(|v| v["type"] == "response.reasoning_summary_text.delta")
    );
    assert!(!json!(progress).to_string().contains("private-"));
    assert!(
        !progress
            .iter()
            .any(|v| v["type"] == "response.output_item.done" || v["type"] == "response.completed")
    );
    assert_eq!(json.outcome, Outcome::AwaitingTools);
}

#[test]
fn signed_redacted_and_reasoning_only_responses_need_no_public_answer() {
    let c = config(true);
    let p = prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
    for blocks in [
        json!([{ "type":"thinking","thinking":"only reasoning","signature":"signed"}]),
        json!([{ "type":"thinking","thinking":"","signature":"signed"}]),
        json!([{ "type":"redacted_thinking","data":"private"}]),
    ] {
        let raw = message(blocks, "end_turn");
        let decoded = p
            .decode_bytes(raw.to_string().as_bytes(), "attempt")
            .unwrap();
        let mut stream = p.stream(100000, "attempt".into());
        for event in events(&raw) {
            push(&mut stream, &event).unwrap();
        }
        assert!(stream.finish().unwrap().native == decoded.native);
    }
    let mut raw = message(
        json!([{ "type":"thinking","thinking":"x","signature":"signed"}]),
        "end_turn",
    );
    raw.as_object_mut().unwrap().remove("usage");
    let d = p
        .decode_bytes(raw.to_string().as_bytes(), "attempt")
        .unwrap();
    assert!(d.response["usage"]["input_tokens"].is_null());
    assert!(d.response["usage"]["output_tokens"].is_null());
}

#[test]
fn messages_reject_signature_loss_unknown_blocks_and_malformed_streams() {
    let c = config(false);
    let p = prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
    let raw = message(blocks(), "tool_use");
    for replacement in [
        json!({"type":"thinking","thinking":"x"}),
        json!({"type":"thinking","thinking":"x","signature":""}),
        json!({"type":"server_tool_use","name":"search"}),
    ] {
        let mut invalid = raw.clone();
        invalid["content"][0] = replacement;
        assert!(
            p.decode_bytes(invalid.to_string().as_bytes(), "attempt")
                .is_err()
        );
    }
    let frames = events(&raw);
    let mut truncated = p.stream(100000, "attempt".into());
    for v in &frames[..frames.len() - 1] {
        push(&mut truncated, v).unwrap();
    }
    assert!(truncated.finish().is_err());
    let mut bad = p.stream(100000, "attempt".into());
    push(&mut bad, &frames[0]).unwrap();
    let mut start = frames[1].clone();
    start["index"] = json!(4);
    assert!(push(&mut bad, &start).is_err());
    let mut duplicate = raw.clone();
    let tool = duplicate["content"][3].clone();
    duplicate["content"].as_array_mut().unwrap().push(tool);
    assert!(
        p.decode_bytes(duplicate.to_string().as_bytes(), "attempt")
            .is_err()
    );
}

#[test]
fn authenticated_native_tool_turn_replays_original_blocks_and_rejects_control_change() {
    let c = config(false);
    let h = VerifiedProviderHistory::default();
    let p = prepare(&c, request(), &h).unwrap();
    let d = p
        .decode_bytes(
            message(blocks(), "tool_use").to_string().as_bytes(),
            "attempt",
        )
        .unwrap();
    let mut history = VerifiedProviderHistory::default();
    let output = d.response["output"].as_array().unwrap();
    history
        .segments
        .insert(1, (1 + output.len(), d.native.clone()));
    let mut items = vec![json!({"type":"message","role":"user","content":"synthetic question"})];
    items.extend(output.clone());
    items.push(json!({"type":"function_call_output","call_id":"call_one","output":"done"}));
    let mut next = request();
    next["input"] = json!(items);
    let continued = prepare(&c, next.clone(), &history).unwrap();
    continued.validate_pending_controls(&history).unwrap();
    let native = match d.native {
        NativeReplay::Messages { blocks, .. } => blocks,
        _ => panic!("messages"),
    };
    assert_eq!(continued.payload()["messages"][1]["content"], json!(native));
    next["reasoning"]["effort"] = json!("high");
    let changed = prepare(&c, next, &history).unwrap();
    assert!(changed.validate_pending_controls(&history).is_err());
}
