//! Bounded native framing shared by roles; message semantics stay with each role.
use crate::ir::IrError;
use serde_json::Value;
use std::path::Path;

pub(crate) struct Executable<'a> {
    pub path: &'a Path,
    pub directory: &'a Path,
    pub sha256: &'a str,
    #[cfg(unix)]
    pub owner: u32,
}
#[cfg(unix)]
mod native {
    use super::*;
    use std::{
        os::fd::OwnedFd,
        process::{Child, Command, Stdio},
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    pub(crate) struct FramedProcess {
        child: Child,
        socket: tokio::net::UnixStream,
        failed: bool,
    }
    impl Drop for FramedProcess {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    impl FramedProcess {
        pub(crate) async fn start(executable: Executable<'_>) -> Result<(Self, Value), IrError> {
            let bytes = crate::extensions::filesystem::read(
                executable.path,
                gateway_plugin_contract::MAX_FRAME as u64,
                executable.owner,
            )
            .map_err(|_| IrError::UnsupportedVersion)?;
            if crate::continuation::hex(&crate::digest::sha256(&bytes)) != executable.sha256 {
                return Err(IrError::UnsupportedVersion);
            }
            crate::extensions::filesystem::private_dir(
                executable.directory,
                Some(executable.owner),
            )
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
            let child = Command::new(executable.path)
                .env_clear()
                .current_dir(executable.directory)
                .stdin(Stdio::from(input))
                .stdout(Stdio::from(output))
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| IrError::InvalidEventOrder)?;
            let mut process = Self {
                child,
                socket,
                failed: false,
            };
            let ready = tokio::time::timeout(Duration::from_secs(3), process.read_frame())
                .await
                .map_err(|_| IrError::InvalidEventOrder)??;
            Ok((process, ready))
        }
        async fn read_frame(&mut self) -> Result<Value, IrError> {
            let length = self
                .socket
                .read_u32()
                .await
                .map_err(|_| IrError::InvalidEventOrder)? as usize;
            if length == 0 || length > gateway_plugin_contract::MAX_FRAME {
                return Err(IrError::SizeLimit);
            }
            let mut bytes = vec![0; length];
            self.socket
                .read_exact(&mut bytes)
                .await
                .map_err(|_| IrError::InvalidEventOrder)?;
            crate::adapters::json::decode(&bytes)
        }
        pub(crate) fn poison(&mut self) {
            self.failed = true;
        }
        pub(crate) async fn exchange(&mut self, bytes: &[u8]) -> Result<Value, IrError> {
            if self.failed {
                return Err(IrError::InvalidEventOrder);
            }
            if bytes.is_empty() || bytes.len() > gateway_plugin_contract::MAX_FRAME {
                self.poison();
                return Err(IrError::SizeLimit);
            }
            let result = tokio::time::timeout(Duration::from_secs(3), async {
                self.socket
                    .write_u32(bytes.len() as u32)
                    .await
                    .map_err(|_| IrError::InvalidEventOrder)?;
                self.socket
                    .write_all(bytes)
                    .await
                    .map_err(|_| IrError::InvalidEventOrder)?;
                self.read_frame().await
            })
            .await
            .map_err(|_| IrError::InvalidEventOrder)
            .and_then(|v| v);
            if result.is_err() {
                self.poison();
            }
            result
        }
        #[cfg(test)]
        pub(crate) fn test_pair() -> (Self, std::os::unix::net::UnixStream) {
            let (left, right) = std::os::unix::net::UnixStream::pair().unwrap();
            left.set_nonblocking(true).unwrap();
            let child = Command::new("/bin/sleep")
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            (
                Self {
                    child,
                    socket: tokio::net::UnixStream::from_std(left).unwrap(),
                    failed: false,
                },
                right,
            )
        }
        #[cfg(test)]
        pub(crate) fn test_pid(&self) -> u32 {
            self.child.id()
        }
    }
}
#[cfg(unix)]
pub(crate) use native::FramedProcess;
#[cfg(not(unix))]
pub(crate) struct FramedProcess;
#[cfg(not(unix))]
impl FramedProcess {
    pub(crate) async fn start(executable: Executable<'_>) -> Result<(Self, Value), IrError> {
        let _ = (executable.path, executable.directory, executable.sha256);
        Err(IrError::UnsupportedFeature)
    }
    pub(crate) async fn exchange(&mut self, _: &[u8]) -> Result<Value, IrError> {
        Err(IrError::UnsupportedFeature)
    }
    pub(crate) fn poison(&mut self) {}
}
