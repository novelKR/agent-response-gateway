use clap::{Parser, Subcommand};
use gateway_usage_contract::{MAX_EVENT_BYTES, PROTOCOL, ReceiptStatus, UsageEvent, digest};
use gateway_usage_recorder::{Result, Store, config, export, now_ms, query};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[derive(Parser)]
#[command(version, about = "Local usage ledger and explicit event export")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        store: PathBuf,
    },
    Serve,
    Status {
        #[arg(long)]
        store: PathBuf,
    },
    Query {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        attempt: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Export {
        #[arg(long)]
        store: PathBuf,
        #[arg(long, default_value_t = 0)]
        after_rowid: i64,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Aggregate {
        #[arg(long)]
        store: PathBuf,
        #[arg(long, default_value_t = 0)]
        from_ms: u64,
        #[arg(long)]
        to_ms: Option<u64>,
        #[arg(long, default_value = "UTC")]
        timezone: String,
    },
    Backup {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Migrate {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        backup: PathBuf,
    },
    Prune {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        before_ms: u64,
    },
    RetryBlocked {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        destination: String,
    },
    InitializePostgres {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        destination: String,
    },
    Flush {
        #[arg(long)]
        store: PathBuf,
    },
}
fn serve() -> Result<()> {
    let path = std::env::current_dir()?;
    let c = config(&path.join("recorder.json"))?;
    let mut store = Store::open(&path, false, true)?;
    store.bind_destinations(&c)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let p = path.clone();
    let exports = c.clone();
    // The export thread has its own connection. Network waits never hold the IPC writer.
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut store = Store::open(&p, false, false)?;
            store.connection = rusqlite::Connection::open(p.join("usage.sqlite3"))?;
            store.connection.busy_timeout(Duration::from_secs(3))?;
            while !stop.load(Ordering::Relaxed) {
                let _ = export::run_once(&mut store, &exports);
                for _ in 0..10 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(())
        })();
        if result.is_err() {
            eprintln!("usage_export_worker_failed");
        }
    });
    let result = (|| -> Result<()> {
        println!(
            "{}",
            serde_json::json!({"type":"ready","protocol":PROTOCOL,"producer_id":store.producer()?})
        );
        io::stdout().flush()?;
        let mut input = io::stdin().lock();
        loop {
            let mut raw = vec![];
            loop {
                let b = input.fill_buf()?;
                if b.is_empty() {
                    if raw.is_empty() {
                        return Ok(());
                    }
                    return Err("truncated_event".into());
                }
                let end = b.iter().position(|b| *b == b'\n');
                let n = end.map_or(b.len(), |n| n + 1);
                if raw.len() + n > MAX_EVENT_BYTES + 1 {
                    return Err("event_too_large".into());
                }
                raw.extend_from_slice(&b[..n]);
                input.consume(n);
                if end.is_some() {
                    break;
                }
            }
            raw.pop();
            let event: UsageEvent = serde_json::from_slice(&raw)?;
            if event.producer_id != store.producer()? || event.bytes()? != raw {
                return Err("invalid_event_identity".into());
            }
            if !matches!(
                store.record(&event, &c)?,
                ReceiptStatus::Committed | ReceiptStatus::Duplicate
            ) {
                return Err("event_conflict".into());
            }
            println!(
                "{}",
                serde_json::json!({"type":"committed","event_id":event.event_id,"sha256":digest(&raw)})
            );
            io::stdout().flush()?;
        }
    })();
    stopped.store(true, Ordering::Relaxed);
    let _ = worker.join();
    result
}
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init { store } => {
            let s = Store::open(&store, true, true)?;
            println!("{}", query::status(&s)?);
        }
        Command::Serve => serve()?,
        Command::Status { store } => {
            println!("{}", query::status(&Store::open(&store, false, false)?)?)
        }
        Command::Aggregate {
            store,
            from_ms,
            to_ms,
            timezone,
        } => println!(
            "{}",
            query::aggregate(
                &Store::open(&store, false, false)?,
                from_ms,
                to_ms.unwrap_or_else(now_ms),
                &timezone
            )?
        ),
        Command::Query {
            store,
            attempt,
            limit,
        } => {
            let s = Store::open(&store, false, false)?;
            let mut stmt=s.connection.prepare("SELECT payload FROM usage_current WHERE (?1 IS NULL OR attempt_id=?1) ORDER BY started_at_ms DESC LIMIT ?2")?;
            for row in stmt.query_map(rusqlite::params![attempt, limit.min(1000) as i64], |r| {
                r.get::<_, String>(0)
            })? {
                let event: UsageEvent = serde_json::from_str(&row?)?;
                println!(
                    "{}",
                    serde_json::json!({"event":event,"non_read_input_tokens":event.usage.non_read_input_tokens()})
                );
            }
        }
        Command::Export {
            store,
            after_rowid,
            limit,
        } => {
            let s = Store::open(&store, false, false)?;
            let mut stmt = s.connection.prepare(
                "SELECT rowid,payload FROM usage_events WHERE rowid>?1 ORDER BY rowid LIMIT ?2",
            )?;
            for row in stmt.query_map(
                rusqlite::params![after_rowid, limit.min(1000) as i64],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )? {
                let (cursor, payload) = row?;
                println!(
                    "{}",
                    serde_json::json!({"cursor":cursor,"event":serde_json::from_str::<serde_json::Value>(&payload)?})
                );
            }
        }
        Command::Backup { store, output }
        | Command::Migrate {
            store,
            backup: output,
        } => {
            let s = Store::open(&store, false, true)?;
            s.backup(&output)?;
            println!("{{\"storage_schema\":1,\"backup_completed\":true}}");
        }
        Command::Prune { store, before_ms } => {
            let mut s = Store::open(&store, false, true)?;
            let tx = s.connection.transaction()?;
            tx.execute("DELETE FROM usage_outbox WHERE state='committed' AND EXISTS(SELECT 1 FROM usage_current c WHERE c.producer_id=usage_outbox.producer_id AND c.kind='attempt_finished' AND c.observed_at_ms<?1 AND c.attempt_id=(SELECT attempt_id FROM usage_events e WHERE e.producer_id=usage_outbox.producer_id AND e.event_id=usage_outbox.event_id) AND NOT EXISTS(SELECT 1 FROM usage_outbox o JOIN usage_events e USING(producer_id,event_id) WHERE e.producer_id=c.producer_id AND e.attempt_id=c.attempt_id AND o.state!='committed'))",[i64::try_from(before_ms)?])?;
            tx.execute("INSERT INTO usage_tombstones SELECT producer_id,event_id,attempt_id,revision,sha256,kind FROM usage_events WHERE (producer_id,attempt_id) IN (SELECT producer_id,attempt_id FROM usage_current c WHERE c.kind='attempt_finished' AND c.observed_at_ms<?1 AND NOT EXISTS(SELECT 1 FROM usage_outbox o JOIN usage_events e USING(producer_id,event_id) WHERE e.producer_id=c.producer_id AND e.attempt_id=c.attempt_id))", [i64::try_from(before_ms)?])?;
            let n=tx.execute("DELETE FROM usage_events WHERE (producer_id,attempt_id) IN (SELECT producer_id,attempt_id FROM usage_current c WHERE c.kind='attempt_finished' AND c.observed_at_ms<?1 AND NOT EXISTS(SELECT 1 FROM usage_outbox o JOIN usage_events e USING(producer_id,event_id) WHERE e.producer_id=c.producer_id AND e.attempt_id=c.attempt_id))",[i64::try_from(before_ms)?])?;
            // Identity rows intentionally survive retention, preventing identity reuse.
            tx.commit()?;
            println!("{{\"deleted_events\":{n}}}");
        }
        Command::RetryBlocked { store, destination } => {
            let s = Store::open(&store, false, true)?;
            let n=s.connection.execute("UPDATE usage_outbox SET state='pending',next_at_ms=0 WHERE destination=?1 AND state='blocked'",[destination])?;
            println!("{{\"retry_events\":{n}}}");
        }
        Command::InitializePostgres { store, destination } => {
            let c = config(&store.join("recorder.json"))?;
            let d = c
                .destinations
                .iter()
                .find(|d| d.id() == destination)
                .ok_or("unknown_destination")?;
            export::initialize_postgres(d)?;
            println!("{{\"remote_schema\":1}}");
        }
        Command::Flush { store } => {
            let c = config(&store.join("recorder.json"))?;
            let mut s = Store::open(&store, false, true)?;
            s.bind_destinations(&c)?;
            let n = export::run_once(&mut s, &c)?;
            println!("{{\"committed_events\":{n}}}");
        }
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run(Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => {
            eprintln!(
                "Usage recorder operation failed; check configuration, storage and delivery status"
            );
            std::process::ExitCode::FAILURE
        }
    }
}
