use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use gateway_management::Id;
use gateway_management_api::{Preflight, Submission};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    version,
    about = "Authenticated client for an explicitly selected local management API"
)]
struct Cli {
    #[arg(long)]
    endpoint: String,
    #[arg(long, default_value = "GATEWAY_MANAGEMENT_TOKEN")]
    token_env: String,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Capabilities {
        #[arg(long)]
        target: String,
    },
    State {
        #[arg(long)]
        target: String,
    },
    Operations {
        #[arg(long)]
        target: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Operation {
        #[arg(long)]
        target: String,
        #[arg(long)]
        id: String,
    },
    Usage {
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long)]
        target: String,
        #[arg(long)]
        from_ms: u64,
        #[arg(long)]
        to_ms: u64,
        #[arg(long, default_value = "UTC")]
        timezone: String,
    },
    Preflight {
        #[arg(long)]
        file: PathBuf,
    },
    Submit {
        #[arg(long)]
        file: PathBuf,
    },
    DeliverCredential {
        #[arg(long)]
        file: PathBuf,
        /// New file in an existing private directory. Never overwritten or sent to the server.
        #[arg(long)]
        output: PathBuf,
    },
    Reconcile {
        #[arg(long)]
        target: String,
        #[arg(long)]
        id: String,
    },
}
fn read<T: serde::de::DeserializeOwned>(path: PathBuf) -> Result<T, ()> {
    let mut bytes = vec![];
    std::fs::File::open(path)
        .map_err(|_| ())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 65536 {
        return Err(());
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}
fn id(value: String) -> Result<String, ()> {
    Id::new(value).map(String::from).map_err(|_| ())
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return std::process::ExitCode::SUCCESS;
        }
        Err(_) => {
            eprintln!("invalid_management_arguments");
            return std::process::ExitCode::from(2);
        }
    };
    match run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(()) => {
            eprintln!("management_request_failed");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn run(cli: Cli) -> Result<(), ()> {
    let base = url::Url::parse(&cli.endpoint).map_err(|_| ())?;
    let local = match base.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if base.scheme() != "http"
        || !local
        || base.port().is_none_or(|p| p == 0)
        || base.path() != "/"
        || base.query().is_some()
        || base.fragment().is_some()
        || !base.username().is_empty()
        || base.password().is_some()
    {
        return Err(());
    }
    if cli.token_env.is_empty()
        || cli.token_env.len() > 128
        || !cli
            .token_env
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(());
    }
    let token = std::env::var(&cli.token_env).map_err(|_| ())?;
    if !(32..=4096).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| ())?;
    let root = base.join("management/v1/").map_err(|_| ())?;
    let mut delivery_file = if let Command::DeliverCredential { output, .. } = &cli.command {
        let absolute = if output.is_absolute() {
            output.clone()
        } else {
            std::env::current_dir().map_err(|_| ())?.join(output)
        };
        let parent = absolute.parent().ok_or(())?;
        gateway_management::filesystem::directory(parent).map_err(|_| ())?;
        let name = output.file_name().ok_or(())?;
        Some(gateway_management::filesystem::private_new(&parent.join(name)).map_err(|_| ())?)
    } else {
        None
    };
    let request = match cli.command {
        Command::Capabilities { target } => client
            .get(root.join("capabilities").map_err(|_| ())?)
            .query(&[("target", id(target)?)]),
        Command::State { target } => client
            .get(root.join("state").map_err(|_| ())?)
            .query(&[("target", id(target)?)]),
        Command::Operations {
            target,
            after,
            limit,
        } => client
            .get(root.join("operations").map_err(|_| ())?)
            .query(&[
                ("target", id(target)?),
                ("after", after.to_string()),
                ("limit", limit.to_string()),
            ]),
        Command::Operation {
            target,
            id: operation,
        } => client
            .get(
                root.join(&format!("operations/{}", id(operation)?))
                    .map_err(|_| ())?,
            )
            .query(&[("target", id(target)?)]),
        Command::Usage {
            after,
            target,
            from_ms,
            to_ms,
            timezone,
        } => client.get(root.join("usage").map_err(|_| ())?).query(&[
            ("target", id(target)?),
            ("from_ms", from_ms.to_string()),
            ("to_ms", to_ms.to_string()),
            ("timezone", timezone),
            ("after", after.to_string()),
        ]),
        Command::Preflight { file } => client
            .post(root.join("preflight").map_err(|_| ())?)
            .json(&read::<Preflight>(file)?),
        Command::Submit { file } => client
            .post(root.join("operations").map_err(|_| ())?)
            .json(&read::<Submission>(file)?),
        Command::DeliverCredential { file, .. } => client
            .post(root.join("credential-delivery").map_err(|_| ())?)
            .json(&read::<Submission>(file)?),
        Command::Reconcile {
            target,
            id: operation,
        } => client
            .post(
                root.join(&format!("operations/{}/reconcile", id(operation)?))
                    .map_err(|_| ())?,
            )
            .json(
                &serde_json::json!({"schema":gateway_management_api::SCHEMA,"target":id(target)?}),
            ),
    };
    let response = request.bearer_auth(token).send().await.map_err(|_| ())?;
    let success = response.status().is_success();
    let mut stream = response.bytes_stream();
    let mut bytes = vec![];
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if value["schema"] != gateway_management_api::SCHEMA {
        return Err(());
    }
    if let Some(output) = &mut delivery_file {
        let secret = value["data"]
            .as_object_mut()
            .and_then(|data| data.remove("credential"));
        if success && value["data"]["delivery"] == "one_time" {
            let secret = zeroize::Zeroizing::new(
                secret
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or(())?,
            );
            if secret.len() > 4096
                || !secret.starts_with("gwt1_")
                || !secret.bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(());
            }
            output
                .write_all(secret.as_bytes())
                .and_then(|_| output.sync_all())
                .map_err(|_| ())?;
        }
    }
    // A response is metadata only. Never print the request, environment value or HTTP headers.
    serde_json::to_writer(std::io::stdout().lock(), &value).map_err(|_| ())?;
    std::io::stdout().write_all(b"\n").map_err(|_| ())?;
    if success { Ok(()) } else { Err(()) }
}
