#[tokio::main]
async fn main() -> std::process::ExitCode {
    gateway_management_app::cli::main().await
}
