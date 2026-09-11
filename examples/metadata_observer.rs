//! Reference observer: durable counts of HTTP header observations, never model success counts.
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, BufRead, Read, Write},
};

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    schema: String,
    process_id: u32,
    observed: u64,
    status_counts: BTreeMap<u16, u64>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Http {
        sequence: u64,
        status: u16,
        headers_ms: u64,
    },
}

fn persist(counts: &Counts) -> io::Result<()> {
    let temporary = format!(".counts-{}", uuid::Uuid::new_v4());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        serde_json::to_writer(&mut file, counts)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&temporary, "counts.json")?;
        #[cfg(unix)]
        std::fs::File::open(".")?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // This reference fixture additionally detects accidental parent-environment inheritance.
    if std::env::vars_os().next().is_some() {
        return Err("Unexpected environment inheritance".into());
    }
    let mut counts = match std::fs::File::open("counts.json") {
        Ok(file) => {
            let mut raw = Vec::new();
            file.take(65_537).read_to_end(&mut raw)?;
            if raw.len() > 65_536 {
                return Err("Observer state exceeds limit".into());
            }
            let counts: Counts = serde_json::from_slice(&raw)?;
            if counts.schema != "observer-state/v1" || counts.status_counts.len() > 500 {
                return Err("Observer state is incompatible".into());
            }
            counts
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Counts {
            schema: "observer-state/v1".into(),
            ..Counts::default()
        },
        Err(error) => return Err(error.into()),
    };
    counts.process_id = std::process::id();
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "{}",
        json!({"type":"ready", "protocol":"gateway-observer/v1"})
    )?;
    output.flush()?;
    let mut input = io::stdin().lock();
    let mut previous = 0_u64;
    loop {
        let mut raw = Vec::new();
        let size = input.by_ref().take(4097).read_until(b'\n', &mut raw)?;
        if size == 0 {
            return Ok(());
        }
        if size > 4096 || raw.last() != Some(&b'\n') {
            return Err("Invalid observer frame".into());
        }
        let Input::Http {
            sequence,
            status,
            headers_ms,
        } = serde_json::from_slice(&raw)?;
        if previous.checked_add(1) != Some(sequence) || !(100..=599).contains(&status) {
            return Err("Invalid observer sequence or status".into());
        }
        previous = sequence;
        // Timing describes header delivery, not stream completion; the example does not retain it.
        let _ = headers_ms;
        counts.observed = counts.observed.saturating_add(1);
        let value = counts.status_counts.entry(status).or_default();
        *value = value.saturating_add(1);
        persist(&counts)?;
        writeln!(output, "{}", json!({"type":"ack", "sequence":sequence}))?;
        output.flush()?;
    }
}

fn main() -> std::process::ExitCode {
    if run().is_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!("Observer failed");
        std::process::ExitCode::FAILURE
    }
}
