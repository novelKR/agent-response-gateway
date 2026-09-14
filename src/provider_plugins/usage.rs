//! Generic numeric checks only: no builtin provider profile or raw-provider parser.
use super::contract::{Counter, UsageSnapshot};
use crate::ir::IrError;
use gateway_usage_contract::{CanonicalUsage, Counter as HostCounter, Source};
use serde_json::Value;

fn invalid(usage: &mut CanonicalUsage, field: &str, reason: &str) {
    usage.counters.insert(
        field.into(),
        HostCounter {
            value: None,
            source: Source::Invalid,
        },
    );
    if !usage.violations.iter().any(|value| value == reason) {
        usage.violations.push(reason.into());
    }
}
/// Preserve invalid-vs-missing evidence before the caller decides whether output is admissible.
pub(super) fn observe(snapshot: &UsageSnapshot) -> CanonicalUsage {
    let mut usage = CanonicalUsage::default();
    if let UsageSnapshot::Observed { counters } = snapshot {
        for (name, counter) in [
            ("input_tokens", &counters.input_tokens),
            ("output_tokens", &counters.output_tokens),
            ("total_tokens", &counters.total_tokens),
            ("input_regular_tokens", &counters.input_regular_tokens),
            ("cache_read_input_tokens", &counters.cache_read_input_tokens),
            (
                "cache_write_input_tokens",
                &counters.cache_write_input_tokens,
            ),
            ("reasoning_output_tokens", &counters.reasoning_output_tokens),
        ] {
            let (value, source) = match counter {
                Counter::Reported { value } => (Some(*value), Source::Reported),
                Counter::NotReported => (None, Source::NotReported),
                Counter::NotApplicable => (None, Source::NotApplicable),
                Counter::Invalid => {
                    invalid(&mut usage, name, "invalid_counter");
                    continue;
                }
            };
            usage
                .counters
                .insert(name.into(), HostCounter { value, source });
        }
    }
    if let (Some(input), Some(output)) = (usage.value("input_tokens"), usage.value("output_tokens"))
    {
        match input.checked_add(output) {
            None => invalid(&mut usage, "total_tokens", "total_mismatch"),
            Some(sum) => {
                if usage
                    .value("total_tokens")
                    .is_some_and(|total| total != sum)
                {
                    invalid(&mut usage, "total_tokens", "total_mismatch");
                } else if usage.counters["total_tokens"].source == Source::NotReported {
                    usage.counters.insert(
                        "total_tokens".into(),
                        HostCounter {
                            value: Some(sum),
                            source: Source::Derived,
                        },
                    );
                }
            }
        }
    }
    for (part, total) in [
        ("input_regular_tokens", "input_tokens"),
        ("cache_read_input_tokens", "input_tokens"),
        ("cache_write_input_tokens", "input_tokens"),
        ("reasoning_output_tokens", "output_tokens"),
    ] {
        if let (Some(part_value), Some(total)) = (usage.value(part), usage.value(total))
            && part_value > total
        {
            invalid(&mut usage, part, "subset_exceeds_total");
        }
    }
    let parts = [
        "input_regular_tokens",
        "cache_read_input_tokens",
        "cache_write_input_tokens",
    ];
    let known_sum = parts
        .into_iter()
        .filter_map(|field| usage.value(field))
        .try_fold(0_u64, |sum, value| sum.checked_add(value));
    let all_known = parts.into_iter().all(|field| usage.value(field).is_some());
    if known_sum.is_none()
        || usage.value("input_tokens").is_some_and(|input| {
            known_sum.is_some_and(|sum| sum > input || (all_known && sum != input))
        })
    {
        for field in [
            "input_tokens",
            "input_regular_tokens",
            "cache_read_input_tokens",
            "cache_write_input_tokens",
        ] {
            if usage.value(field).is_some() {
                invalid(&mut usage, field, "input_partition");
            }
        }
        if usage.counters["total_tokens"].source == Source::Derived {
            invalid(&mut usage, "total_tokens", "input_partition");
        }
    }
    usage
}
/// Recovery preserves only typed invalid evidence when the public numeric shape fails.
pub(super) fn observe_wire(value: &Value) -> CanonicalUsage {
    if let Ok(snapshot) = serde_json::from_value::<UsageSnapshot>(value.clone()) {
        return observe(&snapshot);
    }
    // Keep valid siblings only when the enclosing usage and complete counter set
    // have the declared shape. Malformed scalar/field shapes become typed Invalid.
    if value.as_object().is_some_and(|object| {
        object.len() == 2 && object.contains_key("kind") && object.contains_key("counters")
    }) && value["kind"] == "observed"
        && value["counters"].as_object().is_some_and(|counters| {
            counters.len() == gateway_usage_contract::FIELDS.len()
                && gateway_usage_contract::FIELDS
                    .iter()
                    .all(|field| counters.contains_key(*field))
        })
    {
        let mut sanitized = value.clone();
        for field in gateway_usage_contract::FIELDS {
            if serde_json::from_value::<Counter>(sanitized["counters"][field].clone()).is_err() {
                sanitized["counters"][field] = serde_json::json!({"source":"invalid"});
            }
        }
        if let Ok(snapshot) = serde_json::from_value::<UsageSnapshot>(sanitized) {
            return observe(&snapshot);
        }
    }
    let mut usage = CanonicalUsage::default();
    for field in gateway_usage_contract::FIELDS {
        invalid(&mut usage, field, "invalid_counter");
    }
    usage
}
pub(super) fn ensure_valid(usage: &CanonicalUsage) -> Result<(), IrError> {
    if usage.violations.is_empty() {
        Ok(())
    } else {
        Err(IrError::InvalidField("provider_usage"))
    }
}
#[cfg(test)]
pub(super) fn validate(snapshot: &UsageSnapshot) -> Result<CanonicalUsage, IrError> {
    let usage = observe(snapshot);
    ensure_valid(&usage)?;
    Ok(usage)
}
pub(super) fn observe_cumulative(previous: &CanonicalUsage, current: &mut CanonicalUsage) {
    for field in gateway_usage_contract::FIELDS {
        if current.counters[field].source == Source::Invalid {
            continue;
        }
        if let Some(previous) = previous.value(field)
            && current.value(field).is_none_or(|value| value < previous)
        {
            invalid(current, field, "counter_decreased");
        }
    }
    if current.counters["total_tokens"].source == Source::Derived
        && ["input_tokens", "output_tokens"]
            .iter()
            .any(|field| current.counters[*field].source == Source::Invalid)
    {
        invalid(current, "total_tokens", "counter_decreased");
    }
}
#[cfg(test)]
pub(super) fn cumulative(
    previous: &CanonicalUsage,
    current: &CanonicalUsage,
) -> Result<(), IrError> {
    let mut observed = current.clone();
    observe_cumulative(previous, &mut observed);
    ensure_valid(&observed)
}
pub(super) fn verify_response(response: &Value, usage: &CanonicalUsage) -> Result<(), IrError> {
    let expected = usage.responses();
    let actual = response.get("usage");
    if actual == Some(&expected) || (!usage.observed() && actual.is_none_or(Value::is_null)) {
        Ok(())
    } else {
        Err(IrError::InvalidField("provider_usage"))
    }
}
