use std::collections::BTreeMap;

use agent_response_gateway::ir::{
    ApiProtocol, CallId, IrError, ItemId, ToolIdentity, ToolKind,
    bridge::CustomToolBridge,
    capability::{BridgeRule, CapabilityProfile, Feature, Support, plan_translation, requirements},
    continuity::{ContinuityBinding, OpaqueState, RouteSnapshot},
    request::*,
    responses,
};
use serde_json::{Value, json};

fn binding(api: ApiProtocol) -> ContinuityBinding {
    let features = [
        Feature::Instructions,
        Feature::InstructionHierarchy,
        Feature::Images,
        Feature::FunctionTools,
        Feature::StrictToolArguments,
        Feature::CustomTools,
        Feature::CustomGrammar,
        Feature::NamespacedTools,
        Feature::StructuredToolOutput,
        Feature::ToolChoice,
        Feature::ParallelToolControl,
        Feature::StructuredOutput,
        Feature::StrictStructuredOutput,
        Feature::MaxOutputTokens,
        Feature::Temperature,
        Feature::TopP,
        Feature::ReasoningEffort,
        Feature::ReasoningSummary,
        Feature::ReasoningItems,
        Feature::OpaqueContinuation,
    ];
    ContinuityBinding {
        route: RouteSnapshot {
            provider_id: "synthetic-provider".into(),
            model: "actual-model".into(),
            api,
            credential_binding: "synthetic-auth-reference".into(),
            adapter_version: "adapter/1".into(),
            capabilities: CapabilityProfile {
                id: "synthetic-native".into(),
                version: "1".into(),
                protocol: api,
                support: features.into_iter().map(|f| (f, Support::Native)).collect(),
            },
            context_window: Some(10000),
            max_output_tokens: Some(2048),
        },
        scope: "synthetic-principal".into(),
    }
}

fn full_request() -> Value {
    json!({
        "model":"writer", "instructions":"Top-level instruction", "stream":false,
        "input":[
            {"role":"system","content":"First instruction"},
            {"role":"user","content":[{"type":"input_text","text":"합성 입력"},{"type":"input_image","image_url":"data:image/png;base64,AA==","detail":"low"}]},
            {"type":"message","id":"msg_d","role":"developer","content":"Later scoped instruction"},
            {"type":"function_call","id":"item_f","call_id":"call_f","name":"lookup","namespace":"tools","arguments":"{ \"n\": 184467440737095516160 }"},
            {"type":"custom_tool_call","id":"item_c","call_id":"call_c","name":"patch","namespace":"files","input":"*** Begin Patch\n한글 \\\"\n*** End Patch"},
            {"type":"custom_tool_call_output","call_id":"call_c","output":"done"},
            {"type":"function_call_output","call_id":"call_f","output":"found"},
            {"type":"reasoning","id":"reasoning_1","summary":[{"type":"summary_text","text":"Summary"}],"encrypted_content":"opaque-synthetic-payload"}
        ],
        "tools":[
            {"type":"function","name":"lookup","namespace":"tools","description":"Lookup","parameters":{"type":"object","properties":{"n":{"type":"integer"}}},"strict":true},
            {"type":"custom","name":"patch","namespace":"files","format":{"type":"text"}}
        ],
        "tool_choice":{"type":"function","namespace":"tools","name":"lookup"},
        "parallel_tool_calls":false,"max_output_tokens":1024,"temperature":0.25,"top_p":0.9,
        "text":{"format":{"type":"json_schema","name":"answer","strict":true,"schema":{"type":"object","additionalProperties":false,"properties":{}}}},
        "reasoning":{"effort":"high","summary":"auto"}
    })
}

#[test]
fn responses_roundtrip_preserves_order_instructions_ids_and_complete_meaning() {
    let source = binding(ApiProtocol::Responses);
    let mut original = full_request();
    let request = responses::decode(original.clone(), Some(&source)).unwrap();
    let instructions = request.instructions();
    assert_eq!(instructions.len(), 3);
    assert_eq!(instructions[0].role, InstructionRole::ProtocolDefault);
    assert_eq!(instructions[1].position, InstructionPosition::Item(0));
    assert_eq!(instructions[2].role, InstructionRole::Developer);
    assert_eq!(instructions[2].position, InstructionPosition::Item(2));
    original["store"] = json!(false);
    assert_eq!(
        responses::encode(&request, Some(&source)).unwrap(),
        original
    );
    let required = requirements(&request).unwrap();
    for feature in [
        Feature::InstructionHierarchy,
        Feature::Images,
        Feature::FunctionTools,
        Feature::CustomTools,
        Feature::StrictToolArguments,
        Feature::StrictStructuredOutput,
        Feature::NamespacedTools,
        Feature::OpaqueContinuation,
        Feature::ParallelToolControl,
    ] {
        assert!(required.contains(feature));
    }
    assert!(!required.contains(Feature::Extensions));
    assert!(plan_translation(&request, &source).is_ok());
}

#[test]
fn native_extensions_nulls_and_number_precision_survive_roundtrip() {
    let raw = r#"{"model":"writer","instructions":null,"tools":null,"temperature":0.12345678901234567890123456789,"top_p":null,"metadata":{"n":184467440737095516160},"input":[{"role":"user","type":null,"id":null,"content":[{"type":"input_text","text":"test","image_url":"unknown-field-here"},{"type":"future_part","payload":123}]}],"text":{"format":{"type":"future_format","option":true},"verbosity":"low"}}"#;
    let mut original: Value = serde_json::from_str(raw).unwrap();
    let request = responses::decode(original.clone(), None).unwrap();
    original["store"] = json!(false);
    let encoded = responses::encode(&request, None).unwrap();
    assert_eq!(encoded, original);
    assert!(encoded.to_string().contains("184467440737095516160"));
    assert!(
        encoded
            .to_string()
            .contains("0.12345678901234567890123456789")
    );
    assert!(
        requirements(&request)
            .unwrap()
            .contains(Feature::Extensions)
    );
    assert!(plan_translation(&request, &binding(ApiProtocol::Responses)).is_ok());
    assert_eq!(
        plan_translation(&request, &binding(ApiProtocol::Messages)).err(),
        Some(IrError::UnsupportedExtension)
    );
}

#[test]
fn unknown_items_and_tool_fields_keep_their_source_protocol() {
    let mut original = json!({"model":"m","input":[{"type":"future_item","data":{"x":1}}],"tools":[{"type":"function","name":"f","parameters":null,"strict":null,"format":{"future":true}}]});
    let mut request = responses::decode(original.clone(), None).unwrap();
    original["store"] = json!(false);
    assert_eq!(responses::encode(&request, None).unwrap(), original);
    request.extensions.protocol = ApiProtocol::Messages;
    assert_eq!(
        responses::encode(&request, None).err(),
        Some(IrError::WrongProtocol)
    );
}

#[test]
fn typed_edits_are_encoded_and_extensions_cannot_shadow_requirements() {
    let mut request = responses::decode(json!({"model":"m","input":"hello"}), None).unwrap();
    request.model = "new-model".into();
    assert_eq!(
        responses::encode(&request, None).unwrap()["model"],
        "new-model"
    );
    request
        .extensions
        .fields
        .insert("tools".into(), json!([{"type":"custom","name":"hidden"}]));
    assert_eq!(
        requirements(&request).err(),
        Some(IrError::ExtensionConflict)
    );
    assert_eq!(
        responses::encode(&request, None).err(),
        Some(IrError::ExtensionConflict)
    );
}

#[test]
fn stateless_admission_rules_are_shared_with_http_policy() {
    for field in [
        json!({"store":true}),
        json!({"background":true}),
        json!({"previous_response_id":"r"}),
        json!({"conversation":"c"}),
        json!({"context_management":[]}),
        json!({"input":[{"id":"prior"}]}),
        json!({"input":[{"type":"compaction","encrypted_content":"x"}]}),
    ] {
        let mut value = json!({"model":"m"});
        value
            .as_object_mut()
            .unwrap()
            .extend(field.as_object().unwrap().clone());
        assert_eq!(
            responses::decode(value, None).err(),
            Some(IrError::UnsupportedFeature)
        );
    }
    assert!(responses::decode(json!({"model":"m","stream":null}), None).is_err());
}

#[test]
fn request_validates_tool_result_order_kind_and_unique_identities() {
    let call =
        json!({"type":"function_call","id":"item","call_id":"c","name":"f","arguments":"{}"});
    let result = json!({"type":"function_call_output","call_id":"c","output":"ok"});
    for items in [
        json!([result.clone(), call.clone()]),
        json!([call.clone(), call.clone()]),
        json!([call.clone(), result.clone(), result.clone()]),
        json!([call.clone(),{"type":"custom_tool_call_output","call_id":"c","output":"ok"}]),
    ] {
        assert!(responses::decode(json!({"model":"m","input":items}), None).is_err());
    }
    assert!(responses::decode(json!({"model":"m","input":[call,result]}), None).is_ok());
    assert_eq!(responses::decode(json!({"model":"m","input":[{"type":"function_call","call_id":"c","name":"f","arguments":"{"}]}),None).err(),Some(IrError::InvalidJsonArguments));
    assert_eq!(responses::decode(json!({"model":"m","tools":[{"type":"function","name":"f"},{"type":"custom","name":"f"}]}),None).err(),Some(IrError::DuplicateId));
}

#[test]
fn strict_output_and_each_generation_requirement_must_be_supported() {
    let request=responses::decode(json!({"model":"m","temperature":0.5,"text":{"format":{"type":"json_schema","name":"result","schema":{},"strict":true}}}),None).unwrap();
    let mut target = binding(ApiProtocol::ChatCompletions);
    target
        .route
        .capabilities
        .support
        .remove(&Feature::StrictStructuredOutput);
    assert_eq!(
        plan_translation(&request, &target).err(),
        Some(IrError::UnsupportedFeature)
    );
    target
        .route
        .capabilities
        .support
        .insert(Feature::StrictStructuredOutput, Support::Native);
    target
        .route
        .capabilities
        .support
        .remove(&Feature::Temperature);
    assert_eq!(
        plan_translation(&request, &target).err(),
        Some(IrError::UnsupportedFeature)
    );
    target
        .route
        .capabilities
        .support
        .insert(Feature::Temperature, Support::Native);
    assert!(plan_translation(&request, &target).is_ok());
}

#[test]
fn output_limits_and_capability_profile_identity_are_checked() {
    let request = responses::decode(json!({"model":"m","max_output_tokens":2049}), None).unwrap();
    let mut target = binding(ApiProtocol::Responses);
    assert_eq!(
        plan_translation(&request, &target).err(),
        Some(IrError::UnsupportedFeature)
    );
    target.route.max_output_tokens = Some(3000);
    assert!(plan_translation(&request, &target).is_ok());
    target.route.capabilities.protocol = ApiProtocol::Messages;
    assert_eq!(
        plan_translation(&request, &target).err(),
        Some(IrError::WrongProtocol)
    );
}

fn custom_request() -> RequestIR {
    responses::decode(json!({"model":"m","tools":[{"type":"function","name":"arg_custom_0"},{"type":"custom","name":"patch","format":{"type":"text"}}],"tool_choice":{"type":"custom","name":"patch"},"input":[{"type":"custom_tool_call","id":"item_c","call_id":"call_c","name":"patch","input":"original"},{"type":"custom_tool_call_output","call_id":"call_c","output":"ok"}]}),None).unwrap()
}

#[test]
fn custom_bridge_avoids_collisions_and_roundtrips_arbitrary_text_and_results() {
    let request = custom_request();
    let bridge = CustomToolBridge::new(request.tools.as_ref().unwrap()).unwrap();
    let Some(Input::Items(items)) = &request.input else {
        panic!()
    };
    let Item::ToolCall(original) = &items[0] else {
        panic!()
    };
    assert_eq!(bridge.alias(&original.tool).unwrap().name, "arg_custom_1");
    for text in [
        "",
        "한글\n다음 줄",
        "quotes: \" slash: \\ tab:\t",
        "*** Begin Patch\n*** End Patch",
        "emoji: 🧪",
    ] {
        let mut call = original.clone();
        call.input = ToolInput::Freeform(text.into());
        let lower = bridge.lower_call(&call).unwrap();
        let restored = bridge.restore_call(&lower).unwrap();
        assert!(restored == call);
        assert_eq!(lower.call_id, call.call_id);
        assert_eq!(lower.item_id, call.item_id);
    }
    let Item::ToolResult(result) = &items[1] else {
        panic!()
    };
    let lower = bridge.lower_result(result, original).unwrap();
    assert_eq!(lower.kind, ToolKind::Function);
    assert!(bridge.restore_result(&lower, original).unwrap() == *result);
    let choice = bridge
        .lower_choice(request.generation.tool_choice.as_ref().unwrap())
        .unwrap();
    assert!(matches!(
        choice,
        ToolChoice::Named {
            kind: ToolKind::Function,
            ..
        }
    ));
}

#[test]
fn custom_bridge_preserves_original_namespace_as_mapping_identity() {
    let mut request = custom_request();
    let tools = request.tools.as_mut().unwrap();
    tools[1].identity.namespace = Some("synthetic_namespace".into());
    let bridge = CustomToolBridge::new(tools).unwrap();
    let mut call = ToolCall {
        item_id: None,
        call_id: CallId::new("c").unwrap(),
        tool: tools[1].identity.clone(),
        input: ToolInput::Freeform("text".into()),
        extensions: Extensions::responses(),
    };
    assert!(
        bridge
            .restore_call(&bridge.lower_call(&call).unwrap())
            .unwrap()
            == call
    );
    call.tool.namespace = None;
    assert_eq!(
        bridge.lower_call(&call).err(),
        Some(IrError::InvalidToolMapping)
    );
}

#[test]
fn bridge_rejects_unknown_wrappers_grammar_and_wrong_call_mapping() {
    let request = custom_request();
    let mut tools = request.tools.clone().unwrap();
    let bridge = CustomToolBridge::new(&tools).unwrap();
    let Some(Input::Items(items)) = &request.input else {
        panic!()
    };
    let Item::ToolCall(call) = &items[0] else {
        panic!()
    };
    let mut lowered = bridge.lower_call(call).unwrap();
    for raw in [
        "{",
        "{}",
        "{\"input\":3}",
        "{\"input\":\"ok\",\"extra\":true}",
        "{\"input\":\"first\",\"input\":\"second\"}",
    ] {
        lowered.input = ToolInput::Json(raw.into());
        assert!(bridge.restore_call(&lowered).is_err());
    }
    lowered = bridge.lower_call(call).unwrap();
    lowered.tool.name = "unknown".into();
    assert_eq!(
        bridge.restore_call(&lowered).err(),
        Some(IrError::InvalidToolMapping)
    );
    let Item::ToolResult(result) = &items[1] else {
        panic!()
    };
    let mut wrong = result.clone();
    wrong.call_id = CallId::new("other").unwrap();
    assert_eq!(
        bridge.lower_result(&wrong, call).err(),
        Some(IrError::InvalidToolMapping)
    );
    tools[1].kind = ToolDefinitionKind::Custom {
        format: Some(json!({"type":"grammar","syntax":"lark","definition":"start: /.+/"})),
    };
    assert_eq!(
        CustomToolBridge::new(&tools).err(),
        Some(IrError::UnsupportedFeature)
    );
}

#[test]
fn bridged_capability_needs_function_support_and_cannot_downgrade_grammar() {
    let request = custom_request();
    let mut target = binding(ApiProtocol::Messages);
    target.route.capabilities.support.insert(
        Feature::CustomTools,
        Support::Bridged(BridgeRule::CustomToolJson),
    );
    assert_eq!(
        plan_translation(&request, &target).unwrap().bridges,
        vec![BridgeRule::CustomToolJson]
    );
    target
        .route
        .capabilities
        .support
        .remove(&Feature::FunctionTools);
    assert!(plan_translation(&request, &target).is_err());
    target
        .route
        .capabilities
        .support
        .insert(Feature::FunctionTools, Support::Native);
    let mut grammar = request.clone();
    grammar.tools.as_mut().unwrap()[1].kind = ToolDefinitionKind::Custom {
        format: Some(json!({"type":"grammar"})),
    };
    assert!(plan_translation(&grammar, &target).is_err());
    target.route.capabilities.support.insert(
        Feature::StrictStructuredOutput,
        Support::Bridged(BridgeRule::CustomToolJson),
    );
    assert!(target.validate().is_err());
}

#[test]
fn opaque_state_requires_explicit_binding_and_never_becomes_general_text() {
    let origin = binding(ApiProtocol::Responses);
    let wire = json!({"model":"m","input":[{"type":"reasoning","summary":[],"encrypted_content":"opaque-private-bytes"}]});
    assert_eq!(
        responses::decode(wire.clone(), None).err(),
        Some(IrError::UnboundOpaqueState)
    );
    let request = responses::decode(wire, Some(&origin)).unwrap();
    assert_eq!(
        responses::encode(&request, None).err(),
        Some(IrError::UnboundOpaqueState)
    );
    assert!(
        requirements(&request)
            .unwrap()
            .contains(Feature::OpaqueContinuation)
    );
    assert!(responses::encode(&request, Some(&origin)).is_ok());
    let err = responses::encode(&request, None).err().unwrap();
    assert!(!format!("{err:?} {err}").contains("opaque-private-bytes"));
}

#[test]
fn opaque_replay_rejects_every_origin_scope_and_profile_change() {
    let origin = binding(ApiProtocol::Responses);
    let state = OpaqueState::new(origin.clone(), "native/1", b"opaque".to_vec()).unwrap();
    assert_eq!(state.replay(&origin, "native/1").unwrap(), b"opaque");
    let mut changes = Vec::new();
    let mut target = origin.clone();
    target.route.provider_id = "other".into();
    changes.push(target);
    let mut target = origin.clone();
    target.route.model = "other".into();
    changes.push(target);
    let mut target = origin.clone();
    target.route.credential_binding = "other".into();
    changes.push(target);
    let mut target = origin.clone();
    target.route.adapter_version = "other".into();
    changes.push(target);
    let mut target = origin.clone();
    target.scope = "other".into();
    changes.push(target);
    let mut target = origin.clone();
    target.route.capabilities.version = "2".into();
    changes.push(target);
    let mut target = origin.clone();
    target.route.capabilities.support = BTreeMap::new();
    changes.push(target);
    changes.push(binding(ApiProtocol::Messages));
    for target in changes {
        assert_eq!(
            state.replay(&target, "native/1").err(),
            Some(IrError::ContinuityMismatch)
        );
    }
    assert_eq!(
        state.replay(&origin, "native/2").err(),
        Some(IrError::ContinuityMismatch)
    );
}

#[test]
fn typed_ids_and_ir_versions_are_checked() {
    assert!(ItemId::new("").is_err());
    assert!(CallId::new("line\nbreak").is_err());
    assert!(ToolIdentity::new(Some(String::new()), "name").is_err());
    let mut request = responses::decode(json!({"model":"m","input":"text"}), None).unwrap();
    request.version = 2;
    assert_eq!(
        responses::encode(&request, None).err(),
        Some(IrError::UnsupportedVersion)
    );
}

#[test]
fn namespace_groups_roundtrip_order_description_and_effective_identity() {
    let wire = json!({"model":"m","store":false,"tools":[{"type":"namespace","name":"group","description":"group description","tools":[
        {"type":"function","name":"echo","parameters":{"type":"object"}},
        {"type":"custom","name":"patch","format":{"type":"text"}}]},
        {"type":"function","name":"echo"}],"tool_choice":{"type":"function","namespace":"group","name":"echo"}});
    let ir = responses::decode(wire.clone(), None).unwrap();
    assert_eq!(responses::encode(&ir, None).unwrap(), wire);
    let leaves = ir
        .tool_definitions()
        .map(|tool| tool.identity.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        leaves[0],
        ToolIdentity::new(Some("group".into()), "echo").unwrap()
    );
    assert_eq!(
        leaves[1],
        ToolIdentity::new(Some("group".into()), "patch").unwrap()
    );
    assert_eq!(leaves[2], ToolIdentity::new(None, "echo").unwrap());
    for tools in [
        json!([{"type":"namespace","name":"group","tools":[]}]),
        json!([{"type":"namespace","name":"group","tools":[{"type":"namespace","name":"nested","tools":[]}]}]),
        json!([{"type":"namespace","name":"group","tools":[{"type":"function","name":"echo","namespace":"other"}]}]),
        json!([{"type":"namespace","name":"group","tools":[{"type":"function","name":"echo"}]},{"type":"function","name":"echo","namespace":"group"}]),
    ] {
        assert!(responses::decode(json!({"model":"m","tools":tools}), None).is_err());
    }
}

#[test]
fn namespace_bridge_uses_one_bijective_registry_for_functions_and_custom_tools() {
    let request = responses::decode(json!({"model":"m", "tools":[
        {"type":"function","name":"arg_namespaced_0"},
        {"type":"namespace","name":"one","tools":[{"type":"function","name":"same"},{"type":"custom","name":"text"}]},
        {"type":"namespace","name":"two","tools":[{"type":"function","name":"same"}]}
    ]}), None).unwrap();
    let registry = CustomToolBridge::new(request.tools.as_ref().unwrap()).unwrap();
    let mut aliases = std::collections::BTreeSet::new();
    for definition in request.tool_definitions() {
        let original = ToolCall {
            item_id: Some(ItemId::new("item").unwrap()),
            call_id: CallId::new("call").unwrap(),
            tool: definition.identity.clone(),
            input: if definition.kind() == Some(ToolKind::Custom) {
                ToolInput::Freeform("exact\n한글".into())
            } else {
                ToolInput::Json("{\"n\":9007199254740993123}".into())
            },
            extensions: Extensions::responses(),
        };
        let alias = registry.alias(&original.tool).unwrap();
        assert!(aliases.insert(alias.clone()));
        assert!(alias.namespace.is_none());
        let lower = registry.lower_call(&original).unwrap();
        assert!(registry.restore_call(&lower).unwrap() == original);
        let choice = ToolChoice::Named {
            tool: original.tool.clone(),
            kind: original.input.kind(),
            extensions: Extensions::responses(),
        };
        assert!(
            matches!(registry.lower_choice(&choice).unwrap(), ToolChoice::Named { tool, kind: ToolKind::Function, .. } if tool == *alias)
        );
        let result = ToolResult {
            item_id: None,
            call_id: original.call_id.clone(),
            kind: original.input.kind(),
            output: json!("result"),
            extensions: Extensions::responses(),
        };
        assert!(
            registry
                .restore_result(
                    &registry.lower_result(&result, &original).unwrap(),
                    &original
                )
                .unwrap()
                == result
        );
    }
    assert_ne!(registry.definitions()[1].identity.name, "arg_namespaced_0");
}
