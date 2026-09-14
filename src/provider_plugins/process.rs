use super::{Binding, contract::*};
use crate::{
    extensions::framed_process::{Executable, FramedProcess},
    ir::IrError,
};

pub(super) struct Session {
    process: FramedProcess,
    sequence: u64,
    failed: bool,
    attempted_usage: Option<gateway_usage_contract::CanonicalUsage>,
}
impl Session {
    pub(super) async fn start(binding: &Binding) -> Result<Self, IrError> {
        let (process, ready) = FramedProcess::start(Executable {
            path: &binding.executable,
            directory: &binding.directory,
            sha256: &binding.executable_sha256,
            #[cfg(unix)]
            owner: binding.owner,
        })
        .await?;
        validate_ready(binding, ready)?;
        Ok(Self {
            process,
            sequence: 0,
            failed: false,
            attempted_usage: None,
        })
    }
    pub(super) fn attempted_usage(&self) -> Option<&gateway_usage_contract::CanonicalUsage> {
        self.attempted_usage.as_ref()
    }
    pub(super) fn poison(&mut self) {
        self.failed = true;
        self.process.poison();
    }
    pub(super) async fn call(&mut self, operation: Operation) -> Result<ResultValue, IrError> {
        if self.failed {
            return Err(IrError::InvalidEventOrder);
        }
        let result = async {
            self.sequence = self.sequence.checked_add(1).ok_or(IrError::SizeLimit)?;
            let bytes = serde_json::to_vec(&Request {
                protocol: gateway_plugin_contract::PROVIDER_PROTOCOL.into(),
                sequence: self.sequence,
                operation,
            })
            .map_err(|_| IrError::InvalidEventOrder)?;
            let raw = self.process.exchange(&bytes).await?;
            if raw["protocol"] != gateway_plugin_contract::PROVIDER_PROTOCOL
                || raw["sequence"].as_u64() != Some(self.sequence)
            {
                return Err(IrError::UnsupportedVersion);
            }
            let attempted = match raw["value"]["result"].as_str() {
                Some("progress") => Some(&raw["value"]["usage"]),
                Some("completed") => Some(&raw["value"]["value"]["usage"]),
                _ => None,
            };
            if let Some(attempted) = attempted {
                self.attempted_usage = Some(super::usage::observe_wire(attempted));
            }
            let reply: Reply =
                serde_json::from_value(raw).map_err(|_| IrError::UnsupportedVersion)?;
            if matches!(reply.value, ResultValue::Rejected { .. }) {
                return Err(IrError::UnsupportedFeature);
            }
            Ok(reply.value)
        }
        .await;
        if result.is_err() {
            self.poison();
        }
        result
    }
    #[cfg(all(test, unix))]
    pub(super) fn test_pair() -> (Self, std::os::unix::net::UnixStream) {
        let (process, peer) = FramedProcess::test_pair();
        (
            Self {
                process,
                sequence: 0,
                failed: false,
                attempted_usage: None,
            },
            peer,
        )
    }
}
pub(super) fn validate_ready(binding: &Binding, ready: serde_json::Value) -> Result<(), IrError> {
    let reply: Reply = serde_json::from_value(ready).map_err(|_| IrError::UnsupportedVersion)?;
    match reply.value {
        ResultValue::Ready {
            provider_protocol,
            capabilities,
        } if reply.protocol == binding.protocol
            && reply.protocol == gateway_plugin_contract::PROVIDER_PROTOCOL
            && reply.sequence == 0
            && provider_protocol == binding.provider_protocol
            && capabilities == binding.capabilities
            && capabilities.validate_for(&binding.protocol) =>
        {
            Ok(())
        }
        _ => Err(IrError::UnsupportedVersion),
    }
}
