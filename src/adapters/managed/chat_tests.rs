use super::*;
use crate::{
    config::{DeclaredSupport, UpstreamAuth},
    ir::{ApiProtocol, capability::Feature, reasoning::ReasoningContract},
};
use serde_json::json;
fn config(router: bool) -> crate::Config {
    let mut c = super::tests::config(false);
    let m = c.models.get_mut("test").unwrap();
    m.api = ApiProtocol::ChatCompletions;
    m.auth = Some(UpstreamAuth::Bearer);
    m.messages_version = None;
    let p = c.capability_profiles.get_mut("test").unwrap();
    p.api = ApiProtocol::ChatCompletions;
    p.reasoning_contract = Some(if router {
        ReasoningContract::OpenRouter {
            version: 1,
            provider_endpoint: "synthetic/exact".into(),
            efforts: ["low".into(), "high".into()].into(),
            default_effort: Some("high".into()),
            max_tokens: None,
            formats: ["anthropic-claude-v1".into()].into(),
        }
    } else {
        ReasoningContract::DeepSeek {
            version: 1,
            efforts: ["low".into(), "high".into(), "max".into()].into(),
            default_effort: "high".into(),
        }
    });
    p.support
        .insert(Feature::Instructions, DeclaredSupport::Native);
    p.support.insert(
        Feature::InstructionHierarchy,
        if router {
            DeclaredSupport::Native
        } else {
            DeclaredSupport::BridgedChatInstructionEnvelope
        },
    );
    p.support.insert(
        Feature::ParallelToolControl,
        if router {
            DeclaredSupport::Native
        } else {
            DeclaredSupport::BridgedParallelPermission
        },
    );
    c.validate().unwrap();
    c
}
fn request() -> Value {
    let mut r = super::tests::request();
    r["reasoning"]["effort"] = json!("high");
    r["parallel_tool_calls"] = json!(true);
    r
}
fn assistant(router: bool) -> Value {
    let mut a = json!({"role":"assistant","content":"answer","tool_calls":[{"id":"call_one","type":"function","function":{"name":"echo","arguments":"{\"text\":\"synthetic\"}"}}]});
    if router {
        a["reasoning"] = json!("mirror is not shown");
        a["reasoning_details"] = json!([
            {"type":"reasoning.text","id":"detail_one","index":0,"format":"anthropic-claude-v1","text":"public reasoning","signature":"private-signature"},
            {"type":"reasoning.summary","id":null,"index":1,"format":"anthropic-claude-v1","summary":"public summary"},
            {"type":"reasoning.encrypted","id":"encrypted","index":2,"format":"anthropic-claude-v1","data":"private-encrypted"}]);
    } else {
        a["reasoning_content"] = json!("public reasoning");
    }
    a
}
fn response(a: Value) -> Value {
    json!({"id":"chat_synthetic","created":1,"model":"synthetic-model","object":"chat.completion","choices":[{"index":0,"message":a,"finish_reason":"tool_calls"}],
    "usage":{"prompt_tokens":32,"completion_tokens":12,"total_tokens":44,"prompt_cache_hit_tokens":8,"prompt_cache_miss_tokens":24,"prompt_tokens_details":{"cached_tokens":8},"completion_tokens_details":{"reasoning_tokens":5}}})
}
fn chunk(delta: Value, finish: Value) -> Value {
    json!({"object":"chat.completion.chunk","choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}
fn events(raw: &Value) -> Vec<Value> {
    let a = &raw["choices"][0]["message"];
    let mut first = chunk(json!({"role":"assistant","content":null}), Value::Null);
    first["id"] = raw["id"].clone();
    first["created"] = json!(1);
    let mut out = vec![first];
    let mut identity = chunk(json!({}), Value::Null);
    identity["model"] = json!("synthetic-model");
    out.push(identity);
    for key in ["reasoning_content", "reasoning"] {
        if let Some(value) = a.get(key) {
            for c in value.as_str().unwrap().chars() {
                let mut d = json!({});
                d[key] = json!(c.to_string());
                out.push(chunk(d, Value::Null));
            }
        }
    }
    if let Some(details) = a.get("reasoning_details") {
        for detail in details.as_array().unwrap() {
            let mut initial = detail.clone();
            let field = match detail["type"].as_str().unwrap() {
                "reasoning.text" => "text",
                "reasoning.summary" => "summary",
                _ => "data",
            };
            initial.as_object_mut().unwrap().remove(field);
            initial.as_object_mut().unwrap().remove("format");
            let id = initial["id"].clone();
            initial["id"] = Value::Null;
            if detail.get("signature").is_some() {
                initial["signature"] = Value::Null;
            }
            out.push(chunk(json!({"reasoning_details":[initial]}), Value::Null));
            for c in detail[field].as_str().unwrap().chars() {
                let mut d = json!({"index":detail["index"]});
                d[field] = json!(c.to_string());
                out.push(chunk(json!({"reasoning_details":[d]}), Value::Null));
            }
            out.push(chunk(json!({"reasoning_details":[{"index":detail["index"],"id":id,"format":detail["format"]}]}),Value::Null));
            if let Some(signature) = detail.get("signature") {
                for c in signature.as_str().unwrap().chars() {
                    out.push(chunk(json!({"reasoning_details":[{"index":detail["index"],"signature":c.to_string()}]}),Value::Null));
                }
            }
        }
    }
    for c in a["content"].as_str().unwrap().chars() {
        out.push(chunk(json!({"content":c.to_string()}), Value::Null));
    }
    out.push(chunk(json!({"tool_calls":[{"index":0,"id":null,"type":"function","function":{"name":"ec","arguments":""}}]}),Value::Null));
    out.push(chunk(
        json!({"tool_calls":[{"index":0,"id":"call_one","function":{"name":"ho"}}]}),
        Value::Null,
    ));
    for c in a["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap()
        .chars()
    {
        out.push(chunk(
            json!({"tool_calls":[{"index":0,"function":{"arguments":c.to_string()}}]}),
            Value::Null,
        ));
    }
    let mut final_chunk = chunk(json!({}), json!("tool_calls"));
    final_chunk["usage"] = json!({"prompt_tokens":32});
    out.push(final_chunk);
    let mut usage = raw["usage"].clone();
    usage.as_object_mut().unwrap().remove("prompt_tokens");
    out.push(json!({"object":"chat.completion.chunk","choices":[],"usage":usage}));
    out
}
fn push(s: &mut ManagedStream<'_>, v: &Value) -> Result<(), IrError> {
    s.event(SseEvent {
        event: "message".into(),
        data: v.to_string(),
    })
}
fn done(s: &mut ManagedStream<'_>) -> Result<(), IrError> {
    s.event(SseEvent {
        event: "message".into(),
        data: "[DONE]".into(),
    })
}

#[test]
fn dialects_select_explicit_controls_and_endpoint_without_fallback() {
    for router in [false, true] {
        let c = config(router);
        let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
        assert!(p.payload().get("store").is_none());
        assert!(p.payload().get("max_completion_tokens").is_none());
        assert_eq!(p.payload()["max_tokens"], 8192);
        if router {
            assert_eq!(
                p.payload()["provider"],
                json!({"only":["synthetic/exact"],"allow_fallbacks":false,"require_parameters":true})
            );
            assert!(p.payload().get("reasoning_effort").is_none());
        } else {
            assert_eq!(p.payload()["thinking"], json!({"type":"enabled"}));
            assert_eq!(p.payload()["reasoning_effort"], "high");
            assert!(p.payload().get("parallel_tool_calls").is_none());
        }
        for effort in ["medium", "ultra", "none"] {
            let mut r = request();
            r["reasoning"]["effort"] = json!(effort);
            assert!(super::tests::prepare(&c, r, &VerifiedProviderHistory::default()).is_err());
        }
        let mut c = c.clone();
        c.models.get_mut("test").unwrap().continuation_mode = None;
        assert!(c.validate().is_err());
    }
    let c = config(false);
    for change in [
        json!({"parallel_tool_calls":false}),
        json!({"tool_choice":"required"}),
        json!({"temperature":0.5}),
        json!({"top_p":0.9}),
    ] {
        let mut r = request();
        r.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(super::tests::prepare(&c, r, &VerifiedProviderHistory::default()).is_err());
    }
    let mut c = config(true);
    let p = c.capability_profiles.get_mut("test").unwrap();
    if let Some(ReasoningContract::OpenRouter { max_tokens, .. }) = &mut p.reasoning_contract {
        *max_tokens = Some(2048);
    }
    assert!(c.validate().is_err());
}

#[test]
fn native_chat_json_and_partial_identity_stream_have_equivalent_reasoning_and_tools() {
    for router in [false, true] {
        let c = config(router);
        let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
        let raw = response(assistant(router));
        let json = p
            .decode_bytes(raw.to_string().as_bytes(), "attempt")
            .unwrap();
        let mut stream = p.stream(200000, "attempt".into());
        let mut progress = vec![];
        for event in events(&raw) {
            push(&mut stream, &event).unwrap();
            progress.extend(stream.take_progress());
        }
        done(&mut stream).unwrap();
        progress.extend(stream.take_progress());
        let decoded = stream.finish().unwrap();
        assert!(decoded.native == json.native);
        assert_eq!(decoded.response, json.response);
        assert!(!json!(progress).to_string().contains("private-"));
        assert!(!json!(progress).to_string().contains("mirror is not shown"));
        assert!(
            !progress
                .iter()
                .any(|e| e["type"] == "response.completed"
                    || e["type"] == "response.output_item.done")
        );
        assert_eq!(json.response["usage"]["output_tokens"], 12);
        assert_eq!(
            json.response["usage"]["output_tokens_details"]["reasoning_tokens"],
            5
        );
        assert_eq!(
            json.response["output"][0]["summary"]
                .as_array()
                .unwrap()
                .len(),
            if router { 2 } else { 1 }
        );
    }
}

#[test]
fn chat_rejects_cross_dialect_fields_versions_collisions_and_truncation() {
    for router in [false, true] {
        let c = config(router);
        let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
        let mut raw = response(assistant(router));
        raw["choices"][0]["message"][if router {
            "reasoning_content"
        } else {
            "reasoning_details"
        }] = json!("foreign");
        assert!(
            p.decode_bytes(raw.to_string().as_bytes(), "attempt")
                .is_err()
        );
        let raw = response(assistant(router));
        let mut s = p.stream(200000, "attempt".into());
        for e in events(&raw) {
            push(&mut s, &e).unwrap();
        }
        assert!(s.finish().is_err());
        let mut s = p.stream(200000, "attempt".into());
        push(&mut s, &events(&raw)[0]).unwrap();
        let mut conflict = events(&raw)[1].clone();
        conflict["id"] = json!("other");
        assert!(push(&mut s, &conflict).is_err());
    }
    let c = config(true);
    let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
    for change in [
        json!({"format":"future-v99"}),
        json!({"type":"reasoning.context"}),
        json!({"index":3}),
    ] {
        let mut raw = response(assistant(true));
        raw["choices"][0]["message"]["reasoning_details"][0]
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(
            p.decode_bytes(raw.to_string().as_bytes(), "attempt")
                .is_err()
        );
    }
    let mut raw = response(assistant(true));
    raw["choices"][0]["message"]["reasoning_details"][1]["id"] = json!("detail_one");
    assert!(
        p.decode_bytes(raw.to_string().as_bytes(), "attempt")
            .is_err()
    );
}

#[test]
fn chat_native_assistant_replay_keeps_all_reasoning_when_tools_are_declared() {
    for router in [false, true] {
        let c = config(router);
        let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
        let raw = response(assistant(router));
        let d = p
            .decode_bytes(raw.to_string().as_bytes(), "attempt")
            .unwrap();
        let output = d.response["output"].as_array().unwrap();
        let mut history = VerifiedProviderHistory::default();
        history.segments.insert(1, (1 + output.len(), d.native));
        let mut items =
            vec![json!({"type":"message","role":"user","content":"synthetic question"})];
        items.extend(output.clone());
        items.push(json!({"type":"function_call_output","call_id":"call_one","output":"done"}));
        let mut r = request();
        r["input"] = json!(items);
        let next = super::tests::prepare(&c, r.clone(), &history).unwrap();
        next.validate_pending_controls(&history).unwrap();
        assert_eq!(next.payload()["messages"][1], raw["choices"][0]["message"]);
        r["reasoning"]["effort"] = json!("low");
        let changed = super::tests::prepare(&c, r, &history).unwrap();
        assert!(changed.validate_pending_controls(&history).is_err());
    }
}

#[test]
fn missing_usage_stays_unknown_and_stream_usage_cannot_regress_or_change_shape() {
    let c = config(true);
    let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
    let mut raw = response(assistant(true));
    raw["usage"] =
        json!({"completion_tokens":12,"completion_tokens_details":{"reasoning_tokens":null}});
    let mut d = p
        .decode_bytes(raw.to_string().as_bytes(), "attempt")
        .unwrap();
    assert!(d.usage["input_tokens"].is_null());
    assert_eq!(d.usage["output_tokens"], 12);
    d.project_usage();
    assert!(d.response["usage"].is_null());
    assert_eq!(d.usage["output_tokens"], 12);
    for invalid in [
        json!({"prompt_tokens":4}),
        json!({"prompt_tokens_details":42}),
    ] {
        let mut s = p.stream(200000, "attempt".into());
        let mut start = events(&response(assistant(true)))[0].clone();
        start["usage"] = json!({"prompt_tokens":8});
        push(&mut s, &start).unwrap();
        assert!(push(&mut s, &json!({"choices":[],"usage":invalid})).is_err());
    }
}

#[test]
fn router_reasoning_only_encrypted_only_and_deepseek_no_tool_compaction_are_explicit() {
    let c = config(true);
    let p = super::tests::prepare(&c, request(), &VerifiedProviderHistory::default()).unwrap();
    for details in [
        json!([{"type":"reasoning.text","text":"only reasoning","format":"anthropic-claude-v1","index":0}]),
        json!([{"type":"reasoning.encrypted","data":"opaque","format":"anthropic-claude-v1","index":0}]),
    ] {
        let mut raw =
            response(json!({"role":"assistant","content":null,"reasoning_details":details}));
        raw["choices"][0]["finish_reason"] = json!("stop");
        let d = p
            .decode_bytes(raw.to_string().as_bytes(), "attempt")
            .unwrap();
        assert_eq!(d.response["output"].as_array().unwrap().len(), 1);
        assert!(!d.response.to_string().contains("opaque"));
    }
    let c = config(false);
    let mut r = request();
    r.as_object_mut().unwrap().remove("tools");
    r["parallel_tool_calls"] = json!(false);
    let p = super::tests::prepare(&c, r, &VerifiedProviderHistory::default()).unwrap();
    assert!(p.payload().get("parallel_tool_calls").is_none());
}

#[test]
fn earlier_assistant_reasoning_without_tools_is_preserved_before_a_later_tool_turn() {
    for router in [false, true] {
        let c = config(router);
        let mut history = VerifiedProviderHistory::default();
        let p = super::tests::prepare(&c, request(), &history).unwrap();
        let mut first = response(assistant(router));
        first["choices"][0]["message"]
            .as_object_mut()
            .unwrap()
            .remove("tool_calls");
        first["choices"][0]["message"]["content"] = json!("earlier answer");
        first["choices"][0]["finish_reason"] = json!("stop");
        let d = p
            .decode_bytes(first.to_string().as_bytes(), "first")
            .unwrap();
        let out = d.response["output"].as_array().unwrap().clone();
        history.segments.insert(1, (1 + out.len(), d.native));
        let mut input = vec![json!({"type":"message","role":"user","content":"first"})];
        input.extend(out);
        input.push(json!({"type":"message","role":"user","content":"now use the tool"}));
        let mut r = request();
        r["input"] = json!(input);
        let p = super::tests::prepare(&c, r.clone(), &history).unwrap();
        let mut second = response(assistant(router));
        second["id"] = json!("second");
        let d = p
            .decode_bytes(second.to_string().as_bytes(), "second")
            .unwrap();
        let out = d.response["output"].as_array().unwrap().clone();
        let start = input.len();
        history
            .segments
            .insert(start, (start + out.len(), d.native));
        input.extend(out);
        input.push(json!({"type":"function_call_output","call_id":"call_one","output":"done"}));
        r["input"] = json!(input);
        let p = super::tests::prepare(&c, r, &history).unwrap();
        p.validate_pending_controls(&history).unwrap();
        assert_eq!(p.payload()["messages"][1], first["choices"][0]["message"]);
        assert_eq!(p.payload()["messages"][3], second["choices"][0]["message"]);
    }
}
