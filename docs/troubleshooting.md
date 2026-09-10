<a id="문제-해결"></a>

# Troubleshooting

[English](troubleshooting.md) | [한국어](ko/troubleshooting.md)

Identify the failing stage before changing keys, model settings or retry policy.
Use the HTTP status, gateway error code and request ID together. The
[HTTP contract](protocol.md#gateway-error-responses) defines the error format and
limits; [usage examples](usage.md) show working request shapes.

<a id="게이트웨이가-시작되지-않음"></a>

## The gateway does not start

| Symptom | Check | Cause | Action |
|---|---|---|---|
| Configuration is rejected | Selected TOML, provider/model references, API and profile bindings | Unknown fields, missing references or inconsistent declarations | Run the two offline checks below; compare with the selected configuration example |
| A credential variable is missing | Gateway child environment and every configured provider entry | Startup reads all registered provider keys, even unused entries | Supply those variables or remove entries you do not intend to configure; keep actual keys out of TOML |
| Local token is invalid | `ARG_LOCAL_TOKEN` or the name selected by `local_token_env` | Token is missing, too short, contains whitespace or matches a provider key | Use a separate 32–4096 character printable non-space ASCII token and pass the same local token to the consumer |
| Listener cannot bind | Configured numeric loopback address and port owner | Another process owns the port or the address is unavailable | Prefer `127.0.0.1:0` and use readiness; do not terminate an unrelated process |
| Ready message does not arrive | Child exit status, stderr, host startup deadline | Startup failed or the host is not draining/reading child pipes correctly | Stop the owned child on timeout, inspect its local error and follow the process lifecycle contract |

```sh
cargo run --locked -- check-config --config config.local.toml
cargo run --locked -- manifest --config config.local.toml
```

These commands validate configuration and report its normalized form without
reading credentials or contacting providers. A successful check does not prove
that `serve` has its environment variables. Environment or TOML edits affect the
next process start, not the running instance. See [startup and shutdown](embedded-design.md#process-lifecycle).

<a id="소비자가-접속하거나-인증할-수-없음"></a>

## The consumer cannot connect or authenticate

| Symptom | Check | Cause | Action |
|---|---|---|---|
| Connection refused | Readiness address and gateway process | Wrong/stale port, stopped child, or a different network namespace | Use the actual readiness URL and run the consumer in the supported local topology |
| Wrong path or 404 | Consumer URL and `error.code` | Provider endpoint used as the consumer endpoint, duplicate `/v1`, or unknown alias | Use readiness base plus `/responses` once; distinguish `not_found` from `model_not_found` |
| `401 unauthorized` | Exactly one `Authorization: Bearer ...` header with the local token | Gateway authentication failed before dispatch | Correct the local token; do not send a provider key in its place |
| `401 upstream_error` or `403 upstream_error` | Selected alias, provider key reference and that key's provider permissions | The selected provider rejected its credential or access | Correct the provider credential/permissions, then restart if the key changed |
| Browser CORS error or connection from another container fails | Browser origin and network namespace | Browser CORS and public/separate-container service deployment are unsupported | Use the supported same-host or same-network-namespace backend integration |

The gateway replaces consumer provider-authentication headers. A supplied
`x-api-key` does not select a different key. Use registered aliases as described
in [multiple keys](route-design.md#multiple-api-keys-for-one-provider). A local token
permits all aliases in that instance; it is not a per-user permission boundary.

<a id="모델에-도달하기-전에-요청이-거부됨"></a>

## A request is rejected before reaching the model

| Symptom | Check | Cause | Action |
|---|---|---|---|
| 400 with `invalid_json`, `invalid_request` or `invalid_model` | JSON syntax, root object, `model` and boolean fields | The common request envelope is invalid | Start with the minimal usage request, then add fields deliberately |
| `415 unsupported_media_type` | `Content-Type` | The request is not declared as JSON | Send `application/json` |
| `413 request_too_large` | Encoded request bytes and `max_request_bytes` | Complete context exceeds the byte limit | Reduce portable context or choose an appropriate explicit limit; token and byte limits differ |
| `400 unsupported_feature` | `store`, `background`, `previous_response_id`, conversation and stored-item references | Request asks the stateless gateway to store or recover state | Send complete current context with `store:false`; keep history in the host |
| `400 unsupported_request` | API/profile, output limit, client-added fields and complete tool history | Conversion, capability or profiled-limit checks failed | Compare the selected support matrix; remove an unintended option or select a verified compatible route |
| `501 unsupported_endpoint` | Response lookup/delete or compaction URL | Stored-response operations are not implemented | Use the host's history and continuity flow |

Adding a profile flag cannot enable unsupported conversion. For example, reasoning
summaries, `text.verbosity`, unfinished tool history or an unsupported schema may
fail before dispatch even if the real provider exposes a similarly named feature.
Do not silently remove an option required by the application. See
[request options](usage.md#request-options-and-response-values) and [route configuration](route-design.md#configuration).

<a id="요청에-429가-반환됨"></a>

## Requests return 429

| Symptom | Check | Cause | Action |
|---|---|---|---|
| `429 capacity_exceeded` | Active requests, `max_in_flight` and streams still open | The instance-wide capacity is occupied; the new request was not dispatched | Limit concurrent work at the consumer and close completed/cancelled responses |
| `429 upstream_error` | Selected provider/key and provider-side limits | The upstream rejected the attempt | Apply an explicit provider-aware policy after checking the failure; the gateway does not switch keys |

All aliases and providers in one gateway share the same capacity. There is no
waiting queue. A JSON request releases its slot after the gateway has buffered and
processed the upstream body. A streaming response holds its slot until consumed
or closed, including when the consumer is slow or has stopped reading. A different
alias does not create a separate capacity pool. Increase the limit only after
considering the application's concurrency and provider limits.

Provider `Retry-After` and rate-limit headers are not forwarded. The gateway does
not retry on your behalf. Keep a consumer-side concurrency/retry budget and do not
assume a failed network attempt proves the provider performed no work.

<a id="요청-지연시간-초과조기-종료"></a>

## A request stalls, times out or ends early

Use the [timeout and size table](protocol.md#resources-and-failures) to identify
the phase. Request-body timeout bounds local input reception. Connection and
response-header timeouts cover upstream dispatch. Body idle timeout applies to
both JSON and SSE after headers; it is not an overall request duration limit.

| Symptom | Check | Cause | Action |
|---|---|---|---|
| `408 request_timeout` | Consumer upload and request-body deadline | Request body did not arrive in time | Send the complete JSON within the configured budget |
| `504 upstream_timeout` before a body is delivered | Connection/header timing, then gaps in JSON body data | An upstream waiting limit expired | Identify the phase from the error message and logs; retain an uncertain outcome if dispatch may have occurred |
| SSE opens and later disconnects | Terminal event, read error and `upstream_body_closed` outcome | Idle limit, interrupted source, invalid conversion or truncated stream | Preserve partial output and treat the request as interrupted unless a valid terminal result was received |
| Heartbeats continue but useful output never arrives | Overall application deadline | Incoming bytes keep the body idle timer from expiring | Enforce a caller deadline and close the response when cancelling |
| Output is `incomplete` | `incomplete_details` and requested/profile output tokens | The model reached a declared output boundary | Retain the partial result and decide whether to continue or adjust the next request |

An HTTP 200 or a clean connection close is not proof of model completion.
Native SSE is passed through without semantic completion validation. Converted
streams can close after headers with no replacement JSON error. The basic client
prints bytes; use the [completion rules](usage.md#read-a-stream-and-handle-cancellation)
in an application. Cancellation or a timeout does not guarantee provider work or
charges were cancelled.

<a id="업스트림의-502-또는-사용할-수-없는-응답"></a>

## The upstream returns 502 or an unusable response

| Symptom | Check | Cause | Action |
|---|---|---|---|
| `502 upstream_unavailable` | Configured endpoint, DNS/TLS/connectivity in the gateway environment | Upstream transport failed | Verify the endpoint and supported HTTPS connection; ambient HTTP proxies are not used |
| `502 upstream_error` after a redirect | Provider base URL | Upstream sent a redirect that the gateway will not follow | Configure the verified final API prefix; do not append the API endpoint twice |
| `502 upstream_content_type` or `upstream_content_encoding` | Selected API and provider response format | Unexpected media type or compression | Verify JSON versus SSE behavior and identity encoding; do not treat HTML/login pages as model responses |
| `502 upstream_response_too_large` | Non-streaming JSON bytes and response limit | The buffered body exceeds its byte budget | Reduce expected output or set a suitable explicit limit |
| `502 upstream_invalid_json`, `upstream_invalid_response`, or `upstream_read_error` | Native JSON validity, converted response contract and transport completion | Malformed, unsupported or interrupted provider output | Keep the failure and verify that provider/API combination with synthetic input |

The same byte setting has different stream scope: native SSE has no aggregate
response-byte cap from this setting, while converted streams bound each event and
accumulated output. See the HTTP contract before changing it. A supplied capability
profile is not proof that a provider emits the required response format.

<a id="내용을-노출하지-않고-오류-추적"></a>

## Trace a failure without exposing content

For a safe local diagnostic, use a deliberately unregistered alias. This produces
a local failure without a provider call; `GATEWAY_BASE_URL` is the readiness base
from the usage guide.

```sh
curl --noproxy '*' -i "${GATEWAY_BASE_URL}/responses" \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"not-registered","input":"Synthetic diagnostic request.","store":false}'
```

Record the HTTP status, `error.code` and response `x-request-id`. Match that ID to
the gateway log's `request_id`; the gateway generates it and replaces any
consumer-supplied request ID. The error object's `type` is `gateway_error`.
For an active upstream body, the `upstream_body_closed` log also records the route,
elapsed time and outcome. Early local rejection need not produce that body log.

Provider error bodies and arbitrary upstream headers, including provider request-ID
headers, are not forwarded. Use the [error reference](protocol.md#gateway-error-responses) to determine
whether dispatch was possible. Keep prompts, response bodies, keys and headers out
of diagnostic logs and public reports. A status, local request ID, route alias,
phase and timing are sufficient starting points for investigation.
