<a id="provider-플러그인-계약"></a>

# Provider plugin contract

[English](provider-plugins.md) | [한국어](ko/provider-plugins.md)

The `gateway-provider/v1` contract describes a separately implemented parser for a
provider's JSON and SSE semantics. Package declarations can be inspected and
installed, and independent protocol examples can be tested. **Production provider
route activation remains unavailable.** Protected continuity and usage provenance
must be integrated before a provider route is exposed as supported. Codec v3 keeps
its existing built-in API meanings; it does not implement this role.

The [wire schema](../schemas/gateway-provider-v1.schema.json) and
[portable types](../crates/plugin-contract/src/provider.rs) describe the same
language-independent messages. Schema validity alone does not establish message
order, numeric validity, host compatibility, output validity or execution trust.
Package v2 declarations follow the [authoring specification](plugin-authoring.md).

<a id="프로세스와-메시지-수명-주기"></a>

## Process and message lifecycle

The host starts an explicitly trusted native executable with a private working
directory, empty inherited environment, stdin/stdout IPC and discarded stderr.
Native execution is not an OS sandbox. The host owns cancellation and kills/reaps
the direct child when the request ends; the wire has no cancellation ACK or retry.
Frames have a four-byte unsigned big-endian byte length followed by UTF-8 JSON.
The host's absolute frame cap is 128 MiB; configured payload limits also apply.
Startup and each complete exchange have separate three-second deadlines. Prefix,
body, write and read belong to the same exchange deadline.

An unsolicited `ready` reply uses sequence zero and exactly repeats the package's
`provider_protocol` and `capabilities`. Every envelope uses `gateway-provider/v1`.
Host requests start at sequence one and increase without wrapping; replies echo
the exact sequence. Duplicate keys, unknown fields, invalid versions, mismatches,
unexpected result variants and malformed framing fail the session.

1. `prepare` carries an admitted Responses request, a route snapshot, explicit
   `continuation`, and `max_request_bytes`/`max_output_bytes`. `prepared` returns
   a JSON object using the provider's own keys. A root `model` key is not required.
2. JSON processing uses `json` with the raw provider body string and a host-issued
   `response_id`, then one `completed` result.
3. SSE processing uses `stream`, then `event` calls containing SSE event/data
   strings. Each result is `progress`; `complete: true` marks semantic termination.
   The host then sends exactly one `finish` and receives `completed`.
4. Premature finish, events after termination, duplicate completion and EOF without
   semantic completion are errors. No fallback or silent stream degradation applies.

`progress.events` contains only supported nonterminal Responses text/reasoning
progress. Final response, item/tool completion and function arguments are released
only through final validated output and the host's recording/continuity barriers.
The final response must agree with emitted progress, the admitted model and tool
contract. `outcome` is `completed`, `awaiting_tools` or `incomplete`.

The route includes provider protocol identity, model, profile identity/version,
support mapping and explicit editing selection (`none` or `enabled` with policy).
It contains no endpoint, HTTP headers, credentials, transport callback, storage
handle or encryption key. The host selects the fixed HTTP destination and auth.
Transport-looking names inside the returned payload remain ordinary JSON data.
The fixed rejection codes are `unsupported_request`, `invalid_upstream`,
`unsupported_state` and `resource_limit`; arbitrary error/body text is not returned.

The initial host route rejects editing and compatibility-policy selection even if a package declares editing. The editing wire alternative is reserved for separately supported host integration; this example advertises no editing support.

<a id="사용량과-상태-값"></a>

## Usage and state values

Usage is explicitly `unobserved` or `observed` with seven mandatory counters:
`input_tokens`, `output_tokens`, `total_tokens`, `input_regular_tokens`,
`cache_read_input_tokens`, `cache_write_input_tokens`, `reasoning_output_tokens`.
Each counter is `reported` with an exact unsigned 64-bit integer, `not_reported`,
`not_applicable`, or `invalid`. Zero is a reported value; missing data never means
zero. Numeric fractions, negatives and overflow cannot become reported integers.
Schema validators may treat `1.0` as an integer; wire validation must additionally
preserve the exact JSON numeric representation required by the host.

Only the host derives counters or attaches package/parser provenance and request,
attempt or commit identity. Subset/total arithmetic and cumulative snapshots are
validated before successful output. An unknown provider is never relabeled as a
built-in usage parser. Cache TTL detail buckets are outside this wire version.

Continuation is `stateless` or `managed` with `pending_tools` and history spans.
Spans carry start/end indices and opaque state. State results explicitly select
`none` or `opaque`; fields cannot be omitted in place of those alternatives.
Opaque state has a format label, positive unsigned 32-bit version and canonical
padded standard base64. The host enforces a one-MiB decoded bound; schema pattern
validation does not prove canonical pad bits or the decoded size. The host owns
protection, persistence and exact route/session/package binding. This contract
provides no authority to approve sessions or migrate a package's stored state.
The current host rejects managed provider requests; protected persistence and resume are not active. The declaration and independent example do not establish protected restart support.

<a id="독립-예제와-검증"></a>

## Independent example and verification

Copy the entire [synthetic provider example](../tools/plugin-conformance/examples/provider/README.md)
directory elsewhere. Its builder, source, license and package creation tool need
only an explicitly provisioned Python 3.11+ interpreter. Installation does not
download an interpreter or dependencies. The executable pins the build interpreter's
absolute path; the target machine must provide that interpreter at the same path.

The example uses arbitrary `query`/`answer` shapes, SSE text pieces, function-call
results, explicit unknown/zero usage values and a versioned opaque counter. Its
wire restart test is separate from host-encrypted persistence. Tests use only
synthetic inputs and do not qualify any real provider, native sandbox or production
route. The standalone conformance runner currently reports provider execution as
unavailable; its static package result is not a provider execution certificate.
