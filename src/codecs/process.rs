//! Async, bounded request-scoped IPC. Dropping the session kills and reaps its child.
use super::{Binding, conversion::*};
use crate::ir::IrError;

#[cfg(any(unix, test))]
fn validate_ready(binding: &Binding, value: serde_json::Value) -> Result<(), IrError> {
    if binding.protocol == gateway_plugin_contract::CAPABILITIES_PROTOCOL {
        let reply: gateway_plugin_contract::CapabilitiesReply =
            serde_json::from_value(value).map_err(|_| IrError::UnsupportedVersion)?;
        let gateway_plugin_contract::CapabilitiesResult::Ready { capabilities } = reply.value;
        if reply.protocol != binding.protocol
            || reply.sequence != 0
            || !capabilities.validate_for(&binding.protocol)
            || binding.capabilities.as_ref() != Some(&capabilities)
        {
            return Err(IrError::UnsupportedVersion);
        }
    } else {
        let reply = decode_reply(value)?;
        if reply.protocol != binding.protocol
            || reply.sequence != 0
            || binding.capabilities.is_some()
        {
            return Err(IrError::UnsupportedVersion);
        }
        match reply.value {
            ResultValue::Ready {
                apis,
                replay_versions,
            } if apis
                == [
                    crate::ir::ApiProtocol::Responses,
                    crate::ir::ApiProtocol::Messages,
                    crate::ir::ApiProtocol::ChatCompletions,
                    crate::ir::ApiProtocol::GeminiInteractions,
                ]
                && replay_versions == [1] => {}
            _ => return Err(IrError::UnsupportedVersion),
        }
    }
    Ok(())
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
            let value = tokio::time::timeout(Duration::from_secs(3), session.read_frame())
                .await
                .map_err(|_| IrError::InvalidEventOrder)??;
            super::validate_ready(binding, value)?;
            Ok(session)
        }
        async fn read_frame(&mut self) -> Result<serde_json::Value, IrError> {
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
            crate::adapters::json::decode(&bytes)
        }
        async fn read(&mut self) -> Result<ResultValue, IrError> {
            let reply = decode_reply(self.read_frame().await?)?;
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
            let bytes = encode_request(&Request {
                protocol: self.protocol.clone(),
                sequence: self.sequence,
                operation,
            })?;
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

#[cfg(test)]
mod capability_tests {
    use super::*;
    use gateway_plugin_contract::{
        CAPABILITIES_PROTOCOL, Capabilities, EDITING_PROTOCOL, PROTOCOL,
    };
    use serde_json::json;

    fn binding(protocol: &str) -> Binding {
        Binding {
            protocol: protocol.into(),
            capabilities: (protocol == CAPABILITIES_PROTOCOL).then(|| {
                serde_json::from_value::<Capabilities>(json!({
                    "schema":"gateway-plugin-capabilities/v1", "apis":["responses"],
                    "features":["json"], "requires":["codec_ipc_v3","responses_output_validation"]
                }))
                .unwrap()
            }),
            id: "synthetic".into(),
            version: "1.0.0".into(),
            package_sha256: "a".repeat(64),
            executable_sha256: "b".repeat(64),
            executable: "/nonexistent".into(),
            directory: "/nonexistent".into(),
            #[cfg(unix)]
            owner: 0,
        }
    }

    #[test]
    fn ready_requires_exact_declared_capabilities_and_protocol() {
        let binding = binding(CAPABILITIES_PROTOCOL);
        let ready = json!({"protocol":CAPABILITIES_PROTOCOL,"sequence":0,"value":{
            "result":"ready","capabilities":binding.capabilities}});
        assert!(validate_ready(&binding, ready.clone()).is_ok());
        for (pointer, value) in [
            ("/sequence", json!(1)),
            ("/protocol", json!(EDITING_PROTOCOL)),
            ("/value/capabilities/apis", json!(["messages"])),
            ("/value/capabilities/features", json!(["json", "streaming"])),
            ("/value/capabilities/requires", json!(["codec_ipc_v3"])),
        ] {
            let mut invalid = ready.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_ready(&binding, invalid).is_err(), "{pointer}");
        }
        let mut invalid = ready;
        invalid["value"]["capabilities"]["extra"] = json!(true);
        assert!(validate_ready(&binding, invalid).is_err());
    }

    #[tokio::test]
    async fn unsupported_request_features_fail_before_executable_access() {
        use crate::ir::{
            capability::plan_translation,
            continuity::{ContinuityBinding, VerifiedProviderHistory},
            responses,
        };
        let mut request =
            responses::decode(json!({"model":"synthetic","input":"test"}), None).unwrap();
        let route: Route = serde_json::from_value(json!({"api":"responses","model":"synthetic","profile_id":"fixture","profile_version":"1","reasoning_contract":null,"support":{},"context_window":8192,"max_output_tokens":2048})).unwrap();
        let plan = plan_translation(
            &request,
            &ContinuityBinding {
                route: route.snapshot().unwrap(),
                scope: "test".into(),
            },
        )
        .unwrap();
        let history = VerifiedProviderHistory::default();
        for scenario in ["streaming", "managed_continuation", "wrong_api"] {
            let mut binding = binding(CAPABILITIES_PROTOCOL);
            request.generation.stream = Some(scenario == "streaming");
            if scenario == "wrong_api" {
                binding.capabilities.as_mut().unwrap().apis = vec!["messages".into()];
            }
            let result = super::super::execution::PreparedCodec::prepare(
                &binding,
                &request,
                &plan,
                &history,
                (scenario == "managed_continuation").then_some(false),
                65536,
                gateway_usage_contract::Profile::ResponsesV1,
            )
            .await;
            assert!(
                matches!(result, Err(IrError::UnsupportedFeature)),
                "{scenario}"
            );
        }
    }

    #[test]
    fn legacy_ready_never_accepts_subsets_or_capability_metadata() {
        for protocol in [PROTOCOL, EDITING_PROTOCOL] {
            let mut binding = binding(protocol);
            let ready = json!({"protocol":protocol,"sequence":0,"value":{"result":"ready",
                "apis":["responses","messages","chat_completions","gemini_interactions"],"replay_versions":[1]}});
            assert!(validate_ready(&binding, ready.clone()).is_ok());
            let mut subset = ready.clone();
            subset["value"]["apis"] = json!(["responses"]);
            assert!(validate_ready(&binding, subset).is_err());
            binding.capabilities = self::binding(CAPABILITIES_PROTOCOL).capabilities;
            assert!(validate_ready(&binding, ready).is_err());
        }
    }
}
