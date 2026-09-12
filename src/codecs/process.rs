//! Async, bounded request-scoped IPC. Dropping the session kills and reaps its child.
use super::{Binding, contract::*};
use crate::ir::IrError;

#[cfg(unix)]
mod native {
    use super::*;
    use std::{
        os::fd::OwnedFd,
        process::{Child, Command, Stdio},
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    pub(crate) struct Session {
        child: Child,
        socket: tokio::net::UnixStream,
        sequence: u64,
        failed: bool,
        protocol: String,
    }
    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    impl Session {
        pub(crate) async fn start(binding: &Binding) -> Result<Self, IrError> {
            let bytes = crate::extensions::filesystem::read(
                &binding.executable,
                128 * 1024 * 1024,
                binding.owner,
            )
            .map_err(|_| IrError::UnsupportedVersion)?;
            if crate::continuation::hex(&crate::digest::sha256(&bytes)) != binding.executable_sha256
            {
                return Err(IrError::UnsupportedVersion);
            }
            crate::extensions::filesystem::private_dir(&binding.directory, Some(binding.owner))
                .map_err(|_| IrError::UnsupportedVersion)?;
            let (parent, child) =
                std::os::unix::net::UnixStream::pair().map_err(|_| IrError::InvalidEventOrder)?;
            parent
                .set_nonblocking(true)
                .map_err(|_| IrError::InvalidEventOrder)?;
            let input: OwnedFd = child
                .try_clone()
                .map_err(|_| IrError::InvalidEventOrder)?
                .into();
            let output: OwnedFd = child.into();
            let socket =
                tokio::net::UnixStream::from_std(parent).map_err(|_| IrError::InvalidEventOrder)?;
            let child = Command::new(&binding.executable)
                .env_clear()
                .current_dir(&binding.directory)
                .stdin(Stdio::from(input))
                .stdout(Stdio::from(output))
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| IrError::InvalidEventOrder)?;
            let mut session = Self {
                child,
                socket,
                sequence: 0,
                failed: false,
                protocol: binding.protocol.clone(),
            };
            let reply = tokio::time::timeout(Duration::from_secs(3), session.read())
                .await
                .map_err(|_| IrError::InvalidEventOrder)??;
            match reply {
                ResultValue::Ready {
                    apis,
                    replay_versions,
                } if apis
                    == vec![
                        crate::ir::ApiProtocol::Responses,
                        crate::ir::ApiProtocol::Messages,
                        crate::ir::ApiProtocol::ChatCompletions,
                        crate::ir::ApiProtocol::GeminiInteractions,
                    ]
                    && replay_versions == [1] =>
                {
                    Ok(session)
                }
                _ => Err(IrError::UnsupportedVersion),
            }
        }
        async fn read(&mut self) -> Result<ResultValue, IrError> {
            let length = self
                .socket
                .read_u32()
                .await
                .map_err(|_| IrError::InvalidEventOrder)? as usize;
            if length == 0 || length > MAX_FRAME {
                return Err(IrError::SizeLimit);
            }
            let mut bytes = vec![0; length];
            self.socket
                .read_exact(&mut bytes)
                .await
                .map_err(|_| IrError::InvalidEventOrder)?;
            let reply: Reply = serde_json::from_value(crate::adapters::json::decode(&bytes)?)
                .map_err(|_| IrError::UnsupportedVersion)?;
            if reply.protocol != self.protocol || reply.sequence != self.sequence {
                return Err(IrError::UnsupportedVersion);
            }
            Ok(reply.value)
        }
        pub(crate) async fn call(&mut self, operation: Operation) -> Result<ResultValue, IrError> {
            if self.failed {
                return Err(IrError::InvalidEventOrder);
            }
            self.sequence = self.sequence.checked_add(1).ok_or(IrError::SizeLimit)?;
            let bytes = serde_json::to_vec(&Request {
                protocol: self.protocol.clone(),
                sequence: self.sequence,
                operation,
            })
            .map_err(|_| IrError::InvalidEventOrder)?;
            if bytes.len() > MAX_FRAME {
                self.failed = true;
                return Err(IrError::SizeLimit);
            }
            let result = tokio::time::timeout(Duration::from_secs(3), async {
                self.socket
                    .write_u32(bytes.len() as u32)
                    .await
                    .map_err(|_| IrError::InvalidEventOrder)?;
                self.socket
                    .write_all(&bytes)
                    .await
                    .map_err(|_| IrError::InvalidEventOrder)?;
                self.read().await
            })
            .await
            .map_err(|_| IrError::InvalidEventOrder)
            .and_then(|v| v);
            if result.is_err() || matches!(result, Ok(ResultValue::Rejected)) {
                self.failed = true;
            }
            result
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};
        fn pair() -> (Session, std::os::unix::net::UnixStream) {
            let (left, right) = std::os::unix::net::UnixStream::pair().unwrap();
            left.set_nonblocking(true).unwrap();
            let child = Command::new("/bin/sleep")
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            (
                Session {
                    child,
                    socket: tokio::net::UnixStream::from_std(left).unwrap(),
                    sequence: 0,
                    failed: false,
                    protocol: PROTOCOL.into(),
                },
                right,
            )
        }
        #[tokio::test]
        async fn malformed_replies_poison_request_and_child_is_reaped() {
            for response in [
                b"{\"protocol\":\"gateway-api-codec/v2\",\"sequence\":1,\"value\":{\"result\":\"finished\"}}".to_vec(),
                b"{\"protocol\":\"gateway-api-codec/v1\",\"sequence\":2,\"value\":{\"result\":\"finished\"}}".to_vec(),
                b"{\"protocol\":\"gateway-api-codec/v1\",\"sequence\":1,\"sequence\":1,\"value\":{\"result\":\"finished\"}}".to_vec(),
                b"{\"protocol\":\"gateway-api-codec/v1\",\"sequence\":1,\"value\":{\"result\":\"get_credentials\"}}".to_vec(),
            ] {
                let (mut session,mut peer)=pair();let pid=session.child.id();
                let thread=std::thread::spawn(move||{let mut length=[0;4];peer.read_exact(&mut length).unwrap();let mut input=vec![0;u32::from_be_bytes(length) as usize];peer.read_exact(&mut input).unwrap();peer.write_all(&(response.len() as u32).to_be_bytes()).unwrap();peer.write_all(&response).unwrap();});
                assert!(session.call(Operation::Finish).await.is_err());assert!(session.failed);assert!(session.call(Operation::Finish).await.is_err());thread.join().unwrap();drop(session);
                assert!(!Command::new("/bin/kill").args(["-0",&pid.to_string()]).stderr(Stdio::null()).status().unwrap().success());
            }
        }
        #[tokio::test]
        async fn frames_have_size_eof_and_total_deadline_bounds() {
            let (mut session, mut peer) = pair();
            peer.write_all(&((MAX_FRAME + 1) as u32).to_be_bytes())
                .unwrap();
            assert!(session.call(Operation::Finish).await.is_err());
            drop(session);
            let (mut session, mut peer) = pair();
            peer.write_all(&20u32.to_be_bytes()).unwrap();
            peer.write_all(b"{}").unwrap();
            peer.shutdown(std::net::Shutdown::Write).unwrap();
            assert!(session.call(Operation::Finish).await.is_err());
            drop(session);
            let (mut session, _peer) = pair();
            let started = std::time::Instant::now();
            assert!(session.call(Operation::Finish).await.is_err());
            assert!(started.elapsed() < Duration::from_secs(5));
        }
    }
}
#[cfg(unix)]
pub(crate) use native::Session;
#[cfg(not(unix))]
pub(crate) struct Session;
#[cfg(not(unix))]
impl Session {
    pub(crate) async fn start(_: &Binding) -> Result<Self, IrError> {
        Err(IrError::UnsupportedFeature)
    }
    pub(crate) async fn call(&mut self, _: Operation) -> Result<ResultValue, IrError> {
        Err(IrError::UnsupportedFeature)
    }
}
