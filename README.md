# agent-response-gateway

[English](README.md) | [한국어](README.ko.md)

An independent Responses gateway written in Rust. It maps public model aliases to
provider model names and forwards JSON or SSE using separate upstream credentials.
Agent runtimes and backend services can use the same HTTP interface.

The current implementation provides **loopback-only Responses → Responses
forwarding and explicitly profiled Responses → Messages / Chat Completions
conversion**. Tool, approval-denial and cancellation tests pass with a pinned
Codex executable and synthetic upstreams. Embedding, continuity and signed-candidate
adoption contracts and consumer-side synthetic integration are implemented.
Real-provider qualification, consumer production activation and formal release
remain separate acceptance stages.

<a id="시작하기"></a>

## Getting started

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
contains one readiness JSON line; logs go to stderr. The embedded contract adds
schema, manifest_schema and configuration_sha256 to the original fields. A host
compares this digest with the offline manifest, which contains no key values.

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

<a id="지원-범위"></a>

## Supported behavior

| Interface | Behavior |
|---|---|
| `GET /` | Version, license and configured source location |
| `GET /healthz` | Local process liveness |
| `GET /readyz` | Local readiness; no real-provider probe |
| `GET /v1/models` | Configured model aliases after Bearer authentication |
| `POST /v1/responses` | Model mapping, provider-credential replacement and JSON/SSE forwarding after Bearer authentication |

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

The [implementation milestones](docs/roadmap.md) record priorities, dependencies
and remaining acceptance criteria. The [GitHub workflow](docs/github-workflow.md)
defines PRs, commits and CI. Plans and current support remain distinct.

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
the same Rust 1.98.0 checks on Linux and macOS. Verify the relevant commit in the
[public CI runs](https://github.com/novelKR/agent-response-gateway/actions/workflows/ci.yml).
CI success is not real-provider qualification or operational acceptance.

The separate [Codex conformance suite](tests/codex/README.md) connects the actual
Codex and gateway to synthetic upstreams. The temporary baseline is
`0.154.0-alpha.6`; the `0.153.4` heartbeat cancellation failure and the conditions
for replacing it with stable `0.154.0` or later are recorded in the
[pinned contract](docs/codex-contract.md). It does not force a particular Codex
version on product use or replace real-provider and consumer acceptance.

<a id="통합과-라이선스"></a>

## Integration and licensing

An embedding application can pin a verified gateway release by version and hash,
then let its runtime manager supervise it alongside the agent. Backend services
retain ownership of workflows, credentials and external calls while connecting
the HTTP route. The [integration boundaries](docs/integration.md),
[embedded contract](docs/embedded-design.md) and [release procedure](docs/release.md)
describe consumer responsibilities and further validation.

Public source is [AGPL-3.0-only](LICENSE). [COMMERCIAL-LICENSING.md](COMMERCIAL-LICENSING.md)
describes a future separately negotiated license; it is not an alternative grant
or an executed agreement. Also read the [contribution policy](CONTRIBUTING.md)
and [third-party notice guide](THIRD-PARTY-NOTICES.md).

Without `source_url`, the root response reports `source_status:"not_configured"`.
An actual distribution must provide its corresponding source and configure a
verified HTTPS location for that version. Displaying a URL does not establish
fulfillment of every license obligation.

Public documentation contains reusable product contracts only. If local notes
use an independent repository, exclude it from the parent Git and distribution.
[Documentation management](docs/documentation.md) covers publication checks and
source delivery.

See [conformance](docs/conformance.md) for the three-route comparison and
the [configuration example](config.chat.example.toml) for Chat Completions.
