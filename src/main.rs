use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use agent_response_gateway::{
    Config, ConfigError, Secrets,
    extensions::{ExtensionPlan, ExtensionRuntime},
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
    /// Manage non-executable profile packs locally, without credentials or network access.
    ProfilePack {
        #[command(subcommand)]
        command: agent_response_gateway::profile_packs::manager::Command,
    },
    /// Explicitly initialize a new private continuation store; never replaces an existing database.
    InitContinuation {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long, default_value_t = 1073741824)]
        max_store_bytes: u64,
    },
    /// Start the local HTTP service. The first stdout line announces readiness.
    Serve {
        #[arg(long)]
        config: PathBuf,
        /// Explicit activation snapshot for trusted native metadata observers.
        #[arg(long)]
        extensions_lock: Option<PathBuf>,
        /// Explicit frozen activation of non-executable profile packs.
        #[arg(long)]
        profile_packs_lock: Option<PathBuf>,
    },
    /// Report the normalized embedded configuration and digest without reading credentials.
    Manifest {
        #[arg(long)]
        config: PathBuf,
        /// Inspect extension package bytes without running any executable.
        #[arg(long)]
        extensions_lock: Option<PathBuf>,
        /// Explicit frozen activation of non-executable profile packs.
        #[arg(long)]
        profile_packs_lock: Option<PathBuf>,
    },
    /// Validate configuration structure; credentials and providers are not probed.
    CheckConfig {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        extensions_lock: Option<PathBuf>,
        /// Explicit frozen activation of non-executable profile packs.
        #[arg(long)]
        profile_packs_lock: Option<PathBuf>,
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
    if let Command::ProfilePack { command } = cli.command {
        println!(
            "{}",
            agent_response_gateway::profile_packs::manager::run(command)?
        );
        return Ok(());
    }
    if let Command::InitContinuation {
        directory,
        max_store_bytes,
    } = &cli.command
    {
        let store = agent_response_gateway::continuation::SqliteStore::open(
            directory,
            true,
            *max_store_bytes,
        )
        .map_err(|_| ConfigError("Cannot initialize new continuation store".into()))?;
        println!(
            "{}",
            json!({"schema":agent_response_gateway::continuation::SCHEMA,"store_id":store.identity().map_err(|_|ConfigError("Store identity unavailable".into()))?})
        );
        return Ok(());
    }
    let (path, extensions_lock, profile_packs_lock) = match &cli.command {
        Command::InitContinuation { .. } | Command::ProfilePack { .. } => unreachable!(),
        Command::Serve {
            config,
            extensions_lock,
            profile_packs_lock,
        }
        | Command::CheckConfig {
            config,
            extensions_lock,
            profile_packs_lock,
        }
        | Command::Manifest {
            config,
            extensions_lock,
            profile_packs_lock,
        } => (config, extensions_lock, profile_packs_lock),
    };
    let raw = std::fs::read_to_string(path)
        .map_err(|_| ConfigError("Cannot read configuration file".into()))?;
    let config = if let Some(path) = profile_packs_lock {
        Config::parse_with_profile_packs(
            &raw,
            agent_response_gateway::profile_packs::ProfilePackPlan::load(path)?,
        )?
    } else {
        Config::parse(&raw)?
    };
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
            if profile_packs_lock.is_some() {
                report["profile_packs_checked"] = json!(true);
                report["profile_packs_executed"] = json!(false);
            }
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
        Command::InitContinuation { .. } | Command::ProfilePack { .. } => unreachable!(),
    }
    let secrets = Secrets::from_env(&config)?;
    let continuation_enabled = config.continuation.is_some();
    let compatibility_enabled = manifest.schema() == "gateway-embedded-manifest/v4";
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
        "schema":manifest.ready_schema(),"manifest_schema":manifest.schema(),"configuration_sha256":manifest.configuration_sha256()});
    if let Some(extended) = &extended_manifest {
        readiness["schema"] = json!(if manifest.schema() == "gateway-embedded-manifest/v5" {
            "gateway-extended-ready/v5"
        } else if compatibility_enabled {
            "gateway-extended-ready/v4"
        } else if continuation_enabled {
            "gateway-extended-ready/v3"
        } else {
            extensions.as_ref().expect("extension plan").ready_schema()
        });
        readiness["manifest_schema"] = extended["schema"].clone();
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
