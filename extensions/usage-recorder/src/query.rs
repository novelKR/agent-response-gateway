use crate::{Result, Store};
use chrono::TimeZone;
use gateway_usage_contract::{EventKind, FIELDS, Finality, UsageEvent};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
struct Group {
    count: u64,
    final_count: u64,
    partial: u64,
    unobserved: u64,
    unfinished: u64,
    observation_incomplete: u64,
    sums: BTreeMap<String, u128>,
    known: BTreeMap<String, u64>,
    ratio_input: u128,
    ratio_read: u128,
    ratio_calls: u64,
}
pub fn aggregate(store: &Store, from: u64, to: u64, timezone: &str) -> Result<Value> {
    let tz: chrono_tz::Tz = timezone.parse()?;
    let mut groups: BTreeMap<(String, String, String), Group> = BTreeMap::new();
    let mut statement=store.connection.prepare("SELECT payload FROM usage_current WHERE started_at_ms>=?1 AND started_at_ms<?2 ORDER BY started_at_ms")?;
    let rows = statement.query_map(
        rusqlite::params![i64::try_from(from)?, i64::try_from(to)?],
        |r| r.get::<_, String>(0),
    )?;
    for row in rows {
        let e: UsageEvent = serde_json::from_str(&row?)?;
        let date = tz
            .timestamp_millis_opt(i64::try_from(e.started_at_ms)?)
            .single()
            .ok_or("invalid_timestamp")?
            .format("%Y-%m-%d")
            .to_string();
        let g = groups.entry((date, e.provider, e.model_alias)).or_default();
        g.count += 1;
        match e.finality {
            Finality::Final => g.final_count += 1,
            Finality::Partial => g.partial += 1,
            Finality::Unobserved => g.unobserved += 1,
        }
        if e.kind != EventKind::AttemptFinished {
            g.unfinished += 1;
        }
        if e.observation_incomplete {
            g.observation_incomplete += 1;
        }
        for field in FIELDS {
            if let Some(n) = e.usage.value(field) {
                *g.sums.entry(field.into()).or_default() += u128::from(n);
                *g.known.entry(field.into()).or_default() += 1;
            }
        }
        if let (Some(input), Some(read)) = (
            e.usage.value("input_tokens"),
            e.usage.value("cache_read_input_tokens"),
        ) {
            *g.sums.entry("non_read_input_tokens".into()).or_default() += u128::from(input - read);
            *g.known.entry("non_read_input_tokens".into()).or_default() += 1;
            g.ratio_input += u128::from(input);
            g.ratio_read += u128::from(read);
            g.ratio_calls += 1;
        }
    }
    let data:Vec<_>=groups.into_iter().map(|((date,provider,model),g)|json!({"date":date,"provider":provider,"model_alias":model,"calls":g.count,"final":g.final_count,"partial":g.partial,"unobserved":g.unobserved,"unfinished":g.unfinished,"observation_incomplete":g.observation_incomplete,"token_sums":g.sums,"observed_calls":g.known,"cache_read_ratio":if g.ratio_input>0{Some(g.ratio_read as f64/g.ratio_input as f64)}else{None},"cache_read_ratio_calls":g.ratio_calls,"cache_read_ratio_excluded_calls":g.count-g.ratio_calls})).collect();
    Ok(
        json!({"timezone":timezone,"attribution":"attempt_started_at","from_ms":from,"to_ms":to,"groups":data}),
    )
}
pub fn status(store: &Store) -> Result<Value> {
    let mut s = store
        .connection
        .prepare("SELECT state,COUNT(*),MIN(next_at_ms) FROM usage_outbox GROUP BY state")?;
    let rows=s.query_map([],|r|Ok(json!({"state":r.get::<_,String>(0)?,"count":r.get::<_,i64>(1)?,"next_at_ms":r.get::<_,i64>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let unfinished: i64 = store.connection.query_row(
        "SELECT COUNT(*) FROM usage_current WHERE kind!='attempt_finished'",
        [],
        |r| r.get(0),
    )?;
    Ok(
        json!({"producer_id":store.producer()?,"storage_schema":1,"unfinished_outcome":"unknown","unfinished_calls":unfinished,"outbox":rows}),
    )
}
