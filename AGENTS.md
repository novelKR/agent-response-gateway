# Repository instructions

This is an independent Rust Responses gateway. Read the relevant source,
configuration, tests, and documentation before editing. Preserve unrelated work.

## Responsibility boundaries

- The gateway owns model HTTP transport, declared routing, upstream credential
  selection, and protocol compatibility. It does not execute tools, approve
  user actions, manage canon, or own consumer workflows.
- Keep consumer-specific domain types and deployment identities out of this
  package. Generic integration contracts are documented in `docs/integration.md`.
- Preserve the loopback-only boundary until a separately reviewed service mode
  defines authentication, deployment, and tenant requirements.
- Do not add implicit retries, fallback routes, redirects, environment proxy
  inheritance, or silent feature degradation.
- Never log request/response bodies, prompts, credentials, or authentication
  headers. Use synthetic fixtures and mock upstreams for tests.

## Implementation and validation

- Use Rust 1.98.0, edition 2024, UTF-8 text, and the committed Cargo lockfile.
- Keep build artifacts under `target/` and local test state under `.local/` or
  another ignored project-local directory.
- Run `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`.
- Use no paid/live provider calls in the default tests. A passing mock test is
  not Codex conformance, hosted CI completion, or consumer operational acceptance.
- Update the documented support matrix and integration boundaries when behavior
  changes. Unsupported protocol features must remain explicit errors.
- Run `python3 -B -m unittest discover -s scripts/tests -v` and the publication
  boundary checks described in `docs/documentation.md` before committing.
- Use Python 3.11+ for script checks. Follow `licensing/README.md` to prepare
  the pinned development-only cargo-deny and exact crate sources, then run
  `python3 -B scripts/license_audit.py check`. Keep license records and original
  notices consistent with Cargo.lock; refresh creates a reviewable diff, not
  approval of commercial rights. Never globally allow third-party AGPL merely
  because the root package uses it.

## Public documentation and local repositories

- Public documentation contains reusable product contracts, not private consumer
  names, repository URLs, local paths, topology, or operational records.
- `.private/` is reserved for an optional independent local Git repository. The
  parent must ignore it; never add a gitlink, `.gitmodules` entry, or public link
  to private contents. Public builds and tests must work without this directory.
- Accessing `.private/` for internal work does not authorize copying it into
  public files, logs, commit messages, CI artifacts, or source archives.
- Preserve original internal records before generalizing public documents. Keep
  private matching dictionaries in the local repository, not public CI source.
- Parent and local repositories have independent histories and backups. Do not
  delete local repositories while cleaning the public working tree.

## Licensing and release

- Public project code is AGPL-3.0-only. Preserve `LICENSE` and third-party notices.
- `COMMERCIAL-LICENSING.md` describes a future separately negotiated license; it
  grants no alternative permission by itself.
- Do not merge external code contributions until the rights holder and sufficient
  contribution/relicensing terms have been established.
- Do not copy code, tests, prompts, manuscripts, credentials, or Git history from
  consumers or reference implementations without explicit provenance and review.
- Do not invent an owner, contact address, public repository, source URL, release,
  attestation, executed validation, or completed commercial agreement.
- For a distributed build, provide the corresponding source and dependency
  notices for that exact version. Follow `docs/release.md`.
