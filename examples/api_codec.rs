//! Reference full-API codec. Stdio is the only integration surface.
fn main() -> std::process::ExitCode {
    match agent_response_gateway::codecs::engine::serve(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}
