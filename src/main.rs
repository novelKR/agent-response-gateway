use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use agent_response_gateway::{Config, ConfigError, Secrets};
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(version, about = "Independent local Responses gateway")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the local HTTP service. The first stdout line announces readiness.
    Serve {
        #[arg(long)]
        config: PathBuf,
    },
    /// Validate configuration structure; credentials and providers are not probed.
    CheckConfig {
        #[arg(long)]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    match run(Cli::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), ConfigError> {
    let (path, serve) = match cli.command {
        Command::Serve { config } => (config, true),
        Command::CheckConfig { config } => (config, false),
    };
    let raw = std::fs::read_to_string(path)
        .map_err(|_| ConfigError("Cannot read configuration file".into()))?;
    let config = Config::parse(&raw)?;
    if !serve {
        println!(
            "{}",
            json!({"status": "valid", "credentials_checked": false, "provider_probe": false})
        );
        return Ok(());
    }
    let secrets = Secrets::from_env(&config)?;
    let address = config.listen;
    let grace = Duration::from_millis(config.limits.shutdown_grace_ms);
    let router = agent_response_gateway::router(config, secrets)?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|_| ConfigError("Cannot bind the configured loopback address".into()))?;
    let bound = listener
        .local_addr()
        .map_err(|_| ConfigError("Cannot determine listener address".into()))?;
    // Register handlers before announcing readiness to a supervising process.
    let shutdown = shutdown_signal()?;
    println!(
        "{}",
        json!({"event":"ready", "address":bound.to_string(), "base_url":format!("http://{bound}/v1"), "version":env!("CARGO_PKG_VERSION")})
    );
    io::stdout()
        .flush()
        .map_err(|_| ConfigError("Cannot write readiness message".into()))?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });
    let mut server_task = tokio::spawn(async { server.await });
    tokio::select! {
        result = &mut server_task => {
            result.map_err(|_| ConfigError("HTTP server task failed".into()))?.map_err(|_| ConfigError("HTTP server failed".into()))?;
        }
        _ = shutdown => {
            let _ = shutdown_tx.send(());
            if tokio::time::timeout(grace, &mut server_task).await.is_err() {
                server_task.abort();
                tracing::info!("shutdown_grace_expired");
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>, ConfigError> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| ConfigError("Cannot register termination handler".into()))?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| ConfigError("Cannot register interrupt handler".into()))?;
    Ok(async move {
        tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
    })
}

#[cfg(not(unix))]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>, ConfigError> {
    let mut interrupt = tokio::signal::windows::ctrl_c()
        .map_err(|_| ConfigError("Cannot register interrupt handler".into()))?;
    Ok(async move {
        interrupt.recv().await;
    })
}
