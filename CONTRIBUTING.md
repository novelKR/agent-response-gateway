<a id="기여-정책"></a>

# Contribution policy

[English](CONTRIBUTING.md) | [한국어](CONTRIBUTING.ko.md)

Use the public repository's [Issues](https://github.com/novelKR/agent-response-gateway/issues)
for problem reports, API contract feedback and reproduction steps without secrets.
This document does not provide a commercial contracting contact or execute an agreement.

**Merging external code contributions is on hold until the rights holder and
contribution terms are established.** This policy requires sufficient permission
to use the same implementation in the public edition and a separately contracted
edition. Do not assume a CLA has been signed or copyright transferred. A DCO
sign-off alone does not establish alternative-licensing rights.

Before opening external code contributions, define:

- How to verify the contributor's rights and the provenance of third-party components.
- The modification, distribution and relicensing permissions required for both editions.
- The rights holder, contracting party, contribution terms and retention of consent records.

Review external contributions, ports and copied code using the records below.
Public records contain only publishable provenance and permission terms. Retain
consents, contracts and internal identifiers in independent private history.
Writing a policy does not create consent.

| Item | Required record |
|---|---|
| Original | Public source location, exact version or commit and imported scope |
| Permission | Original license expression, selected permission and required license, copyright and change notices |
| Provenance evidence | Source and notice hashes, plus evidence supporting any license exception |
| Distribution rights | Evidence and unresolved questions about permission for public and separately contracted distribution |

Do not treat license checks, passing tests, DCO sign-offs or general contribution
consent as commercial relicensing authority. Keep the merge on hold while required
rights remain unresolved.

For internal development, read existing files, make a focused change, and run
`cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and
`cargo test --locked`. Do not add tests requiring real API keys to default CI.
Use publishable synthetic data and mock providers.

When changing dependencies, review the lockfile, license selections and original
notices in the same change under the [license-management procedure](licensing/README.md).
`refresh` produces a reviewable diff; `check` verifies consistency. Do not expand
the allowlist or supplemental notices to hide a failure. Python checks require
version 3.11 or later.

Do not import code or tests from another implementation or a private consumer
without authorization. Permitted reuse must preserve original copyright, license
notices and provenance. Do not put manuscripts, configuration material, internal
business data or credentials into issues, PRs or fixtures. Describe generic
integration requirements without exposing consumer names, repository URLs or
internal topology. Follow [documentation management](docs/documentation.md) to
separate public documentation and local records.

Review API changes with the [support contract](docs/protocol.md), and consumer
responsibility changes with the [integration boundaries](docs/integration.md).
