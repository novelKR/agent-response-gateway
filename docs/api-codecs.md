<a id="ipc-수명-주기와-검증"></a>
<a id="변경과-복구"></a>
<a id="외부-api-codec"></a>
<a id="책임과-신뢰"></a>
<a id="패키지와-경로-선택"></a>

# External API codecs

[English](api-codecs.md) | [한국어](ko/api-codecs.md)

Select a reviewed native package to translate a complete model API through
versioned IPC. The reference package supports Responses, Messages, Chat Completions
and Gemini Interactions using the same codec library as built-in dispatch.
The host gateway owns admission, HTTP transport, credentials, final validation,
usage recording and durable continuation storage.

## Responsibilities and trust

A codec receives an admitted Responses request, effective capability declarations
and authenticated native replay spans. It returns a provider JSON payload,
restored Responses output or public progress, and versioned native replay.
It receives no provider URL, authentication header, environment credential,
session token, encryption key, database handle or host storage path over IPC.
The reference codec performs no HTTP requests, tool execution or database writes.
There is no callback for these operations, arbitrary transformation script API,
retry, fallback, implicit restart, download or hot reload.

Native packages are trusted code, not an OS sandbox. Clearing the environment and
restricting the protocol does not stop malicious same-user native code from using
its own OS privileges. Review the executable and its provenance before granting
`read_model_payload` and `transform_model_protocol`. Payload access is substantially
more sensitive than an HTTP metadata observer. The package must match a supported
Linux or macOS target. Windows rejects native codec activation; ordinary gateway
operation and non-executable profile packs remain available.

The core rechecks the executable digest immediately before spawning a dedicated
process for each request. Input and output use socket-backed standard streams;
stderr is discarded and the inherited environment is cleared. The private
per-package directory is only a working directory, not a codec database contract.
Package files and that directory follow the existing native-extension ownership
and permission checks. Concurrent hostile modification by the same owner is
outside this trust model.

## Package and route selection

Build the reference executable from the exact reviewed gateway source:

```sh
cargo build --locked
cargo build --locked --example api_codec
python3 -B scripts/extension_manager.py package \
  --binary /absolute/path/target/debug/examples/api_codec \
  --license-file /absolute/path/LICENSE --output /absolute/path/codec-package \
  --id reference-codec --version 1.0.0 --role api_codec
```

Use the existing offline [install and enable procedure](extensions.md) with the
exact printed package digest and both codec grants. The package uses
`gateway-extension-package/v1`, protocol `gateway-api-codec/v1` and
`request-memory/v1` as its state contract. Installation remains inactive;
activation alone does not route traffic to a codec. Existing package size, notice,
platform and immutable-installation limits apply. The example command includes
the project license for local testing; redistribution also requires corresponding
source and dependency notices for the exact binary under the [release procedure](release.md).

```sh
python3 -B scripts/extension_manager.py install \
  --store /absolute/path/store --package /absolute/path/codec-package \
  --expected-sha256 <exact-package-sha256>
python3 -B scripts/extension_manager.py enable \
  --store /absolute/path/store --id reference-codec --version 1.0.0 \
  --package-sha256 <exact-package-sha256> \
  --grant read_model_payload --grant transform_model_protocol
```

Set `api_codec = "reference-codec"` on each intended model and pass
`--extensions-lock /absolute/path/store/active.json` to `check-config`, `manifest`
and `serve`. The model must also declare its capability profile and authentication.
Responses routes require a selected tool compatibility policy so admission is
checked. Models without `api_codec` retain built-in dispatch; an unresolved codec
selection is an error. Profile-pack imports and codec selection may be combined.
An external codec implements the supported API contracts; it does not add API
enum values or grant unsupported features.

Offline inspection verifies bytes without executing the codec. Selected routes
use `gateway-embedded-manifest/v6` and `gateway-ready/v6`; the extension wrapper
uses `gateway-extended-manifest/v6` and `gateway-extended-ready/v6`. The wrapper
also uses v6 when an activated codec is unused. Each selected route includes the
package identity, executable hash, protocol, replay version and grants. Package
identity also participates in the route adapter identity. Hosts must understand
the schema and compare inspected/readiness digests before starting their agent.

## IPC lifecycle and validation

Each frame is a four-byte unsigned big-endian length followed by UTF-8 JSON.
Frames are bounded to 128 MiB, while request/response and event accumulation still
obey configured gateway limits. Duplicate keys, unknown fields and unsupported
versions fail. Replies echo `protocol` and the exact request `sequence`.
The initial ready reply uses sequence zero and declares all four supported APIs
and native replay version one. Later sequences increase by one.

The [typed contract](../src/codecs/contract.rs) defines `Request.operation` and
`Reply.value`. The first operation is `prepare`; the next selects either `json`
or `stream`. Streaming sends one parsed provider SSE `event` at a time and ends
with `finish`. The gateway owns SSE byte framing and HTTP cancellation. The
reference engine has no Rust ABI boundary; external implementations use the
versioned JSON contract rather than internal Rust structs or memory layouts.

Startup and each IPC exchange have a three-second total deadline, including
partial reads and writes. Failure poisons the request. There is no retry or
replacement process for that attempt. Dropping a session kills and reaps its
direct child, including on cancellation and interrupted provider transport.
The native trust contract forbids daemonization; this is not descendant-process
containment. Request-per-process execution adds startup and digest-check cost.

The core independently verifies restored tool identities, choices, argument JSON,
registered grammar output and completed Responses event lifecycles. Executable
arguments remain behind terminal validation and the configured recording barrier.
To preserve item order, a stateless tool and following output items are held until
validation, then published in original output-index order. Text preceding tools
can progress immediately. Managed progress permits only public text and reasoning;
its displayed text must match final output. Native replay is separate from public
Responses output and is never emitted as a client-owned state handle.

Usage comes from the actual provider bytes observed by the core. Codec usage is
compared against those observations; the core retains numeric provenance and
allowlisted provider identity. A codec cannot fabricate recorder success or a
durable continuation token. Core continuation admission, protection, finalization,
recovery and existing replay formats remain authoritative.

## Change and recovery

Disable the selected package, enable the reviewed exact replacement, inspect the
manifest and restart the gateway. A changed package identity invalidates the old
route origin; do not silently resume its previous session with different code.
Rollback explicitly selects retained earlier bytes. Existing storage and replay
recovery procedures still apply; there is no codec-owned migration or second
database. A malformed codec reply or process failure may leave a core-owned
attempt requiring ordinary host reconciliation.

The tests combine the shared reference implementation with independent framing,
sequence, size, EOF, deadline, public-output and process-reaping checks. Actual
Codex scenarios use synthetic providers and exercise built-in and external routes,
tool order, grammar failure, cancellation, managed replay and usage-recorder
barriers. These checks do not qualify a live provider or approve a third-party
codec's native privileges.
