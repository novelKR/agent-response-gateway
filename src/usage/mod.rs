//! Accounting follows upstream attempts; it never owns model retries or storage.
pub use gateway_usage_contract::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};

pub(crate) struct Delivery {
    pub event: UsageEvent,
    pub ack: oneshot::Sender<bool>,
}
#[derive(Clone)]
pub struct UsageSink {
    pub(crate) sender: mpsc::Sender<Delivery>,
    pub(crate) mode: Mode,
    pub(crate) timeout: Duration,
    pub(crate) producer: String,
    pub(crate) dropped: Arc<AtomicU64>,
}
impl UsageSink {
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub(crate) struct Attempt {
    sink: UsageSink,
    terminal: Option<mpsc::OwnedPermit<Delivery>>,
    pub event: UsageEvent,
    pub accumulator: Accumulator,
    finished: bool,
}
impl Attempt {
    pub async fn start(
        sink: Option<&UsageSink>,
        mut event: UsageEvent,
    ) -> Result<Option<Self>, ()> {
        let Some(sink) = sink else { return Ok(None) };
        if sink.mode == Mode::Off {
            return Ok(None);
        }
        let terminal = if sink.mode == Mode::DurableLocal {
            Some(
                tokio::time::timeout(sink.timeout, sink.sender.clone().reserve_owned())
                    .await
                    .map_err(|_| ())?
                    .map_err(|_| ())?,
            )
        } else {
            match sink.sender.clone().try_reserve_owned() {
                Ok(p) => Some(p),
                Err(_) => {
                    sink.dropped.fetch_add(1, Ordering::Relaxed);
                    return Ok(None);
                }
            }
        };
        event.producer_id = sink.producer.clone();
        let mut a = Self {
            sink: sink.clone(),
            terminal,
            accumulator: Accumulator::new(event.profile),
            event,
            finished: false,
        };
        a.deliver(false).await?;
        Ok(Some(a))
    }
    async fn deliver(&mut self, terminal: bool) -> Result<(), ()> {
        self.event.observed_at_ms = now().max(self.event.started_at_ms);
        self.event.usage = self.accumulator.usage.clone();
        self.event.observation_incomplete |= self.accumulator.incomplete;
        let (tx, rx) = oneshot::channel();
        let delivery = Delivery {
            event: self.event.clone(),
            ack: tx,
        };
        if terminal {
            self.terminal.take().ok_or(())?.send(delivery);
        } else if self.sink.mode == Mode::DurableLocal {
            tokio::time::timeout(self.sink.timeout, self.sink.sender.send(delivery))
                .await
                .map_err(|_| ())?
                .map_err(|_| ())?;
        } else if self.sink.sender.try_send(delivery).is_err() {
            self.sink.dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        if self.sink.mode == Mode::DurableLocal
            && !matches!(
                tokio::time::timeout(self.sink.timeout, rx).await,
                Ok(Ok(true))
            )
        {
            return Err(());
        }
        Ok(())
    }
    fn next(&mut self, kind: EventKind) {
        self.event.revision += 1;
        self.event.event_id = uuid::Uuid::new_v4().to_string();
        self.event.kind = kind;
    }
    pub async fn observe(&mut self, value: &serde_json::Value) -> Result<(), ()> {
        if self.finished {
            return Ok(());
        }
        if self.accumulator.observe(value) {
            self.next(EventKind::UsageUpdated);
            self.event.finality = if self.accumulator.usage.observed() {
                Finality::Partial
            } else {
                Finality::Unobserved
            };
            self.deliver(false).await?;
        }
        Ok(())
    }
    pub fn incomplete(&mut self) {
        self.accumulator.incomplete = true;
        self.event.observation_incomplete = true;
    }
    pub async fn finish(&mut self, gateway: Outcome) -> Result<(), ()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.next(EventKind::AttemptFinished);
        self.event.gateway = gateway;
        self.event.finality = if !self.accumulator.usage.observed() {
            Finality::Unobserved
        } else if !self.accumulator.incomplete
            && matches!(
                self.event.upstream,
                Outcome::Completed | Outcome::Incomplete
            )
            && self.accumulator.usage.violations.is_empty()
        {
            Finality::Final
        } else {
            Finality::Partial
        };
        self.deliver(true).await
    }
    /// An observed terminal is not evidence that all bytes reached the client.
    pub fn payload_metadata(&mut self, value: &serde_json::Value) {
        let source = value
            .get("response")
            .or_else(|| value.get("message"))
            .unwrap_or(value);
        for (key, target) in [
            ("model", &mut self.event.reported_model),
            ("id", &mut self.event.provider_response_id),
        ] {
            if let Some(v) = source
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|v| safe_label(v))
            {
                *target = Some(v.into());
            }
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.next(EventKind::AttemptFinished);
        if self.event.gateway == Outcome::InProgress {
            self.event.gateway = Outcome::Cancelled;
        }
        self.event.observed_at_ms = now().max(self.event.started_at_ms);
        self.event.usage = self.accumulator.usage.clone();
        self.event.observation_incomplete |= self.accumulator.incomplete;
        self.event.finality = if self.accumulator.usage.observed() {
            Finality::Partial
        } else {
            Finality::Unobserved
        };
        if let Some(permit) = self.terminal.take() {
            let (ack, _) = oneshot::channel();
            permit.send(Delivery {
                event: self.event.clone(),
                ack,
            });
        }
    }
}
/// Called on original parsed upstream events before conversion; no model content retained.
pub(crate) async fn observe_payload(
    attempt: &mut Option<Attempt>,
    value: &serde_json::Value,
) -> Result<(), ()> {
    let Some(a) = attempt else { return Ok(()) };
    a.payload_metadata(value);
    let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let source = value
        .get("response")
        .or_else(|| value.get("message"))
        .unwrap_or(value);
    if let Some(u) = source.get("usage").filter(|u| !u.is_null()) {
        a.observe(u).await?;
    }
    match kind {
        "response.completed" | "message_stop" => a.event.upstream = Outcome::Completed,
        "response.incomplete" => a.event.upstream = Outcome::Incomplete,
        "response.failed" | "error" => a.event.upstream = Outcome::Failed,
        _ => {}
    }
    Ok(())
}
pub(crate) async fn finish(attempt: &mut Option<Attempt>, outcome: Outcome) -> Result<(), ()> {
    if let Some(a) = attempt {
        a.finish(outcome).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
