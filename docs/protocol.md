<a id="http-지원-계약"></a>

# HTTP support contract

[English](protocol.md) | [한국어](ko/protocol.md)

This contract describes native Responses forwarding and declared Messages / Chat
Completions conversion. It does not qualify model output semantics, Codex tool
execution, compaction/resume or consumer approval. The separate [IR v1](ir.md) is
an internal library contract, not an automatic extension of HTTP support. All
three routes share stateless admission rules.

<a id="설정과-인증"></a>

## Configuration and authentication

The process reads TOML shaped like `config.example.toml`. Unknown configuration
keys, unregistered provider references and invalid limits fail before startup.
Configuration is not reloaded automatically; applying changes requires a restart.

The listener accepts numeric loopback addresses only, defaulting to `127.0.0.1:0`.
Provider URLs require HTTPS, except HTTP on numeric loopback hosts for mock
servers. URL user information, query and fragment are forbidden. Operators choose
addresses and model routes; a request body cannot add a provider. Ambient HTTP
proxies and redirects are disabled.

The local token is whitespace-free ASCII, 32–4096 characters, and must differ
from provider keys. Consumer `Authorization` is not forwarded. The selected
provider key forms a Bearer or `x-api-key` header according to declared `auth`,
defaulting to Bearer. Consumer cookies and arbitrary headers are not forwarded.
There is no browser CORS support or public-service authentication.

<a id="엔드포인트"></a>

## Endpoints

| Request | Authentication | Meaning |
|---|---|---|
| `GET /` | None | Name, version, license and optional `source_url` |
| `GET /healthz` | None | The process responds |
| `GET /readyz` | None | Local startup configuration is ready; `provider_probe:false` |
| `GET /v1/models` | Local Bearer | Configured model aliases |
| `POST /v1/responses` | Local Bearer | One attempt through the registered route |

Readiness checks neither actual provider-key validity, network connectivity,
model availability nor qualification. The root's source URL is not the result of
an online availability check either.

<a id="요청응답"></a>

## Requests and responses

Requests must be JSON objects with a registered string `model`. Native Responses
replaces the model with its upstream name, preserving other JSON values except
for the restrictions below. Messages and Chat Completions translate the registered
feature subset and reject unsupported fields before sending. The original-value
rules in this table describe the native route. `stream:true` selects SSE; omission
or false selects JSON.

| Input | Handling |
|---|---|
| `store` omitted or false | Send `store:false` upstream |
| `store:true` | Unsupported error |
| Non-null `previous_response_id`, `conversation`, `context_management` | Unsupported state/compaction error |
| `item_reference` in `input` (including ID references with omitted/null type), `compaction`, `compaction_trigger` | Unsupported stored-item reference/compaction error |
| `background:true` | Unsupported error |
| Function/custom tools, structured output and reasoning fields | Preserve JSON values; real-provider support needs separate qualification |

Native routing does not store or translate response IDs or rewrite the model
field of successful responses. Converted routes derive Responses response/item
IDs from the provider ID and retain original tool call IDs. None of the routes
stores responses or resolves a response ID. Provider error bodies can echo prompts
or credentials, so the gateway returns a local error and HTTP status instead.
Redirects produce 502. Successful response and SSE bodies are delivered; do not
interpret this contract as shared-service or tenant isolation.

Native SSE bodies pass through without collecting the whole stream or reinterpreting
JSON. Converted routes parse SSE incrementally and emit validated Responses events.
Chunks need not align with events, characters or JSON. A failure after streaming
begins closes the connection without appending another provider's response or
inventing successful completion. Consumers must not treat a stream ending without
a terminal completion event as successful. The gateway requests
`Accept-Encoding: identity` and rejects compressed bodies with 502. Arbitrary-
precision JSON parsing preserves large integer and decimal values.

<a id="자원과-실패"></a>

## Resources and failures

Defaults are an 8 MiB request, 16 MiB non-streaming response, 32 concurrent
requests, 30-second request-body timeout, 10-second connect timeout, 60-second
response-header timeout, 60-second stream idle timeout and 5-second shutdown grace.
Exact values are configured in TOML `[limits]`. Native streams are not fully
buffered. Converted routes retain bounded text/tool input for final Responses
output; max_response_bytes bounds each SSE event and aggregate output. Custom
wrappers are restored after validation.

When a consumer disconnects, upstream reading ends. Graceful server shutdown
allows a bounded window for active requests. Neither guarantees cancellation of
already-processed provider work or a refund.

The gateway performs no implicit retries or fallback. Authentication failures,
invalid inputs and unregistered models fail before a provider request. Connection
errors, timeouts, oversized bodies and provider HTTP errors are not converted
into success. Consumers own retry policy and execution records for uncertain outcomes.

Logs contain operational metadata such as request/route identifiers, status and
duration. Never log bodies, prompts, credentials or headers.

<a id="선언된-경로와-기능-프로필"></a>

## Declared routes and capability profiles

Models can declare `api`, `auth`, `capability_profile` and `messages_version`.
Legacy configuration remains `responses` with Bearer authentication. Messages
can be enabled with explicit profiles following G09's pinned-Codex/synthetic-HTTP
verification. Chat Completions can also be enabled following G12's common suite.
The [three-route matrix](conformance.md) distinguishes the verified subset from
unverified real-provider and consumer acceptance.

A profile's provider, upstream model and API must match its model mapping. Its
key is its ID; declare `version`, `tested_codex_version`, `context_window` and
`max_output_tokens`. These are operator declarations, not automatic runtime or
provider qualification. `support` maps feature names to `native`, `unsupported`
or an implemented bridge; omitted features are Unsupported. namespaced_tools can
declare `bridged_tool_namespace`, and custom_grammar can declare
`bridged_codex_patch_grammar`. Both require native function support. custom_tools
can declare the JSON bridge, and Messages instruction_hierarchy can explicitly
declare the approved bridged_instruction_envelope. See the optional profile in
`config.example.toml`.

Profiled routes reject requested `max_output_tokens` unless it is a positive
integer within the declared bound. Input-token counting is not implemented or
qualified; `context_window` is a host-configuration contract. Unprofiled native
Responses retains unqualified passthrough. Profiled native routes still preserve
original JSON/SSE; feature-by-feature semantic admission applies to converted routes.

Each HTTP request fixes its resolved provider, model, API, credential reference,
profile and limits once. An environment-variable name selects credentials for that
request; it is not a credential generation proving resumable history. Persistent
resume follows the separate continuity contract. Converted admission excludes
only declared client_metadata, prompt_cache_key and optional encrypted-reasoning
output requests as transport hints. Unknown extensions and opaque inputs reject.
[Messages](messages.md) and [Chat](chat-completions.md) document namespace/grammar
bridges and actual-Codex synthetic coverage. Real-provider model qualification is separate.

<a id="오프라인-내장-계약"></a>

## Offline embedded contract

`manifest --config` produces normalized JSON and a configuration digest using
the same validation and route resolution as the server. It does not access a
listener, environment key values or a provider. The first `serve` readiness JSON
adds `schema`, `manifest_schema` and `configuration_sha256` to the existing fields.
There is no new HTTP management endpoint. The [embedded contract](embedded-design.md)
defines the schema and host responsibilities.

Chat Completions provides request, JSON response and streaming codecs with an
explicit HTTP route. Its [initial profile](chat-completions.md) distinguishes the
function-wire subset from the explicit custom JSON bridge.
