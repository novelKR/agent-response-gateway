//! Only ledger events are retried. This module has no model transport entry point.
use crate::{Result, Store, now_ms, read_private};
use gateway_usage_contract::{
    Batch, BatchReceipt, Destination, Receipt, ReceiptStatus, RecorderConfig, UsageEvent, digest,
};
use postgres::{Client, config::SslMode};
use rusqlite::params;
use std::{io::Read, path::Path, time::Duration};

fn postgres(destination: &Destination) -> Result<Client> {
    let Destination::Postgres {
        connection_file,
        tls_ca_file,
        ..
    } = destination
    else {
        return Err("not_postgres".into());
    };
    let raw = read_private(Path::new(connection_file), 8192)?;
    let text = std::str::from_utf8(&raw)?.trim();
    let mut config: postgres::Config = text.parse()?;
    // TLS certificate and host validation are mandatory, including on loopback.
    config
        .ssl_mode(SslMode::Require)
        .connect_timeout(Duration::from_secs(5));
    let mut tls = native_tls::TlsConnector::builder();
    if let Some(path) = tls_ca_file {
        tls.add_root_certificate(native_tls::Certificate::from_pem(&read_private(
            Path::new(path),
            65_536,
        )?)?);
    }
    let tls = tls.build()?;
    let mut client = config.connect(postgres_native_tls::MakeTlsConnector::new(tls))?;
    client.batch_execute("SET statement_timeout='5s'; SET lock_timeout='3s';")?;
    Ok(client)
}
pub fn initialize_postgres(destination: &Destination) -> Result<()> {
    let mut c = postgres(destination)?;
    // No implicit migration or writes to a consumer-owned schema.
    c.batch_execute("BEGIN; CREATE SCHEMA gateway_usage;
CREATE TABLE gateway_usage.metadata(version INTEGER PRIMARY KEY CHECK(version=1)); INSERT INTO gateway_usage.metadata VALUES(1);
CREATE TABLE gateway_usage.events(producer_id TEXT NOT NULL,event_id TEXT NOT NULL,attempt_id TEXT NOT NULL,revision BIGINT NOT NULL,identity TEXT NOT NULL,kind TEXT NOT NULL,sha256 TEXT NOT NULL,payload TEXT NOT NULL,input_tokens NUMERIC(20,0),output_tokens NUMERIC(20,0),PRIMARY KEY(producer_id,event_id),UNIQUE(producer_id,attempt_id,revision));
CREATE VIEW gateway_usage.current AS SELECT e.* FROM gateway_usage.events e WHERE revision=(SELECT MAX(x.revision) FROM gateway_usage.events x WHERE x.producer_id=e.producer_id AND x.attempt_id=e.attempt_id); COMMIT;")?;
    Ok(())
}
fn pg_batch(d: &Destination, events: &[UsageEvent]) -> Result<Vec<Receipt>> {
    let mut c = postgres(d)?;
    let version: i32 = c
        .query_one("SELECT version FROM gateway_usage.metadata", &[])?
        .get(0);
    if version != 1 {
        return Err("unsupported_remote_schema".into());
    }
    let mut tx = c.transaction()?;
    let mut receipts = vec![];
    for e in events {
        let bytes = e.bytes()?;
        let hash = digest(&bytes);
        let text = String::from_utf8(bytes)?;
        let rev = i64::try_from(e.revision)?;
        let identity = serde_json::to_string(&(
            &e.request_id,
            e.started_at_ms,
            &e.provider,
            &e.model_alias,
            &e.upstream_model,
            e.profile,
            &e.configuration_sha256,
        ))?;
        // Serialize all revisions of an attempt, including concurrent independent exporters.
        let key = format!("{}:{}", e.producer_id, e.attempt_id);
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            &[&key],
        )?;
        let duplicate=tx.query_opt("SELECT sha256 FROM gateway_usage.events WHERE producer_id=$1 AND (event_id=$2 OR (attempt_id=$3 AND revision=$4))",&[&e.producer_id,&e.event_id,&e.attempt_id,&rev])?;
        let status = if let Some(row) = duplicate {
            if row.get::<_, String>(0) == hash {
                ReceiptStatus::Duplicate
            } else {
                ReceiptStatus::Conflict
            }
        } else {
            let rows=tx.query("SELECT identity,revision,kind FROM gateway_usage.events WHERE producer_id=$1 AND attempt_id=$2",&[&e.producer_id,&e.attempt_id])?;
            let finished = e.kind == gateway_usage_contract::EventKind::AttemptFinished;
            let conflict = rows.iter().any(|r| {
                r.get::<_, String>(0) != identity
                    || (finished && r.get::<_, i64>(1) > rev)
                    || (r.get::<_, String>(2) == "attempt_finished"
                        && (rev >= r.get::<_, i64>(1) || finished))
            });
            if conflict {
                ReceiptStatus::Conflict
            } else {
                let kind = match e.kind {
                    gateway_usage_contract::EventKind::AttemptStarted => "attempt_started",
                    gateway_usage_contract::EventKind::UsageUpdated => "usage_updated",
                    gateway_usage_contract::EventKind::AttemptFinished => "attempt_finished",
                };
                let input = e.usage.value("input_tokens").map(|v| v.to_string());
                let output = e.usage.value("output_tokens").map(|v| v.to_string());
                tx.execute("INSERT INTO gateway_usage.events VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9::text::numeric,$10::text::numeric)",&[&e.producer_id,&e.event_id,&e.attempt_id,&rev,&identity,&kind,&hash,&text,&input,&output])?;
                ReceiptStatus::Committed
            }
        };
        receipts.push(Receipt {
            producer_id: e.producer_id.clone(),
            event_id: e.event_id.clone(),
            sha256: hash,
            status,
        });
    }
    tx.commit()?;
    Ok(receipts)
}
fn http_batch(d: &Destination, events: &[UsageEvent]) -> Result<Vec<Receipt>> {
    let Destination::Http {
        url, bearer_file, ..
    } = d
    else {
        return Err("not_http".into());
    };
    let url = reqwest::Url::parse(url)?;
    let loopback = url
        .host_str()
        .and_then(|h| h.parse::<std::net::IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_export_url".into());
    }
    let secret = read_private(Path::new(bearer_file), 8192)?;
    let secret = std::str::from_utf8(&secret)?.trim();
    if secret.is_empty() {
        return Err("empty_export_credential".into());
    }
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .post(url)
        .bearer_auth(secret)
        .json(&Batch {
            schema: "gateway-usage-batch/v1".into(),
            events: events.to_vec(),
        })
        .send()?;
    let status = response.status();
    if status.is_client_error() && !matches!(status.as_u16(), 408 | 429) {
        return Err("permanent_export_error".into());
    }
    if status != reqwest::StatusCode::OK {
        return Err("retryable_export_error".into());
    }
    let mut bytes = vec![];
    response.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("invalid_export_ack".into());
    }
    let ack: BatchReceipt = serde_json::from_slice(&bytes).map_err(|_| "invalid_export_ack")?;
    if ack.schema != "gateway-usage-batch-receipt/v1" || ack.receipts.len() != events.len() {
        return Err("invalid_export_ack".into());
    }
    Ok(ack.receipts)
}
pub fn run_once(store: &mut Store, config: &RecorderConfig) -> Result<usize> {
    let mut sent = 0;
    for d in &config.destinations {
        let mut stmt=store.connection.prepare("SELECT e.payload,o.attempts FROM usage_outbox o JOIN usage_events e USING(producer_id,event_id) WHERE o.destination=?1 AND o.state='pending' AND o.next_at_ms<=?2 ORDER BY e.observed_at_ms,e.producer_id,e.attempt_id,e.revision LIMIT 100")?;
        let rows = stmt
            .query_map(params![d.id(), now_ms() as i64], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut events = vec![];
        let mut attempts = vec![];
        let mut bytes = 128;
        for (payload, n) in rows {
            if bytes + payload.len() + 1 > 1_048_576 {
                break;
            }
            bytes += payload.len() + 1;
            events.push(serde_json::from_str::<UsageEvent>(&payload)?);
            attempts.push(n);
        }
        if events.is_empty() {
            continue;
        }
        let result = match d {
            Destination::Http { .. } => http_batch(d, &events),
            Destination::Postgres { .. } => pg_batch(d, &events),
        };
        let permanent = result.as_ref().err().is_some_and(|e| {
            matches!(
                e.to_string().as_str(),
                "permanent_export_error"
                    | "invalid_export_ack"
                    | "invalid_export_url"
                    | "empty_export_credential"
                    | "unsupported_remote_schema"
            )
        });
        let receipts = result.ok();
        // ACK must contain exactly one matching entry per submitted event. No partial guesswork.
        let valid = receipts.as_ref().is_some_and(|rs| {
            rs.len() == events.len()
                && events.iter().all(|e| {
                    rs.iter()
                        .filter(|r| {
                            r.event_id == e.event_id
                                && r.producer_id == e.producer_id
                                && e.bytes().is_ok_and(|b| digest(&b) == r.sha256)
                        })
                        .count()
                        == 1
                })
        });
        let tx = store.connection.transaction()?;
        for (i, e) in events.iter().enumerate() {
            let state = if permanent || (receipts.is_some() && !valid) {
                "blocked"
            } else if valid {
                match receipts
                    .as_ref()
                    .and_then(|rs| {
                        rs.iter()
                            .find(|r| r.event_id == e.event_id && r.producer_id == e.producer_id)
                    })
                    .map(|r| r.status)
                {
                    Some(ReceiptStatus::Committed | ReceiptStatus::Duplicate) => "committed",
                    Some(ReceiptStatus::Conflict) => "conflict",
                    _ => "blocked",
                }
            } else {
                "pending"
            };
            let n = attempts[i].saturating_add(1);
            let delay = 1000u64.saturating_mul(1u64 << n.min(8));
            tx.execute("UPDATE usage_outbox SET state=?1,attempts=?2,next_at_ms=?3 WHERE producer_id=?4 AND event_id=?5 AND destination=?6",params![state,n,now_ms().saturating_add(delay) as i64,e.producer_id,e.event_id,d.id()])?;
            if state == "committed" {
                sent += 1;
            }
        }
        tx.commit()?;
    }
    Ok(sent)
}
