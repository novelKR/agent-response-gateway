use crate::{Application, Component, settings::Settings};
use clap::{Parser, Subcommand};
use std::{io::Read, path::PathBuf, process::ExitCode, sync::Arc};
#[derive(Parser)]
#[command(
    version,
    about = "Opt-in local Gateway management with explicit stores and execution ownership"
)]
struct Cli {
    #[arg(long)]
    registration: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long, value_enum)]
        component: Component,
    },
    Backup {
        #[arg(long, value_enum)]
        component: Component,
        #[arg(long)]
        destination: PathBuf,
    },
    Serve {
        /// Stop the manager and its owned Gateway when the supervisor closes stdin.
        #[arg(long)]
        parent_stdin: bool,
    },
}
pub async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        Err(_) => {
            eprintln!("invalid_manager_arguments");
            return ExitCode::from(2);
        }
    };
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("manager_operation_failed");
            ExitCode::FAILURE
        }
    }
}
async fn run(cli: Cli) -> gateway_management::Result<()> {
    use gateway_management::Error;
    let settings = Settings::load(&cli.registration)?;
    match cli.command {
        Command::Init { component } => {
            crate::initialize(&settings, component)?;
            println!("management_store_initialized");
            Ok(())
        }
        Command::Backup {
            component,
            destination,
        } => {
            crate::backup(&settings, component, &destination)?;
            println!("management_store_backed_up");
            Ok(())
        }
        Command::Serve { parent_stdin } => {
            let listener = tokio::net::TcpListener::bind(settings.listen)
                .await
                .map_err(|_| Error::Storage)?;
            let bound = listener.local_addr().map_err(|_| Error::Storage)?;
            #[cfg(feature = "team")]
            let team_listener = if let Some(team) = &settings.team {
                Some(
                    tokio::net::TcpListener::bind(team.listen)
                        .await
                        .map_err(|_| Error::Storage)?,
                )
            } else {
                None
            };
            #[cfg(feature = "team")]
            let team_bound = team_listener
                .as_ref()
                .map(|l| l.local_addr().map_err(|_| Error::Storage))
                .transpose()?;
            #[cfg(not(feature = "team"))]
            let team_bound = None;
            let mut app =
                tokio::task::spawn_blocking(move || Application::open(settings, bound, team_bound))
                    .await
                    .map_err(|_| Error::Storage)??;
            let router = app.router();
            let app = Arc::new(app);
            let (quit, receive) = tokio::sync::watch::channel(false);
            let server = tokio::spawn(
                axum::serve(listener, router)
                    .with_graceful_shutdown(stopped(receive.clone()))
                    .into_future(),
            );
            #[cfg(feature = "team")]
            let team_server =
                if let (Some(listener), Some(team)) = (team_listener, app.team.as_ref()) {
                    Some(tokio::spawn(
                        axum::serve(listener, team.router())
                            .with_graceful_shutdown(stopped(receive.clone()))
                            .into_future(),
                    ))
                } else {
                    None
                };
            let (parent_send, parent_receive) = tokio::sync::oneshot::channel();
            if parent_stdin {
                std::thread::spawn(move || {
                    let mut byte = [0u8; 1];
                    let _ = std::io::stdin().read(&mut byte);
                    let _ = parent_send.send(());
                });
            } else {
                drop(parent_send);
            }
            println!(
                "{}",
                serde_json::json!({"schema":"gateway-management-listeners/v1","target":app.target,"management":format!("http://{bound}"),"team":team_bound.map(|a|format!("http://{a}"))})
            );
            if parent_stdin {
                tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = parent_receive=>{}}
            } else {
                let _ = tokio::signal::ctrl_c().await;
            }
            app.api.close_admission();
            let _ = quit.send(true);
            let cleanup = app.clone();
            let result = tokio::task::spawn_blocking(move || cleanup.shutdown())
                .await
                .map_err(|_| Error::Storage)?;
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
            #[cfg(feature = "team")]
            if let Some(server) = team_server {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
            }
            result
        }
    }
}
async fn stopped(mut receiver: tokio::sync::watch::Receiver<bool>) {
    if !*receiver.borrow() {
        let _ = receiver.changed().await;
    }
}
