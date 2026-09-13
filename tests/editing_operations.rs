use agent_response_gateway::editing::{ContextEdit, Operation, OperationBundle};
use serde_json::json;

fn bundle() -> OperationBundle {
    OperationBundle {
        operations: vec![
            Operation::Create {
                path: "생성.txt".into(),
                lines: vec!["".into(), " Unicode ".into(), "*** End Patch".into()],
            },
            Operation::Delete {
                path: "deleted.txt".into(),
            },
            Operation::Move {
                source: "from.txt".into(),
                destination: "to.txt".into(),
                context: vec!["existing".into()],
            },
            Operation::Update {
                edit: ContextEdit {
                    path: "updated.txt".into(),
                    before_context: vec![" context".into()],
                    old_lines: vec!["old".into()],
                    new_lines: vec!["new\\\"".into()],
                    after_context: vec!["".into()],
                },
            },
        ],
    }
}

#[test]
fn operations_round_trip_order_and_unicode_without_invented_status() {
    let b = bundle();
    let patch = b.compile().unwrap();
    assert_eq!(OperationBundle::from_patch(&patch).unwrap(), b);
    assert_eq!(
        OperationBundle::from_json(&serde_json::to_string(&b).unwrap()).unwrap(),
        b
    );
    for op in b.operations {
        let single = OperationBundle {
            operations: vec![op],
        };
        assert_eq!(
            OperationBundle::from_patch(&single.compile().unwrap()).unwrap(),
            single
        );
    }
    assert!(OperationBundle::from_patch(&(patch + "\n")).is_err());
}

#[test]
fn operations_reject_dependencies_aliases_and_invalid_inputs() {
    for (a, b) in [
        ("same", "same"),
        ("a/./b", "a/b"),
        ("x/../b", "b"),
        ("a\\b", "a/b"),
        ("Name", "name"),
        ("a", "a/child"),
        ("/a/b", "/a//b"),
    ] {
        let ops = OperationBundle {
            operations: vec![
                Operation::Delete { path: a.into() },
                Operation::Delete { path: b.into() },
            ],
        };
        assert!(ops.compile().is_err(), "{a}, {b}");
    }
    for path in ["", "bad\n*** End Patch", "../escape", "a/..", "a. "] {
        assert!(
            OperationBundle {
                operations: vec![Operation::Delete { path: path.into() }]
            }
            .compile()
            .is_err()
        );
    }
    let mut b = bundle();
    b.operations.push(Operation::Delete {
        path: "to.txt".into(),
    });
    assert!(b.compile().is_err());
    assert!(
        OperationBundle {
            operations: vec![Operation::Move {
                source: "a".into(),
                destination: "a".into(),
                context: vec!["x".into()]
            }]
        }
        .compile()
        .is_err()
    );
    for raw in [
        r#"{"operations":[],"operations":[]}"#,
        r#"{"operations":[{"operation":"delete","path":"a","path":"b"}]}"#,
        r#"{"operations":[{"operation":"delete","operation":"delete","path":"a"}]}"#,
        r#"{"operations":[{"operation":"delete","path":"a","unknown":1}]}"#,
        r#"{"operations":[{"operation":"move","source":"a","destination":"b"}]}"#,
    ] {
        assert!(OperationBundle::from_json(raw).is_err(), "{raw}");
    }
    for value in [
        json!({"operations":[]}),
        json!({"operations":[{"operation":"create","path":"a","lines":[]}]}),
        json!({"operations":[{"operation":"move","source":"a","destination":"b","context":[]}]}),
        json!({"operations":[{"operation":"create","path":"a","lines":["a\nb"]}]}),
    ] {
        assert!(
            OperationBundle::from_json(&value.to_string())
                .unwrap()
                .compile()
                .is_err()
        );
    }
    assert!(
        OperationBundle {
            operations: vec![Operation::Delete { path: "a".into() }; 65]
        }
        .compile()
        .is_err()
    );
}
