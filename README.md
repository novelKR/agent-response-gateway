# agent-response-gateway

[English](README.md) | [한국어](README.ko.md)

An independent local Responses gateway for agent runtimes and backend services.
It maps configured model aliases to provider models and uses separate upstream
credentials for Responses, Messages and Chat Completions.

The gateway forwards JSON/SSE and checks declared conversion features. Your
application executes tools, manages approval and keeps conversation history.
Connections are restricted to loopback addresses.

<a id="시작하기"></a>

## Getting started

For a packaged build, select a specific version and platform using the
[download and execution guide](docs/usage.md#run-a-downloaded-package).
The source-build steps below require Rust 1.98.0.

Rust 1.98.0 and Cargo are required. The pinned toolchain is in
`rust-toolchain.toml`; dependency versions are recorded in `Cargo.lock`.

```sh
cargo build --locked
cp config.example.toml config.local.toml
```

Set the provider `base_url` and `upstream_model` in `config.local.toml` for the
route you intend to use. `base_url` is an API prefix such as `/v1`; the declared
API appends `/responses`, `/messages` or `/chat/completions`. For Messages, follow
the [configuration example](config.messages.example.toml) and [support matrix](docs/messages.md).
Use mock provider addresses and synthetic inputs for initial verification. Sending
requests to a real provider with the client below can incur provider charges.

Set `ARG_LOCAL_TOKEN` to a whitespace-free ASCII token of 32–4096 characters, and
set `ARG_EXAMPLE_API_KEY` to a different provider key. Keep keys out of TOML and Git.
For example, generate a local development token with:

```sh
export ARG_LOCAL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
```

`check-config` validates the TOML structure, routes and limits. It does not check
environment credentials or a live provider. Starting `serve` also requires the
provider-key environment variables and checks their local format.

```sh
cargo run --locked -- check-config --config config.local.toml
cargo run --locked -- manifest --config config.local.toml
cargo run --locked -- serve --config config.local.toml
```

The default binding, `127.0.0.1:0`, selects an available port. Once ready, stdout
contains one readiness JSON line; logs go to stderr. The fields schema,
manifest_schema and configuration_sha256 identify the protocol and effective
configuration. Compare the digest with the offline manifest before starting the agent.

```json
{"event":"ready","address":"127.0.0.1:43127","base_url":"http://127.0.0.1:43127/v1","version":"0.1.0","schema":"gateway-ready/v1","manifest_schema":"gateway-embedded-manifest/v1","configuration_sha256":"<64 lowercase hex characters>"}
```

Use the same `ARG_LOCAL_TOKEN` in a separate shell. Replace the example port with
the actual readiness address. The Python client uses only the standard library.

```sh
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --list-models
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --model example/writer --input 'Reply with hello.'
python3 examples/client.py --base-url http://127.0.0.1:43127/v1 --model example/writer --input 'Reply with hello.' --stream
```

For provider-qualified names, aliases such as `Large-Model`, and separate aliases
for multiple API keys, see the [model naming and routing examples](docs/route-design.md#consumer-model-names).
Continue with [calling and integration examples](docs/usage.md) for conversations,
tools and streams, or [troubleshooting](docs/troubleshooting.md) to diagnose a failure.

<a id="지원-범위"></a>

## Supported behavior

<SupportTable>

| Interface | Behavior |
|---|---|
| `GET /` | Version, license and configured source location |
| `GET /healthz` | Local process liveness |
| `GET /readyz` | Local readiness; no real-provider probe |
| `GET /v1/models` | Configured model aliases after Bearer authentication |
| `POST /v1/responses` | Model mapping, provider-credential replacement and JSON/SSE forwarding after Bearer authentication |

</SupportTable>

Native Responses does not reconstruct tools, structured output or reasoning
items. JSON values are preserved except for model mapping and `store:false`
normalization; identical serialization bytes are not promised. Native SSE response
bodies are forwarded byte for byte. Converted routes map declared function,
custom and namespace tools and text, rejecting unsupported required features
before sending. Registration in `/v1/models` is not model compatibility verification.

Gateway storage, server state through `previous_response_id` or conversation,
remote compact API, background execution, response lookup/deletion, implicit
retries/fallback, WebSocket and OAuth/account pools are unsupported. Hosts can own
local compaction, resume and recovery through their Codex history and journals.
The [continuity contract](docs/continuity.md) describes verified bindings and
recovery of uncertain work. See the [protocol](docs/protocol.md) for HTTP limits.

<a id="내부-ir-v1"></a>

## Internal IR v1

The library's IR v1 represents request semantics, output-event state, capability
admission and origin-bound opaque state. It includes a pure Responses round-trip
codec, custom-tool JSON / namespace / patch-grammar bridges and event validation.
Converted HTTP routes use this contract; native routes retain original forwarding.
The [IR contract](docs/ir.md) documents its supported subset and adapter boundaries.

<a id="검증"></a>

## Validation

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python3 -B -m unittest discover -s scripts/tests -v
python3 -B scripts/license_audit.py check
```

Python checks require 3.11 or later. License checks read the separately prepared
cargo-deny 0.20.2 and exact locked crate sources offline. Follow the
[tool preparation and notice guide](licensing/README.md) first.

Tests use mock upstreams and synthetic data without real model API keys. CI runs
Rust 1.98.0 checks and native package execution on Linux x64/ARM64, macOS ARM64
and Windows x64. Verify the relevant commit in the
[public CI runs](https://github.com/novelKR/agent-response-gateway/actions/workflows/ci.yml).
CI success is not real-provider qualification or operational acceptance.

The [Codex conformance suite](tests/codex/README.md) runs the actual
`0.154.0-alpha.6` test runtime with mock providers. The [runtime guide](docs/codex-contract.md)
explains preparation, and [conformance](docs/conformance.md) lists the scenarios.
Test the selected real model and application integration before operational use.

<a id="통합과-라이선스"></a>

## Integration and licensing

An embedding application can pin a verified gateway release by version and hash,
then let its runtime manager supervise it alongside the agent. Backend services
retain ownership of workflows, credentials and external calls while connecting
the HTTP route. The [integration boundaries](docs/integration.md),
[embedded contract](docs/embedded-design.md) and [release procedure](docs/release.md)
describe consumer responsibilities and further validation.

Choose [AGPL-3.0-only](LICENSE) or a [commercial license](COMMERCIAL-LICENSING.md).
A commercial agreement permits proprietary use without the AGPL source-disclosure
obligations for the covered project code. Contributions follow the
[rights requirements](CONTRIBUTING.md), and dependencies retain their
[third-party licenses and notices](THIRD-PARTY-NOTICES.md).

Without `source_url`, the root response reports `source_status:"not_configured"`.
For an AGPL distribution, provide the corresponding source and configure its
version-specific HTTPS location under the [release procedure](docs/release.md).

See [development direction](docs/roadmap.md) for scope and
[the development workflow](docs/github-workflow.md) for contribution checks.
