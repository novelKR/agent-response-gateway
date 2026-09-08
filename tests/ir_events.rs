use agent_response_gateway::ir::{
    CallId, IrError, ItemId, ResponseId, ToolIdentity, ToolKind,
    event::{
        ContentIndex, EventIR, EventLimits, EventValidator, OutputIndex, OutputKind, PartKind,
        Terminal, Usage,
    },
};

fn item(name: &str) -> ItemId {
    ItemId::new(name).unwrap()
}
fn validator() -> EventValidator {
    let mut state = EventValidator::new(EventLimits::default()).unwrap();
    state
        .apply(EventIR::Started {
            id: ResponseId::new("r").unwrap(),
        })
        .unwrap();
    state
}
fn start_tool(state: &mut EventValidator, name: &str, index: u32, kind: ToolKind) {
    state
        .apply(EventIR::ItemStarted {
            id: item(name),
            index: OutputIndex(index),
            kind: OutputKind::Tool {
                tool: ToolIdentity::new(None, "tool").unwrap(),
                call_id: CallId::new(format!("call_{name}")).unwrap(),
                kind,
            },
        })
        .unwrap();
}
fn finish(status: Terminal) -> EventIR {
    EventIR::Finished {
        status,
        reason: None,
    }
}

#[test]
fn interleaved_tools_keep_their_fragments_indices_and_call_ids() {
    let mut state = validator();
    start_tool(&mut state, "a", 0, ToolKind::Function);
    start_tool(&mut state, "b", 1, ToolKind::Custom);
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "{\"value\":".into(),
        })
        .unwrap();
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("b"),
            text: "*** Begin Patch\n".into(),
        })
        .unwrap();
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "\"한글\"}".into(),
        })
        .unwrap();
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("b"),
            text: "*** End Patch".into(),
        })
        .unwrap();
    state
        .apply(EventIR::ItemFinished { item: item("b") })
        .unwrap();
    state
        .apply(EventIR::ItemFinished { item: item("a") })
        .unwrap();
    assert_eq!(state.arguments(&item("a")).unwrap(), "{\"value\":\"한글\"}");
    assert_eq!(
        state.arguments(&item("b")).unwrap(),
        "*** Begin Patch\n*** End Patch"
    );
    state.apply(finish(Terminal::Completed)).unwrap();
    assert_eq!(state.terminal(), Some(Terminal::Completed));
}

#[test]
fn complete_json_is_validated_only_at_item_finish_at_every_string_split() {
    let raw = "{\"한글\":\"quote \\\" emoji 🧪\",\"n\":184467440737095516160}";
    for split in (0..=raw.len()).filter(|i| raw.is_char_boundary(*i)) {
        let mut state = validator();
        start_tool(&mut state, "a", 0, ToolKind::Function);
        state
            .apply(EventIR::ArgumentsDelta {
                item: item("a"),
                text: raw[..split].into(),
            })
            .unwrap();
        assert_eq!(
            state.arguments(&item("a")).err(),
            Some(IrError::InvalidEventOrder)
        );
        state
            .apply(EventIR::ArgumentsDelta {
                item: item("a"),
                text: raw[split..].into(),
            })
            .unwrap();
        state
            .apply(EventIR::ItemFinished { item: item("a") })
            .unwrap();
        assert_eq!(state.arguments(&item("a")).unwrap(), raw);
    }
}

#[test]
fn invalid_json_finish_does_not_complete_item_or_response() {
    let mut state = validator();
    start_tool(&mut state, "a", 0, ToolKind::Function);
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "{".into(),
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::ItemFinished { item: item("a") }),
        Err(IrError::InvalidJsonArguments)
    );
    assert_eq!(
        state.apply(finish(Terminal::Completed)),
        Err(IrError::InvalidEventOrder)
    );
    assert_eq!(state.terminal(), None);
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "}".into(),
        })
        .unwrap();
    state
        .apply(EventIR::ItemFinished { item: item("a") })
        .unwrap();
    state.apply(finish(Terminal::Completed)).unwrap();
}

#[test]
fn message_and_reasoning_parts_must_start_and_finish_in_order() {
    let mut state = validator();
    state
        .apply(EventIR::ItemStarted {
            id: item("m"),
            index: OutputIndex(0),
            kind: OutputKind::Message,
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::TextDelta {
            item: item("m"),
            index: ContentIndex(0),
            text: "early".into()
        }),
        Err(IrError::InvalidEventOrder)
    );
    state
        .apply(EventIR::PartStarted {
            item: item("m"),
            index: ContentIndex(0),
            kind: PartKind::Text,
        })
        .unwrap();
    state
        .apply(EventIR::TextDelta {
            item: item("m"),
            index: ContentIndex(0),
            text: "content".into(),
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::ItemFinished { item: item("m") }),
        Err(IrError::InvalidEventOrder)
    );
    state
        .apply(EventIR::PartFinished {
            item: item("m"),
            index: ContentIndex(0),
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::TextDelta {
            item: item("m"),
            index: ContentIndex(0),
            text: "late".into()
        }),
        Err(IrError::InvalidEventOrder)
    );
    state
        .apply(EventIR::ItemFinished { item: item("m") })
        .unwrap();
    state
        .apply(EventIR::ItemStarted {
            id: item("r"),
            index: OutputIndex(1),
            kind: OutputKind::Reasoning,
        })
        .unwrap();
    assert!(
        state
            .apply(EventIR::PartStarted {
                item: item("r"),
                index: ContentIndex(0),
                kind: PartKind::Text
            })
            .is_err()
    );
    state
        .apply(EventIR::PartStarted {
            item: item("r"),
            index: ContentIndex(0),
            kind: PartKind::ReasoningText,
        })
        .unwrap();
    state
        .apply(EventIR::PartFinished {
            item: item("r"),
            index: ContentIndex(0),
        })
        .unwrap();
    state
        .apply(EventIR::ItemFinished { item: item("r") })
        .unwrap();
    state.apply(finish(Terminal::Completed)).unwrap();
}

#[test]
fn duplicate_response_item_index_call_and_terminal_are_rejected() {
    let mut state = EventValidator::new(EventLimits::default()).unwrap();
    assert_eq!(
        state.apply(finish(Terminal::Completed)),
        Err(IrError::InvalidEventOrder)
    );
    state
        .apply(EventIR::Started {
            id: ResponseId::new("r").unwrap(),
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::Started {
            id: ResponseId::new("r2").unwrap()
        }),
        Err(IrError::InvalidEventOrder)
    );
    start_tool(&mut state, "a", 0, ToolKind::Custom);
    for (id, index, call) in [("a", 1, "new"), ("b", 0, "new"), ("b", 1, "call_a")] {
        assert_eq!(
            state.apply(EventIR::ItemStarted {
                id: item(id),
                index: OutputIndex(index),
                kind: OutputKind::Tool {
                    tool: ToolIdentity::new(None, "f").unwrap(),
                    call_id: CallId::new(call).unwrap(),
                    kind: ToolKind::Custom
                }
            }),
            Err(IrError::DuplicateId)
        );
    }
    state.apply(finish(Terminal::TransportLost)).unwrap();
    assert_eq!(
        state.apply(finish(Terminal::Completed)),
        Err(IrError::InvalidEventOrder)
    );
}

#[test]
fn failures_and_incomplete_termination_preserve_class_and_reason() {
    for terminal in [
        Terminal::Incomplete,
        Terminal::Failed,
        Terminal::Cancelled,
        Terminal::TransportLost,
    ] {
        let mut state = validator();
        start_tool(&mut state, "pending", 0, ToolKind::Function);
        state
            .apply(EventIR::ArgumentsDelta {
                item: item("pending"),
                text: "{".into(),
            })
            .unwrap();
        state
            .apply(EventIR::Finished {
                status: terminal,
                reason: Some("synthetic_reason".into()),
            })
            .unwrap();
        assert_eq!(state.terminal(), Some(terminal));
        assert_eq!(state.terminal_reason(), Some("synthetic_reason"));
        assert!(
            state
                .apply(EventIR::ArgumentsDelta {
                    item: item("pending"),
                    text: "}".into()
                })
                .is_err()
        );
        assert!(state.arguments(&item("pending")).is_err());
    }
}

#[test]
fn resource_limits_and_wrong_item_references_leave_state_usable() {
    let limits = EventLimits {
        max_items: 1,
        max_parts: 1,
        max_delta_bytes: 4,
        max_argument_bytes: 5,
    };
    let mut state = EventValidator::new(limits).unwrap();
    state
        .apply(EventIR::Started {
            id: ResponseId::new("r").unwrap(),
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::ArgumentsDelta {
            item: item("missing"),
            text: "x".into()
        }),
        Err(IrError::UnknownItem)
    );
    start_tool(&mut state, "a", 0, ToolKind::Custom);
    assert!(
        state
            .apply(EventIR::ItemStarted {
                id: item("b"),
                index: OutputIndex(1),
                kind: OutputKind::Message
            })
            .is_err()
    );
    assert_eq!(
        state.apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "12345".into()
        }),
        Err(IrError::SizeLimit)
    );
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "1234".into(),
        })
        .unwrap();
    assert_eq!(
        state.apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "xx".into()
        }),
        Err(IrError::SizeLimit)
    );
    state
        .apply(EventIR::ArgumentsDelta {
            item: item("a"),
            text: "5".into(),
        })
        .unwrap();
    state
        .apply(EventIR::ItemFinished { item: item("a") })
        .unwrap();
    assert_eq!(state.arguments(&item("a")).unwrap(), "12345");
}

#[test]
fn usage_updates_are_cumulative_and_omitted_fields_do_not_erase_counts() {
    let mut state = validator();
    state
        .apply(EventIR::UsageUpdated(Usage {
            input_tokens: Some(10),
            output_tokens: Some(1),
        }))
        .unwrap();
    state
        .apply(EventIR::UsageUpdated(Usage {
            input_tokens: None,
            output_tokens: Some(2),
        }))
        .unwrap();
    assert_eq!(
        state.usage(),
        Some(&Usage {
            input_tokens: Some(10),
            output_tokens: Some(2)
        })
    );
    assert_eq!(
        state.apply(EventIR::UsageUpdated(Usage {
            input_tokens: Some(9),
            output_tokens: Some(3)
        })),
        Err(IrError::InvalidEventOrder)
    );
    assert_eq!(state.usage().unwrap().output_tokens, Some(2));
    state.apply(finish(Terminal::Completed)).unwrap();
    assert!(
        state
            .apply(EventIR::UsageUpdated(Usage {
                input_tokens: None,
                output_tokens: Some(3)
            }))
            .is_err()
    );
}
