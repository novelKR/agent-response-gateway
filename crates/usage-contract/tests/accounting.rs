use gateway_usage_contract::*;
use serde_json::json;
fn normalized(p: Profile, v: serde_json::Value) -> CanonicalUsage {
    normalize(p, extract(p, &v))
}
#[test]
fn messages_partition_and_ttl_are_preserved() {
    let u = normalized(
        Profile::MessagesV1,
        json!({"input_tokens":10,"output_tokens":3,"cache_read_input_tokens":5,"cache_creation_input_tokens":2,"cache_creation":{"ephemeral_5m_input_tokens":1,"ephemeral_1h_input_tokens":1}}),
    );
    assert_eq!(
        (
            u.value("input_tokens"),
            u.value("input_regular_tokens"),
            u.non_read_input_tokens()
        ),
        (Some(17), Some(10), Some(12))
    );
    assert_eq!(u.value("total_tokens"), Some(20));
    assert_eq!(u.cache_write_details.len(), 2);
    assert!(u.validate());
}
#[test]
fn missing_write_is_not_zero_or_regular_input() {
    let u = normalized(
        Profile::ChatV1,
        json!({"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":4}}),
    );
    assert_eq!(u.non_read_input_tokens(), Some(6));
    assert_eq!(u.value("cache_write_input_tokens"), None);
    assert_eq!(u.value("input_regular_tokens"), None);
    assert_eq!(
        u.counters["cache_write_input_tokens"].source,
        Source::NotReported
    );
}
#[test]
fn explicit_zero_missing_and_invalid_are_distinct() {
    let zero = normalized(
        Profile::ResponsesV1,
        json!({"input_tokens":0,"output_tokens":0}),
    );
    let missing = normalized(Profile::ResponsesV1, json!({}));
    let invalid = normalized(
        Profile::ResponsesV1,
        json!({"input_tokens":-1,"output_tokens":true}),
    );
    assert_eq!(zero.value("total_tokens"), Some(0));
    assert_eq!(missing.counters["input_tokens"].source, Source::NotReported);
    assert_eq!(invalid.counters["input_tokens"].source, Source::Invalid);
}
#[test]
fn checked_sums_and_subsets_do_not_saturate_or_double_count() {
    let u = normalized(
        Profile::ResponsesV1,
        json!({"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_tokens_details":{"cached_tokens":4,"cache_write_tokens":2},"output_tokens_details":{"reasoning_tokens":3}}),
    );
    assert_eq!(u.value("total_tokens"), Some(15));
    assert_eq!(u.value("input_regular_tokens"), Some(4));
    let u = normalized(
        Profile::ResponsesV1,
        json!({"input_tokens":u64::MAX,"output_tokens":1}),
    );
    assert_eq!(u.counters["total_tokens"].source, Source::Invalid);
    let u = normalized(
        Profile::ResponsesV1,
        json!({"input_tokens":2,"output_tokens":1,"total_tokens":99,"input_tokens_details":{"cached_tokens":3}}),
    );
    assert_eq!(u.value("cache_read_input_tokens"), None);
    assert_eq!(u.value("total_tokens"), None);
}
#[test]
fn cumulative_snapshot_merges_missing_fields_without_adding() {
    let mut a = Accumulator::new(Profile::MessagesV1);
    a.observe(&json!({"input_tokens":10,"cache_read_input_tokens":5,"cache_creation_input_tokens":2,"output_tokens":1}));
    a.observe(&json!({"output_tokens":3}));
    a.observe(&json!({"output_tokens":8}));
    assert_eq!(a.usage.value("output_tokens"), Some(8));
    assert_eq!(a.usage.value("input_tokens"), Some(17));
    assert_eq!(a.usage.value("total_tokens"), Some(25));
    a.observe(&json!({"output_tokens":7}));
    assert!(a.incomplete);
    assert_eq!(a.usage.counters["output_tokens"].source, Source::Invalid);
}
#[test]
fn arbitrary_usage_metadata_is_not_retained() {
    let u = normalized(
        Profile::ResponsesV1,
        json!({"input_tokens":3,"output_tokens":2,"prompt":"synthetic-private-marker","extensions":{"credential":"synthetic-secret-marker"}}),
    );
    let raw = serde_json::to_string(&u).unwrap();
    assert!(!raw.contains("marker"));
    assert!(u.validate());
}
#[test]
fn ttl_mismatch_is_visible() {
    let u = normalized(
        Profile::MessagesV1,
        json!({"input_tokens":1,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":3,"cache_creation":{"ephemeral_5m_input_tokens":1,"ephemeral_1h_input_tokens":1}}),
    );
    assert!(u.violations.iter().any(|s| s == "ttl_mismatch"));
}

#[test]
fn malformed_usage_is_invalid_not_missing() {
    let mut a = Accumulator::new(Profile::ResponsesV1);
    a.observe(&json!("synthetic-invalid-shape"));
    assert!(a.incomplete);
    assert_eq!(a.usage.counters["input_tokens"].source, Source::Invalid);
    assert!(
        !serde_json::to_string(&a.usage)
            .unwrap()
            .contains("synthetic-invalid-shape")
    );
}
