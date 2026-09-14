//! Host-owned versioned recording; provider replies supply only numeric observations.
use super::*;
use crate::provider_plugins::{Binding, ProviderIdentity, ProviderObservation};
pub(crate) fn interpretation(identity: &ProviderIdentity) -> Interpretation {
    Interpretation {
        kind: "trusted_provider_plugin".into(),
        protocol: identity.protocol.clone(),
        provider_protocol: identity.provider_protocol.clone(),
        package_id: identity.id.clone(),
        package_version: identity.version.clone(),
        package_sha256: identity.package_sha256.clone(),
        executable_sha256: identity.executable_sha256.clone(),
    }
}
pub(crate) fn binding_interpretation(b: &Binding) -> Interpretation {
    interpretation(&ProviderIdentity {
        protocol: b.protocol.clone(),
        provider_protocol: b.provider_protocol.clone(),
        id: b.id.clone(),
        version: b.version.clone(),
        package_sha256: b.package_sha256.clone(),
        executable_sha256: b.executable_sha256.clone(),
    })
}
pub(crate) struct ProviderAttempt {
    sink: UsageSink,
    terminal: Option<mpsc::OwnedPermit<Delivery>>,
    pub event: UsageEventV2,
    finished: bool,
}
impl ProviderAttempt {
    pub async fn start(
        sink: Option<&UsageSink>,
        mut event: UsageEventV2,
    ) -> Result<Option<Self>, ()> {
        let Some(sink) = sink else { return Ok(None) };
        if sink.mode == Mode::Off {
            return Ok(None);
        }
        if !sink.supports_v2 {
            return Err(());
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
            event,
            finished: false,
        };
        a.deliver(false).await?;
        Ok(Some(a))
    }
    async fn deliver(&mut self, terminal: bool) -> Result<(), ()> {
        self.event.observed_at_ms = now().max(self.event.started_at_ms);
        let (tx, rx) = oneshot::channel();
        let delivery = Delivery {
            event: self.event.clone().into(),
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
    pub async fn observe(&mut self, observation: &ProviderObservation) -> Result<(), ()> {
        if interpretation(&observation.identity) != self.event.interpretation
            || !provider_usage_valid(&observation.usage)
        {
            return Err(());
        }
        if self.finished {
            return Ok(());
        }
        if self.event.usage != observation.usage {
            self.event.usage = observation.usage.clone();
            self.event.observation_incomplete |= !self.event.usage.violations.is_empty()
                || self
                    .event
                    .usage
                    .counters
                    .values()
                    .any(|c| c.source == Source::Invalid);
            self.event.finality = if self.event.usage.observed() {
                Finality::Partial
            } else {
                Finality::Unobserved
            };
            self.next(EventKind::UsageUpdated);
            self.deliver(false).await?;
        }
        Ok(())
    }
    pub async fn finish(&mut self, gateway: Outcome) -> Result<(), ()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.next(EventKind::AttemptFinished);
        self.event.gateway = gateway;
        self.event.finality = if !self.event.usage.observed() {
            Finality::Unobserved
        } else if !self.event.observation_incomplete
            && self.event.usage.violations.is_empty()
            && matches!(
                self.event.upstream,
                Outcome::Completed | Outcome::Incomplete
            )
        {
            Finality::Final
        } else {
            Finality::Partial
        };
        self.deliver(true).await
    }
}
impl Drop for ProviderAttempt {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.next(EventKind::AttemptFinished);
        if self.event.gateway == Outcome::InProgress {
            self.event.gateway = Outcome::Cancelled;
        }
        self.event.observation_incomplete = true;
        self.event.observed_at_ms = now().max(self.event.started_at_ms);
        self.event.finality = if self.event.usage.observed() {
            Finality::Partial
        } else {
            Finality::Unobserved
        };
        if let Some(permit) = self.terminal.take() {
            let (ack, _) = oneshot::channel();
            permit.send(Delivery {
                event: self.event.clone().into(),
                ack,
            });
        }
    }
}
pub(crate) async fn observe_provider(
    attempt: &mut Option<ProviderAttempt>,
    observation: Option<ProviderObservation>,
) -> Result<(), ()> {
    if let (Some(a), Some(o)) = (attempt, observation) {
        a.observe(&o).await?;
    }
    Ok(())
}
pub(crate) async fn finish_both(
    builtin: &mut Option<Attempt>,
    provider: &mut Option<ProviderAttempt>,
    outcome: Outcome,
) -> Result<(), ()> {
    finish(builtin, outcome).await?;
    if let Some(a) = provider {
        a.finish(outcome).await?;
    }
    Ok(())
}
