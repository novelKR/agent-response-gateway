//! Host-owned listener, runtime and shutdown around the ordinary validated router.
//! Run only with an explicitly prepared config and environment; tests compile this example.
use agent_response_gateway::{Config, Secrets, router};
use std::{error::Error, future::Future, io::Write};

async fn serve_hosted(
    listener: tokio::net::TcpListener,
    config: Config,
    secrets: Secrets,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), Box<dyn Error>> {
    let manifest = config.manifest()?;
    let address = listener.local_addr()?;
    let app = router(config, secrets)?;
    println!(
        "{}",
        serde_json::json!({"event":"ready","address":address.to_string(),
        "base_url":format!("http://{address}/v1"),"version":env!("CARGO_PKG_VERSION"),
        "schema":manifest.ready_schema(),"manifest_schema":manifest.schema(),
        "configuration_sha256":manifest.configuration_sha256()})
    );
    std::io::stdout().flush()?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Pass an explicitly prepared gateway configuration")?;
    let config = Config::parse(&std::fs::read_to_string(path)?)?;
    let secrets = Secrets::from_env(&config)?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    // This example is the host: the library does not install process-wide handlers.
    serve_hosted(listener, config, secrets, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}
