//! Acknowledged, bounded recorder IPC. No SQL or downstream model-body queue.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]
use crate::{
    ConfigError,
    usage::{Delivery, UsageSink},
};
use gateway_usage_contract::{MAX_EVENT_BYTES, Mode, PROTOCOL, RecorderBinding};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct UsageWorker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}
impl Drop for UsageWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
#[cfg(unix)]
pub(super) fn spawn(
    executable: &std::path::Path,
    state: &std::path::Path,
    binding: &RecorderBinding,
) -> Result<(UsageSink, UsageWorker), ConfigError> {
    use std::{
        io::{BufRead, BufReader, Write},
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::{Command, Stdio},
    };
    fn fail() -> ConfigError {
        ConfigError("Usage recorder protocol failed".into())
    }
    fn read(
        reader: &mut BufReader<UnixStream>,
        deadline: Instant,
    ) -> Result<serde_json::Value, ConfigError> {
        let mut data = Vec::new();
        loop {
            reader
                .get_mut()
                .set_read_timeout(Some(
                    deadline
                        .checked_duration_since(Instant::now())
                        .ok_or_else(fail)?,
                ))
                .map_err(|_| fail())?;
            let b = reader.fill_buf().map_err(|_| fail())?;
            if b.is_empty() {
                return Err(fail());
            }
            let end = b.iter().position(|v| *v == b'\n');
            let n = end.map_or(b.len(), |i| i + 1);
            if data.len() + n > 4096 {
                return Err(fail());
            }
            data.extend_from_slice(&b[..n]);
            reader.consume(n);
            if end.is_some() {
                return serde_json::from_slice(&data).map_err(|_| fail());
            }
        }
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (parent, child) = UnixStream::pair().map_err(|_| fail())?;
    let input: OwnedFd = child.try_clone().map_err(|_| fail())?.into();
    let output: OwnedFd = child.into();
    let child = Child(
        Command::new(executable)
            .arg("serve")
            .env_clear()
            .current_dir(state)
            .stdin(Stdio::from(input))
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| fail())?,
    );
    let timeout = Duration::from_millis(binding.ack_timeout_ms);
    let mut reader = BufReader::new(parent);
    let ready = read(&mut reader, Instant::now() + timeout)?;
    let producer = ready
        .get("producer_id")
        .and_then(|v| v.as_str())
        .filter(|v| gateway_usage_contract::safe_label(v))
        .ok_or_else(fail)?
        .to_owned();
    if ready.get("type").and_then(|v| v.as_str()) != Some("ready")
        || ready.get("protocol").and_then(|v| v.as_str()) != Some(PROTOCOL)
        || ready.as_object().is_none_or(|m| m.len() != 3)
    {
        return Err(fail());
    }
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Delivery>(binding.queue_capacity);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let dropped = Arc::new(AtomicU64::new(0));
    let lost = dropped.clone();
    let handle=std::thread::Builder::new().name("usage-recorder".into()).spawn(move||{
        let _child=child;
        loop {
            let delivery=match receiver.try_recv(){Ok(v)=>v,Err(tokio::sync::mpsc::error::TryRecvError::Empty)=>{if stopped.load(Ordering::Relaxed){break}std::thread::sleep(Duration::from_millis(10));continue},Err(_)=>break};
            let result=(||->Result<(),ConfigError>{
                let mut bytes=delivery.event.bytes().map_err(|_|fail())?;
                if bytes.len()>MAX_EVENT_BYTES{return Err(fail())}bytes.push(b'\n');let deadline=Instant::now()+timeout;
                let mut remaining=bytes.as_slice();
                while !remaining.is_empty(){reader.get_mut().set_write_timeout(Some(deadline.checked_duration_since(Instant::now()).ok_or_else(fail)?)).map_err(|_|fail())?;let n=reader.get_mut().write(remaining).map_err(|_|fail())?;if n==0{return Err(fail())}remaining=&remaining[n..];}
                let ack=read(&mut reader,deadline)?;
                if ack!=serde_json::json!({"type":"committed","event_id":delivery.event.event_id,"sha256":gateway_usage_contract::digest(&bytes[..bytes.len()-1])}){return Err(fail())}Ok(())
            })();
            let ok=result.is_ok();let _=delivery.ack.send(ok);
            if !ok {lost.fetch_add(1,Ordering::Relaxed);tracing::warn!("usage_recorder_unavailable");break}
        }
    }).map_err(|_|fail())?;
    Ok((
        UsageSink {
            sender,
            mode: binding.mode,
            timeout,
            producer,
            dropped,
        },
        UsageWorker {
            stop,
            handle: Some(handle),
        },
    ))
}
#[allow(dead_code)]
fn _mode_contract(mode: Mode) -> bool {
    mode == Mode::DurableLocal
}
