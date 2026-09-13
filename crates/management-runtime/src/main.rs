//! A separately selected, parent-supervised host around the ordinary gateway router.
use agent_response_gateway::{Secrets, extensions::ExtensionRuntime};
use gateway_management::{Error, Result, filesystem};
use gateway_management_runtime::protocol::{LAUNCH_SCHEMA, Launch, READY_SCHEMA, Ready};
use std::io::{BufRead, Read, Write};

fn frame(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    reader
        .take(65537)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() > 65536 || bytes.last() != Some(&b'\n') {
        return Err(Error::InvalidInput);
    }
    Ok(bytes)
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("managed_gateway_failed");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("gateway-managed-child {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if !args.is_empty() {
        return Err(Error::InvalidInput);
    }
    let mut input = std::io::BufReader::new(std::io::stdin());
    let launch: Launch =
        serde_json::from_slice(&frame(&mut input)?).map_err(|_| Error::InvalidInput)?;
    if launch.schema != LAUNCH_SCHEMA {
        return Err(Error::InvalidInput);
    }
    filesystem::directory(&launch.directory)?;
    let _lease = filesystem::lease(&launch.directory.join("runtime.lease"))?;
    let inspected = gateway_management_runtime::prepare_launch(&launch)?;
    let manifest = inspected
        .config
        .manifest()
        .map_err(|_| Error::InvalidInput)?;
    let secrets = Secrets::from_env(&inspected.config).map_err(|_| Error::InvalidInput)?;
    let grace = std::time::Duration::from_millis(inspected.config.limits.shutdown_grace_ms);
    let listener = tokio::net::TcpListener::bind(inspected.config.listen)
        .await
        .map_err(|_| Error::Storage)?;
    let bound = listener.local_addr().map_err(|_| Error::Storage)?;
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        // EOF, malformed control, and an explicit stop all close this owned runtime.
        let _ = frame(&mut input);
        let _ = stop_tx.send(());
    });
    let extensions = inspected
        .extensions
        .as_ref()
        .map(ExtensionRuntime::start)
        .transpose()
        .map_err(|_| Error::InvalidInput)?;
    let ready = Ready {
        schema: READY_SCHEMA.into(),
        instance_id: launch.instance_id,
        gateway: serde_json::from_value(
            manifest
                .readiness(bound, inspected.extensions.as_ref())
                .map_err(|_| Error::InvalidInput)?,
        )
        .map_err(|_| Error::InvalidInput)?,
    };
    let router = agent_response_gateway::router_with_usage(
        inspected.config,
        secrets,
        extensions.as_ref().map(ExtensionRuntime::sink),
        extensions.as_ref().and_then(ExtensionRuntime::usage_sink),
    )
    .map_err(|_| Error::InvalidInput)?;
    println!(
        "{}",
        serde_json::to_string(&ready).map_err(|_| Error::InvalidInput)?
    );
    std::io::stdout().flush().map_err(|_| Error::Storage)?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });
    let mut running = tokio::spawn(async { server.await });
    tokio::select! {
        result = &mut running => { result.map_err(|_| Error::Storage)?.map_err(|_| Error::Storage)?; }
        _ = stop_rx => {
            let _ = shutdown_tx.send(());
            if tokio::time::timeout(grace, &mut running).await.is_err() { running.abort(); }
        }
    }
    drop(extensions);
    Ok(())
}
