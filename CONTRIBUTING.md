<a id="기여-정책"></a>

# Contribution policy

[English](CONTRIBUTING.md) | [한국어](CONTRIBUTING.ko.md)

This project uses **AGPL-3.0-only and a separate commercial license**.
Contributions must support both licensing options described in the
[licensing policy](COMMERCIAL-LICENSING.md).

<a id="코드-기여에-필요한-권한"></a>

## Rights required for code contributions

Code is merged only after the following conditions are verified:

1. The contributor owns the necessary rights or has permission from their rights
   holder, including their employer where applicable.
2. A written contribution agreement grants the project the rights to use, modify
   and redistribute the contribution under both AGPL-3.0-only and commercial terms.
3. Copied or adapted material identifies its original source, exact version,
   license and required notices. Its terms must permit the proposed reuse.
4. The maintainer has reviewed the provenance and recorded the required consent.
   A pull request or DCO sign-off alone does not grant commercial relicensing rights.

Keep private agreements and consent records out of public issues and source files.

<a id="변경-제출"></a>

## Submitting a change

Use [Issues](https://github.com/novelKR/agent-response-gateway/issues) for bug reports,
reproduction steps and API feedback. Discuss substantial changes before opening a
pull request. Keep each change focused and include relevant tests and documentation.

Read the repository instructions and run `cargo fmt --check`,
`cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked`.
Default tests use synthetic inputs and mock providers, without real API keys.

Dependency changes must include the lockfile and required license records under
[license management](licensing/README.md). Use Python 3.11 or later for script checks.

Do not include credentials, private configuration, business data or consumer code
without permission. Preserve the original notices for permitted reuse. Follow
[documentation management](docs/documentation.md) for public content,
[the support contract](docs/protocol.md) for API changes and
[integration boundaries](docs/integration.md) for host responsibilities.
