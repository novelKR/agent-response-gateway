//! Bounded best-effort metadata observers; never on the model transport path.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{SyncSender, TrySendError},
};

#[cfg(unix)]
use serde::Deserialize;
use serde::Serialize;

use super::ExtensionPlan;
#[cfg(unix)]
use super::OBSERVER_PROTOCOL;
use crate::ConfigError;

#[derive(Clone, Serialize)]
struct Observation {
    status: u16,
    headers_ms: u64,
}

/// Nonblocking delivery of fixed numeric metadata. Slow observers cannot delay HTTP responses.
#[derive(Clone, Default)]
pub struct ObserverSink {
    senders: Vec<SyncSender<Observation>>,
    dropped: Arc<AtomicU64>,
}

impl ObserverSink {
    pub fn observe_headers(&self, status: u16, headers_ms: u64) {
        if !(100..=599).contains(&status) {
            return;
        }
        for sender in &self.senders {
            match sender.try_send(Observation { status, headers_ms }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn dropped_observations(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Ready { protocol: String },
    Ack { sequence: u64 },
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Own for the listener's lifetime. Drop stops observers and reaps direct child processes.
pub struct ExtensionRuntime {
    workers: Vec<Worker>,
    usage_worker: Option<super::usage_runtime::UsageWorker>,
    usage_sink: Option<crate::usage::UsageSink>,
    sink: ObserverSink,
    _store_lock: Option<std::fs::File>,
}

impl ExtensionRuntime {
    #[cfg(unix)]
    pub fn start(plan: &ExtensionPlan) -> Result<Self, ConfigError> {
        let lock = super::filesystem::runtime_lock(&plan.root, plan.owner)?;
        let mut runtime = Self {
            workers: Vec::new(),
            usage_worker: None,
            usage_sink: None,
            sink: ObserverSink::default(),
            _store_lock: Some(lock),
        };
        for (entry, package) in plan.activation.extensions.iter().zip(&plan.packages) {
            if crate::codecs::contract::supported_protocol(&package.protocol) {
                continue;
            }
            let executable = plan
                .root
                .join("packages")
                .join(&entry.id)
                .join(&entry.version)
                .join(&entry.package_sha256)
                .join("extension");
            let state = plan
                .root
                .join("state")
                .join(&entry.id)
                .join(&entry.package_sha256);
            // Recheck immediately before spawn; installed content must not be mutated in place.
            if super::hash(&super::filesystem::read(
                &executable,
                super::MAX_BINARY,
                plan.owner,
            )?) != package.files["extension"]
            {
                return Err(super::invalid());
            }
            super::filesystem::private_dir(&state, Some(plan.owner))?;
            if package.protocol == gateway_usage_contract::PROTOCOL {
                let binding = plan
                    .activation
                    .recorder
                    .as_ref()
                    .ok_or_else(super::invalid)?;
                if binding.mode != gateway_usage_contract::Mode::Off {
                    let state = plan.root.join("usage").join(&binding.store_id);
                    filesystem_check_config(plan, binding, &state)?;
                    let (sink, worker) = super::usage_runtime::spawn(&executable, &state, binding)?;
                    runtime.usage_sink = Some(sink);
                    runtime.usage_worker = Some(worker);
                }
                continue;
            }
            let (sender, worker) = unix::spawn(&executable, &state)?;
            runtime.sink.senders.push(sender);
            runtime.workers.push(worker);
        }
        Ok(runtime)
    }

    #[cfg(not(unix))]
    pub fn start(_plan: &ExtensionPlan) -> Result<Self, ConfigError> {
        Err(ConfigError(
            "Native extension supervision is unsupported on this platform".into(),
        ))
    }

    pub fn usage_sink(&self) -> Option<crate::usage::UsageSink> {
        self.usage_sink.clone()
    }
    pub fn sink(&self) -> ObserverSink {
        self.sink.clone()
    }
}

impl Drop for ExtensionRuntime {
    fn drop(&mut self) {
        // Signal all workers first; joins do not serialize the per-operation deadlines.
        for worker in &self.workers {
            worker.stop.store(true, Ordering::Relaxed);
        }
        self.workers.clear();
        self.usage_worker.take();
        if let Some(sink) = &self.usage_sink {
            tracing::info!(
                dropped_usage_events = sink.dropped_events(),
                "usage_recorder_stopped"
            );
        }
        tracing::info!(
            dropped_observations = self.sink.dropped_observations(),
            "extension_observers_stopped"
        );
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use serde_json::json;
    use std::{
        io::{self, BufRead, BufReader, Write},
        os::{fd::OwnedFd, unix::net::UnixStream},
        path::Path,
        process::{Child, Command, Stdio},
        sync::mpsc::{self, RecvTimeoutError},
        time::{Duration, Instant},
    };

    const FRAME_LIMIT: usize = 4096;
    const QUEUE_LIMIT: usize = 64;
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
    const EVENT_TIMEOUT: Duration = Duration::from_secs(1);

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn remaining(deadline: Instant) -> io::Result<Duration> {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
            .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))
    }

    fn read_frame(reader: &mut BufReader<UnixStream>, deadline: Instant) -> io::Result<Reply> {
        let mut frame = Vec::new();
        loop {
            reader
                .get_ref()
                .set_read_timeout(Some(remaining(deadline)?))?;
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let newline = available.iter().position(|b| *b == b'\n');
            let length = newline.map_or(available.len(), |index| index + 1);
            if length > FRAME_LIMIT.saturating_sub(frame.len()) {
                return Err(io::ErrorKind::InvalidData.into());
            }
            frame.extend_from_slice(&available[..length]);
            reader.consume(length);
            if newline.is_some() {
                return serde_json::from_slice(&frame)
                    .map_err(|_| io::ErrorKind::InvalidData.into());
            }
        }
    }

    fn write_frame(stream: &mut UnixStream, raw: &[u8], deadline: Instant) -> io::Result<()> {
        let mut pending = raw;
        while !pending.is_empty() {
            stream.set_write_timeout(Some(remaining(deadline)?))?;
            let written = stream.write(pending)?;
            if written == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            pending = &pending[written..];
        }
        Ok(())
    }

    pub(super) fn spawn(
        executable: &Path,
        state: &Path,
    ) -> Result<(SyncSender<Observation>, Worker), ConfigError> {
        let (parent, child_socket) = UnixStream::pair().map_err(|_| super::super::invalid())?;
        let child_input: OwnedFd = child_socket
            .try_clone()
            .map_err(|_| super::super::invalid())?
            .into();
        let child_output: OwnedFd = child_socket.into();
        // Socket-backed stdio permits deadlines on reads AND writes without unbounded IO threads.
        let child = Command::new(executable)
            .env_clear()
            .current_dir(state)
            .stdin(Stdio::from(child_input))
            .stdout(Stdio::from(child_output))
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ConfigError("Cannot start verified observer executable".into()))?;
        let guard = ChildGuard(child);
        let mut reader = BufReader::new(parent);
        match read_frame(&mut reader, Instant::now() + STARTUP_TIMEOUT) {
            Ok(Reply::Ready { protocol }) if protocol == OBSERVER_PROTOCOL => {}
            _ => {
                return Err(ConfigError(
                    "Observer startup protocol failed or timed out".into(),
                ));
            }
        }
        let (sender, receiver) = mpsc::sync_channel::<Observation>(QUEUE_LIMIT);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handle = std::thread::Builder::new().name("gateway-observer".into()).spawn(move || {
            let _guard = guard;
            let mut sequence = 0_u64;
            while !stopped.load(Ordering::Relaxed) {
                let event = match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                let Some(next) = sequence.checked_add(1) else { break };
                sequence = next;
                let mut raw = serde_json::to_vec(&json!({"type":"http", "sequence":sequence,
                    "status":event.status, "headers_ms":event.headers_ms})).expect("numeric metadata JSON");
                raw.push(b'\n');
                let deadline = Instant::now() + EVENT_TIMEOUT;
                let result = write_frame(reader.get_mut(), &raw, deadline)
                    .and_then(|()| read_frame(&mut reader, deadline));
                if !matches!(result, Ok(Reply::Ack { sequence: observed }) if observed == sequence) {
                    tracing::warn!("extension_observer_protocol_failed");
                    break;
                }
            }
            // Neither queued observations nor failed calls are replayed or retried.
        }).map_err(|_| ConfigError("Cannot start observer supervisor".into()))?;
        Ok((
            sender,
            Worker {
                stop,
                handle: Some(handle),
            },
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn frame_parser_bounds_and_rejects_callbacks() {
            let (left, mut right) = UnixStream::pair().unwrap();
            right
                .write_all(b"{\"type\":\"get_credentials\"}\n")
                .unwrap();
            assert!(read_frame(&mut BufReader::new(left), Instant::now() + EVENT_TIMEOUT).is_err());
            let (left, mut right) = UnixStream::pair().unwrap();
            right.write_all(&vec![b'x'; FRAME_LIMIT + 1]).unwrap();
            assert!(read_frame(&mut BufReader::new(left), Instant::now() + EVENT_TIMEOUT).is_err());
        }

        #[test]
        fn frame_deadline_includes_partial_messages() {
            let (left, _right) = UnixStream::pair().unwrap();
            let started = Instant::now();
            assert!(
                read_frame(
                    &mut BufReader::new(left),
                    started + Duration::from_millis(30)
                )
                .is_err()
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }
}

#[cfg(unix)]
fn filesystem_check_config(
    plan: &ExtensionPlan,
    binding: &gateway_usage_contract::RecorderBinding,
    state: &std::path::Path,
) -> Result<(), ConfigError> {
    super::filesystem::private_dir(state, Some(plan.owner))?;
    if super::hash(&super::filesystem::read(
        &state.join("recorder.json"),
        65_536,
        plan.owner,
    )?) != binding.config_sha256
    {
        return Err(super::invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_is_bounded_and_contains_only_numeric_metadata() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let sink = ObserverSink {
            senders: vec![sender],
            ..ObserverSink::default()
        };
        sink.observe_headers(200, 3);
        sink.observe_headers(503, 5);
        sink.observe_headers(999, 7);
        assert_eq!(sink.dropped_observations(), 1);
        let event = serde_json::to_value(receiver.recv().unwrap()).unwrap();
        assert_eq!(event, serde_json::json!({"status":200,"headers_ms":3}));
    }
}
