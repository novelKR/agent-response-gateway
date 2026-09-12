use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use agent_response_gateway::{
    Config, ConfigError, Secrets,
    extensions::{ExtensionPlan, ExtensionRuntime},
    manifest::{MANIFEST_SCHEMA, READY_SCHEMA},
};
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
        /// Explicit activation snapshot for trusted native metadata observers.
        #[arg(long)]
        extensions_lock: Option<PathBuf>,
    },
    /// Report the normalized embedded configuration and digest without reading credentials.
    Manifest {
        #[arg(long)]
        config: PathBuf,
        /// Inspect extension package bytes without running any executable.
        #[arg(long)]
        extensions_lock: Option<PathBuf>,
    },
    /// Validate configuration structure; credentials and providers are not probed.
    CheckConfig {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        extensions_lock: Option<PathBuf>,
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
    let (path, extensions_lock) = match &cli.command {
        Command::Serve {
            config,
            extensions_lock,
        }
        | Command::CheckConfig {
            config,
            extensions_lock,
        }
        | Command::Manifest {
            config,
            extensions_lock,
        } => (config, extensions_lock),
    };
    let raw = std::fs::read_to_string(path)
        .map_err(|_| ConfigError("Cannot read configuration file".into()))?;
    let config = Config::parse(&raw)?;
    let extensions = extensions_lock
        .as_deref()
        .map(ExtensionPlan::load)
        .transpose()?;
    let manifest = config.manifest()?;
    let base_manifest = serde_json::to_value(&manifest).expect("manifest JSON");
    let extended_manifest = extensions
        .as_ref()
        .map(|plan| plan.manifest(&base_manifest))
        .transpose()?;
    match cli.command {
        Command::CheckConfig { .. } => {
            let mut report =
                json!({"status": "valid", "credentials_checked": false, "provider_probe": false});
            if let Some(plan) = &extensions {
                report["extensions_checked"] = json!(true);
                report["extensions_executed"] = json!(false);
                report["extensions_sha256"] = json!(plan.configuration_sha256());
            }
            println!("{report}");
            return Ok(());
        }
        Command::Manifest { .. } => {
            if let Some(extended) = &extended_manifest {
                println!("{extended}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&manifest).expect("manifest JSON")
                );
            }
            return Ok(());
        }
        Command::Serve { .. } => {}
    }
    let secrets = Secrets::from_env(&config)?;
    let address = config.listen;
    let grace = Duration::from_millis(config.limits.shutdown_grace_ms);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|_| ConfigError("Cannot bind the configured loopback address".into()))?;
    let bound = listener
        .local_addr()
        .map_err(|_| ConfigError("Cannot determine listener address".into()))?;
    // Register handlers before announcing readiness to a supervising process.
    let shutdown = shutdown_signal()?;
    let extension_runtime = extensions
        .as_ref()
        .map(ExtensionRuntime::start)
        .transpose()?;
    let router = agent_response_gateway::router_with_usage(
        config,
        secrets,
        extension_runtime.as_ref().map(ExtensionRuntime::sink),
        extension_runtime
            .as_ref()
            .and_then(ExtensionRuntime::usage_sink),
    )?;
    let mut readiness = json!({"event":"ready", "address":bound.to_string(), "base_url":format!("http://{bound}/v1"), "version":env!("CARGO_PKG_VERSION"),
        "schema":READY_SCHEMA,"manifest_schema":MANIFEST_SCHEMA,"configuration_sha256":manifest.configuration_sha256()});
    if let Some(extended) = &extended_manifest {
        readiness["schema"] = json!(extensions.as_ref().expect("extension plan").ready_schema());
        readiness["manifest_schema"] = json!(
            extensions
                .as_ref()
                .expect("extension plan")
                .manifest_schema()
        );
        readiness["execution_sha256"] = extended["execution_sha256"].clone();
    }
    println!("{readiness}");
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
    drop(extension_runtime);
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

#[cfg(windows)]
fn shutdown_signal() -> Result<impl std::future::Future<Output = ()>, ConfigError> {
    let mut interrupt = tokio::signal::windows::ctrl_c()
        .map_err(|_| ConfigError("Cannot register interrupt handler".into()))?;
    let mut terminate = tokio::signal::windows::ctrl_break()
        .map_err(|_| ConfigError("Cannot register termination handler".into()))?;
    Ok(async move {
        tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
    })
}
