//! Test-only host composition: read-only synthetic metadata, not an application backend.
use gateway_management::{
    Action, Actor, Effect, Error, Grant, Id, Identity, Journal, Operation, PreparedOperation,
    Reader, Request, Snapshot,
};
use gateway_management_api::{
    Authenticator, Command, CredentialKind, Dispatcher, Feature, LocalAuthenticator,
    LocalCredential, Query,
};
use gateway_management_embedded::{
    HOST_SCHEMA, HostContract, HostDispatcher, IDENTITY_SCHEMA, IdentityVerifier, Lifecycle,
    VerifiedIdentity,
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Read, sync::Arc};
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
struct VerifiedHost(LocalAuthenticator);
impl IdentityVerifier for VerifiedHost {
    fn authenticate(&self, credential: &str) -> Option<VerifiedIdentity> {
        self.0
            .authenticate(credential)
            .map(|principal| VerifiedIdentity {
                schema: IDENTITY_SCHEMA.into(),
                principal,
            })
    }
    fn refresh(
        &self,
        identity: &Identity,
        version: &gateway_management::Digest,
    ) -> Option<VerifiedIdentity> {
        self.0
            .refresh(identity, version)
            .map(|principal| VerifiedIdentity {
                schema: IDENTITY_SCHEMA.into(),
                principal,
            })
    }
}
struct Metadata;
impl Dispatcher for Metadata {
    fn features(&self) -> Vec<Feature> {
        vec![Feature {
            id: id("synthetic-host"),
            version: "example/v1".into(),
            installed: true,
            enabled: true,
            operations: self.supported(),
        }]
    }
    fn supported(&self) -> Vec<Action> {
        vec![Action::ReadState, Action::ReadOperations]
    }
    fn snapshot(&mut self, _: &Command) -> gateway_management::Result<Snapshot> {
        Err(Error::Unsupported)
    }
    fn read(&mut self, _: &Actor, query: &Query) -> gateway_management::Result<Value> {
        if !matches!(query, Query::State) {
            return Err(Error::Unsupported);
        }
        Ok(
            json!({"schema":gateway_management_api::STATE_SCHEMA,"modules":[{"id":"host","contract":"synthetic-host/v1","observation":{"state":"observed","observed_at_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|Error::Storage)?.as_millis(),"data":{"schema":"synthetic-host/v1","lifecycle_owner":"host"}}}]}),
        )
    }
    fn prepare<'a>(
        &'a mut self,
        _: &'a Request,
        _: &'a Command,
    ) -> gateway_management::Result<Box<dyn PreparedOperation + 'a>> {
        Err(Error::Unsupported)
    }
    fn reconcile(&mut self, _: &Operation) -> gateway_management::Result<Effect> {
        Err(Error::Unsupported)
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("provide an empty private directory")?,
    )
    .canonicalize()?;
    let journal = Journal::initialize(&directory, 8 * 1024 * 1024)?;
    let reader = Reader::open(&directory)?;
    let declaration = HostContract {
        schema: HOST_SCHEMA.into(),
        target: id("gateway"),
        lifecycle: Lifecycle::HostOwned,
        operations: BTreeSet::from([Action::ReadState, Action::ReadOperations]),
    };
    let auth = LocalAuthenticator::new(vec![LocalCredential {
        token: "synthetic-embedded-read-key-01234567890123456789".into(),
        identity: Identity {
            subject: id("synthetic-host-reader"),
            credential: id("synthetic-host-key"),
        },
        kind: CredentialKind::ReadOnly,
        grants: declaration
            .operations
            .iter()
            .map(|action| Grant {
                action: *action,
                target: id("gateway"),
            })
            .collect(),
    }])?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let bound = listener.local_addr()?;
    let host = HostDispatcher::new(declaration, Box::new(Metadata))?;
    let service = host.service(bound, Arc::new(VerifiedHost(auth)), journal, reader, true)?;
    let (send, receive) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        let _ = std::io::stdin().read(&mut byte);
        let _ = send.send(());
    });
    println!("Synthetic host API: http://{bound}/management/v1");
    axum::serve(listener, service.router())
        .with_graceful_shutdown(async {
            let _ = receive.await;
        })
        .await?;
    Ok(())
}
