<a id="http-지원-계약"></a>

# HTTP support contract

[English](protocol.md) | [한국어](ko/protocol.md)

The gateway accepts Responses requests and routes them to Responses, Messages
or Chat Completions, with an opt-in [Gemini Interactions route](interactions.md).
This page defines authentication, request handling, limits and failures. The original
three routes are stateless; Interactions adds durable provider continuation. Tools,
approval and Codex history belong to the host application. The [IR reference](ir.md) describes the Rust library types.

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
provider key forms a Bearer, `x-api-key` or `x-goog-api-key` header according to declared `auth`,
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

Requests must be JSON objects with a registered string `model`. A Responses route without
a selected compatibility policy replaces the model with its upstream name, preserving other JSON values except
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
IDs from the provider ID and retain original tool call IDs. Interactions instead
uses a durable local attempt ID and stores encrypted replay state. No route exposes
public response lookup or resolves previous_response_id. Provider error bodies can echo prompts
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

<a id="checked-responses-tools"></a>
<a id="명시적-도구-호환성-정책"></a>

## Explicit tool compatibility policies

A model selects a versioned policy with `compatibility_policy`; merely defining
`[compatibility_policies.NAME]` does not activate it. An explicit `auth` and
`capability_profile` are required. The provider profile declares support;
the policy selects gateway transformations. These declarations do not qualify
a real provider. See the [configuration example](../config.checked-responses.example.toml).

| Policy field | Values | Effect |
|---|---|---|
| `version` | `1` | Reject unknown policy versions |
| `tools.custom_input` | `preserve`, `function_json` | Preserve custom input or wrap its exact string in a function JSON object |
| `tools.namespaces` | `preserve`, `flatten` | Preserve namespace groups or map members to collision-free function names |
| `tools.grammar` | `preserve`, `registered_output_validation` | Preserve the declared format or validate a registered grammar's output locally |

Omitted choices inherit existing bridge declarations. An explicit choice must
agree with an existing bridge; a conversion conflicting with declared native
support is rejected. A policy does not turn unsupported preservation into native
support. Function wrapping and namespace flattening require native function-tool
support. Grammar validation requires either the custom JSON wrapper or native
Responses custom input. Wrapping cannot retain native custom grammar generation.

Selecting a policy on a Responses route enables checked admission and conversion.
Without a selected policy, the original Responses value/byte forwarding contract
remains in effect. Messages, Chat Completions and Interactions use the same selected
tool rules within their existing protocol and continuation contracts.
The request-scoped registry handles definitions, descriptions, named choices,
history calls/results and returned identities together; it never executes tools.

| Checked Responses surface | Support and limits |
|---|---|
| Requests/history | Declared text/images, instructions, function/custom tools and matched string results; unknown semantic fields and opaque history reject |
| Output controls | Declared output limits, sampling, reasoning options and structured formats retain their values; schema validation remains a host responsibility |
| Custom format | Omitted format, exact text format, or the registered Codex patch grammar; unknown grammars reject before dispatch |
| JSON output | Validate response identity, model, item/call uniqueness, tool choice, parallel count, wrapper shape and usage counters before delivery |
| SSE output | Validate event sequence, item/part lifecycles, deltas, done values and the terminal output; text and public reasoning summaries progress incrementally |
| Tool completion | Hold tool lifecycle events until the complete terminal validates; with durable usage recording, also wait for its final local commit |
| Unsupported output | Opaque reasoning, nonempty annotations/logprobs, unknown items/events, inconsistent or unfinished terminal items reject explicitly |

The checked route is stateless. It retains recognized metadata/cache hints and the
optional `include:["reasoning.encrypted_content"]` request hint, but does not
accept encrypted reasoning output or replay. Select a separately supported managed
route when opaque continuation is required. Public reasoning summaries require
their declared capabilities. Message `phase` preserves `commentary`,
`final_answer` or null across output and history. A failed stream closes without
a synthetic success. The checked event subset follows the
[Responses streaming reference](https://developers.openai.com/api/reference/resources/responses/streaming-events).

The grammar rule validates syntax after generation; it does not provide constrained
decoding, approval or file applicability. It preserves the grammar in the lowered
description and restores the original tool definitions in response echoes. Buffer
and event limits apply to checked output, so large tool calls can fail before
completion is exposed. No policy retries, chooses a fallback or weakens a rule.

<a id="자원과-실패"></a>

## Resources and failures

TOML `[limits]` defines the following independent limits. The time settings use
milliseconds; body settings use bytes.

| Setting | Default | Scope |
|---|---|---|
| `max_request_bytes` | 8388608 (8 MiB) | Complete incoming request body |
| `max_response_bytes` | 16777216 (16 MiB) | Buffered upstream JSON; for converted SSE, each event and aggregate output; not a total native-SSE byte cap |
| `max_in_flight` | 32 | All active model requests in this gateway instance, across every alias and provider |
| `request_body_timeout_ms` | 30000 | Total time spent receiving the local request body, not a per-chunk idle timer |
| `connect_timeout_ms` | 10000 | Establishing the upstream connection |
| `response_header_timeout_ms` | 60000 | Upstream send through receipt of response headers, including connection and request transmission |
| `stream_idle_timeout_ms` | 60000 | Waiting for the next upstream body chunk after headers, for both JSON and SSE |
| `shutdown_grace_ms` | 5000 | Graceful HTTP shutdown before the server task is stopped |

Capacity is acquired before reading the model request body. A non-streaming
request releases it after the upstream body has been buffered and processed; a
streaming response holds it until consumed or closed. There is no waiting queue:
excess requests receive `429 capacity_exceeded` without an upstream call. An open
stream retains its slot; slow downstream consumption drives slower upstream polling.

Body data, including heartbeat bytes, can keep the idle timer from expiring.
None of these settings imposes one overall deadline for the complete model call.
The host must own that deadline and cancellation. Native streams are not fully
buffered. Converted routes retain bounded text/tool input for final output and
restore custom wrappers after validation.

When a consumer disconnects, upstream reading ends. Graceful server shutdown
allows a bounded window for active requests. Neither guarantees cancellation of
already-processed provider work or a refund.

The gateway performs no implicit retries or fallback. Local authentication,
common request validation and route-admission failures occur before a provider
request. Connection errors, timeouts, oversized bodies and provider HTTP errors
are not converted into success. Consumers own retry policy and execution records
for uncertain outcomes.

Logs contain operational metadata such as request/route identifiers, status and
duration. Keep bodies, prompts, credentials and complete HTTP header dumps out of logs.

<a id="게이트웨이-오류-응답"></a>

## Gateway error responses

Gateway-generated API errors use this JSON envelope. The HTTP status is carried
separately; `error.code` distinguishes failures that share a status. Errors from
the HTTP stack or client transport need not have this envelope.

```json
{
  "error": {
    "code": "unauthorized",
    "message": "A valid local bearer token is required",
    "type": "gateway_error"
  }
}
```

In this table, an upstream call means the gateway attempted provider HTTP
communication, not that the provider executed or billed model work.

| HTTP status | `error.code` | Stage | Upstream call | Action |
|---|---|---|---|---|
| 401 | `unauthorized` | Local authentication | No | Correct the local Bearer token |
| 404 | `not_found` | Endpoint routing | No | Use a supported gateway path |
| 404 | `model_not_found` | Alias lookup | No | Select a registered model alias |
| 400 | `invalid_body`, `invalid_json`, `invalid_request`, `invalid_model` | Request body or envelope | No | Correct the JSON object, model and field types |
| 400 | `unsupported_feature` | Stateless policy | No | Remove unsupported storage/state requests and send current context |
| 400 | `unsupported_request` | Route limits, profile or conversion admission | No | Check required features, limits and complete tool history |
| 408 | `request_timeout` | Local body reception | No | Finish the upload within the request-body deadline |
| 413 | `request_too_large` | Local body reception | No | Reduce request bytes or choose a suitable explicit limit |
| 415 | `unsupported_media_type` | Local content type | No | Send application/json |
| 429 | `capacity_exceeded` | Local admission | No | Bound consumer concurrency and close finished responses |
| 501 | `unsupported_endpoint` | Stored-response endpoint | No | Use host-owned history instead of lookup/delete/remote compaction |
| Provider status; redirects become 502 | `upstream_error` | Provider response headers | Yes | Check the selected provider/key and status without automatic replay |
| 502 | `upstream_unavailable` | Upstream transport | Possible | Check connectivity and retain an uncertain attempt |
| 504 | `upstream_timeout` | Upstream send or buffered-body read | Possible or already received headers | Identify the waiting phase and preserve the outcome before retrying |
| 502 | `upstream_content_encoding`, `upstream_content_type` | Provider response headers | Yes | Verify identity encoding and the expected JSON/SSE content type |
| 502 | `upstream_read_error`, `upstream_response_too_large` | Buffered response body | Yes | Check transport completion or response byte budget |
| 502 | `upstream_invalid_json`, `upstream_invalid_response` | Native JSON or converted response validation | Yes | Verify the selected provider's response contract |

For example, local `401 unauthorized` differs from `401 upstream_error`, and
local `429 capacity_exceeded` differs from `429 upstream_error`. A received error
after dispatch does not establish whether provider work occurred. The gateway
suppresses provider error bodies and arbitrary response headers, including
`Retry-After` and provider request identifiers.

The gateway generates a fresh `x-request-id`, returns it to the consumer and sends
that generated ID upstream. Match it to the local `request_id` log field; a
consumer-supplied request ID is not reused. This ID does not provide deduplication,
response lookup or a safe-retry guarantee.

After SSE headers are sent, transport or conversion failure closes the stream
instead of replacing its status with one of these JSON errors. Inspect terminal
events and the local body-close outcome. See [troubleshooting](troubleshooting.md)
for diagnosis and [stream consumption](usage.md#read-a-stream-and-handle-cancellation)
for application handling.

<a id="선언된-경로와-기능-프로필"></a>

## Declared routes and capability profiles

Models can declare `api`, `auth`, `capability_profile` and `messages_version`.
The default API is `responses` with Bearer authentication. Messages and Chat
Completions require explicit capability profiles. The [three-route matrix](conformance.md)
lists their conversion features and mock-provider test coverage.

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
Responses retains unqualified passthrough. Profiled Responses routes preserve original JSON/SSE unless a compatibility policy
is selected; selected policies also enable feature-by-feature semantic admission.

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
contains `schema`, `manifest_schema` and `configuration_sha256` alongside its
address and version. There is no HTTP management endpoint. The [embedded contract](embedded-design.md)
defines the schema and host responsibilities.

[Chat Completions support](chat-completions.md) describes the function-tool format
and explicit custom-text conversion.

[External API codecs](api-codecs.md) may implement these existing API contracts
through explicit native package selection. They retain checked admission and
core-owned transport, tool validation, accounting and continuation. Unsupported
features remain errors; a codec grant does not extend the support matrix.

Explicit [editing compatibility](editing-design.md) additionally admits the pinned
Code Mode text-part result contract. It preserves the whole program result through
a selected bridge; unrelated structured tool results remain unsupported.
