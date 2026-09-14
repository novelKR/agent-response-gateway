<a id="네이티브-플러그인-작성"></a>

# Writing native plugins

[English](plugin-authoring.md) | [한국어](ko/plugin-authoring.md)

This specification describes the existing installable native roles. A plugin can be built in an independent repository and in any language that can implement the process and byte contracts below. Installing a compatible executable does not rebuild the gateway. No Rust ABI or gateway library import is required. Legacy roles retain their contracts; codec API subsets use the explicit capability contract below. Provider package declarations do not enable provider execution.

Read the [installation guide](extensions.md), [trust and lifecycle contract](extensions-design.md), [codec behavior](api-codecs.md) and [usage semantics](usage-accounting.md) with this specification. Native plugins are trusted programs running as the host user. A narrow IPC interface and an empty inherited environment are not an OS sandbox.

<a id="지원-역할-선택"></a>

## Choose a supported role

Package identity, package version and protocol version are separate. The `protocol` selects exactly one existing role; `permissions` and `state_schema` must match the row below, including permission order. Package version is three decimal components, each at most six digits, with no leading zero except zero itself. A version number does not declare a host version range.

```json
[
  {
    "protocol": "gateway-observer/v1",
    "permissions": [
      "observe_http_metadata",
      "write_private_state"
    ],
    "state_schema": "observer-state/v1"
  },
  {
    "protocol": "gateway-usage-recorder/v1",
    "permissions": [
      "export_usage",
      "observe_usage",
      "write_usage_store"
    ],
    "state_schema": "usage-store/v1"
  },
  {
    "protocol": "gateway-api-codec/v1",
    "permissions": [
      "read_model_payload",
      "transform_model_protocol"
    ],
    "state_schema": "request-memory/v1"
  },
  {
    "protocol": "gateway-api-codec/v2",
    "permissions": [
      "read_model_payload",
      "transform_model_protocol"
    ],
    "state_schema": "request-memory/v1"
  }
]
```

Observer sees only numeric HTTP metadata. Recorder sees usage events with the separately configured delivery mode. Codec transforms the already admitted request and provider response for the existing Responses, Messages, Chat Completions and Gemini Interactions APIs. Codec selection does not add an API enum, route, credential source or transport. A single-API codec cannot satisfy the legacy startup declaration; it requires codec v3 below.

The [legacy package schema](../schemas/gateway-extension-package-v1.schema.json) enumerates the current role combinations. Unknown roles or fields are errors. There is no implicit feature negotiation or optional-permission downgrade. Publish the exact supported host release and tested OS/architecture alongside your artifact; this metadata is release documentation, not extra manifest fields. Future incompatible wire changes require a separately supported protocol identifier.

<a id="버전이-명시된-기능-선언"></a>

## Versioned capability declarations

[Package v2](../schemas/gateway-extension-package-v2.schema.json) adds a required `capabilities` object and admits only `gateway-api-codec/v3` or `gateway-provider/v1`. Legacy package v1 remains restricted to the four roles above and forbids capability fields. Neither a new package version nor a manifest edit upgrades a legacy executable protocol.

Capabilities use `gateway-plugin-capabilities/v1` with exactly `apis`, `features`, `requires` and `schema`. All arrays contain unique strings in ASCII lexical order. Codec APIs are a nonempty subset of `chat_completions`, `gemini_interactions`, `messages`, `responses`. Features are a subset of `editing`, `json`, `managed_continuation`, `streaming`, with `json` required. Codec requirements are exactly `codec_ipc_v3` and `responses_output_validation`. The declarations are separate from the existing `read_model_payload` and `transform_model_protocol` grants and `request-memory/v1` state contract.

The [codec v3 schema](../schemas/gateway-api-codec-v3.schema.json) retains v2 operation and editing-policy semantics, but replaces the legacy ready fields with the complete capability object. Native replay version one is part of this contract; do not add legacy `apis` or `replay_versions` fields beside `capabilities`. The ready declaration must equal the inspected manifest, not merely overlap it. This example supports only Messages:

```json
{
  "protocol": "gateway-api-codec/v3",
  "sequence": 0,
  "value": {
    "result": "ready",
    "capabilities": {
      "schema": "gateway-plugin-capabilities/v1",
      "apis": [
        "messages"
      ],
      "features": [
        "editing",
        "json",
        "managed_continuation",
        "streaming"
      ],
      "requires": [
        "codec_ipc_v3",
        "responses_output_validation"
      ]
    }
  }
}
```

The selected model API must belong to `apis`. Each requested feature must be declared before preparation: streaming requires `streaming`, managed execution requires `managed_continuation`, and editing requires `editing`. These checks supplement the route capability profile and permission checks; a declaration cannot widen host admission. Offline inspect/install never executes a handshake. Unsupported requirements or mismatched startup declarations are explicit errors, with no downgrade or implicit protocol selection.

Provider packages declare `gateway-provider/v1`, the same payload grants, `provider-request-memory/v1`, empty `apis`, and requirements exactly `provider_ipc_v1` and `responses_output_validation`. They additionally require `provider_protocol` matching `[a-z][a-z0-9._-]{0,63}/v[1-9][0-9]{0,5}`. Codec packages forbid that field, including null. Provider declarations may be inspected, installed and inventoried, but provider activation and runtime execution are unavailable. Declared provider features are not evidence of implemented host provider support. No provider role messages or new supplier routing are accepted through codec v3.

The [capability vectors](../schemas/plugin-capabilities-vectors.json) cover valid declarations, ordering, missing/unknown requirements, forbidden legacy reinterpretation and the v3 ready shape. Exact manifest/ready equality, route membership and feature gates also require runtime tests; schema validity alone does not establish compatibility.

<a id="패키지-바이트와-실행"></a>

## Package bytes and execution

Distribute a flat directory containing `extension.json`, executable `extension`, `LICENSE.txt`, and any other declared notice/runtime files. The `files` map contains every file except the manifest, with lowercase SHA-256 values; unlisted files and subdirectories fail. There are 2–8 listed files. Names match ASCII `[A-Za-z0-9][A-Za-z0-9_.-]{0,63}` and cannot equal `extension.json`. The manifest `id` matches `[a-z][a-z0-9-]{0,63}`.

Manifest bytes are UTF-8 without BOM: object keys sorted ascending, no insignificant whitespace, decimal integers, and exactly one final LF. All permitted package strings are ASCII, so Unicode escape variants are not involved. SHA-256 of these exact bytes, including LF, is the package digest. This is the package canonical format, not a claim of RFC 8785 compliance. JSON Schema alone cannot check canonical bytes, duplicate keys, file inventory, filesystem identity or a trusted digest.

Manifest and activation JSON are each limited to 65,536 bytes; executable to 128 MiB; each other file to 256 KiB. Targets are `linux-x64`, `linux-arm64`, `macos-x64`, `macos-arm64`. Installation and execution require the actual host target. A container is not a substitute for testing a different host OS. Build output may be copied from a hard link by the packaging helper; distributed/installed package files must be regular files with one link. Symlinks in paths are rejected. The installed store is private and owned by the execution user.

The executable is started by absolute path with socket-backed stdin/stdout, empty inherited environment, private working directory and discarded stderr. Observer and codec receive no arguments; Recorder receives `serve`. No shell or language runtime is supplied. Use a native/self-contained binary or an explicitly provisioned absolute interpreter; a script using `/usr/bin/env` cannot rely on inherited `PATH`. The flat file and size limits still apply to bundled runtimes. Do not daemonize or spawn descendants.

Installation only verifies and copies bytes. Explicit activation records the exact package digest and grants; it does not execute code. Activation is a frozen next-start selection, with at most four selected packages across roles. Recorder binding uses activation v2; other selections use v1. Do not hand-edit host-managed activation files. Changes require a restart, and package replacement changes execution identity. Rollback selects retained bytes; it is not a data migration or restoration of old credentials. See [managed lifecycle](managed-extensions.md).

<a id="observer-프로토콜"></a>

## Observer protocol

The [Observer schema](../schemas/gateway-observer-v1.schema.json) defines each message. Write one UTF-8 JSON object and LF, then flush. Child sends ready first; host sends one observation at a time; child acknowledges the exact sequence. Sequence starts at one per process and increases by one without reuse. Emit integer fields as decimal digit tokens without a fraction or exponent. JSON integers must be exact unsigned 64-bit values; implementations with floating-point-only JSON numbers need lossless integer handling. Missing, extra, duplicate or mistyped reply fields are invalid.

```json
{"type":"ready","protocol":"gateway-observer/v1"}
{"type":"http","sequence":1,"status":200,"headers_ms":8}
{"type":"ack","sequence":1}
```

Ready must arrive within three seconds. Each observation write and acknowledgement shares one second, including partial I/O. Reply frames including LF are at most 4,096 bytes. Status is 100–599; timing is milliseconds until headers, not full model completion. Queue capacity is 64 per observer; full/disconnected queues drop observations. EOF, wrong sequence, malformed reply or timeout at startup rejects launch; after readiness it disables that observer. No restart or event replay occurs. Acknowledgement is receipt, not proof of durable storage.

<a id="recorder-프로토콜"></a>

## Recorder protocol

The [Recorder schema](../schemas/gateway-usage-recorder-v1.schema.json) references the complete [usage event schema](../schemas/gateway-usage-event-v1.schema.json). Recorder sends ready with its persistent `producer_id`. Gateway sends the usage event object directly, not an envelope. Recorder replies only after local commit with the same `event_id` and SHA-256 of exact event JSON bytes excluding the LF. Flush every reply.

```json
{"type":"ready","protocol":"gateway-usage-recorder/v1","producer_id":"synthetic-producer"}
{"type":"committed","event_id":"synthetic-event","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}
```

The zero digest above is a shape example, not an acknowledgement for an actual event. Labels use 1–200 ASCII characters from letters, digits, `-_/.:`. Events are at most 65,536 bytes before LF; replies at most 4,096 including LF. `ack_timeout_ms` is 1–60,000 and bounds startup and each complete write/ACK exchange; `queue_capacity` is 2–4,096. Both are host binding fields, not ready fields.

Host events use sorted object keys and compact UTF-8 JSON. Preserve exact bytes and exact integers; do not hash a pretty-printed or numerically rounded reconstruction. Validate the schema, producer identity, event/revision identity, timestamp ordering and the [normalization rules](usage-accounting.md). `attempt_started` has revision zero; later kinds have positive revision. A repeated identity with identical bytes is idempotent; conflicting bytes must not receive committed. Missing usage is unknown, never zero. Remote delivery is separate from local ACK.

Mode `off` does not launch the recorder. Mode `best_effort` permits delivery loss. Mode `durable_local` gates configured start/final boundaries on local commit; recorder failure does not retry model inference. A ready failure rejects the opted-in startup; later exit, invalid ACK or timeout makes the recorder unavailable. Do not claim client receipt, remote persistence or exactly-once provider billing from the ACK.

The working directory is the explicitly bound usage store, containing private `recorder.json` whose exact digest is selected by the host. Prepare storage with the chosen recorder before enabling it; installation runs no initializer. The [reference recorder configuration and storage procedure](usage-accounting.md) describes its local ledger and export destinations. An independent implementation must provide the declared local commit semantics and explicit storage preparation/recovery instructions. Do not open an existing reference ledger with an incompatible format or reinterpret its state schema as permission to migrate it.

<a id="codec-메시지와-상태-기계"></a>

## Codec messages and state machine

The [codec v1 schema](../schemas/gateway-api-codec-v1.schema.json) and [codec v2 schema](../schemas/gateway-api-codec-v2.schema.json) describe complete envelopes and nested contract fields. Each frame is a four-byte unsigned big-endian length, then exactly that many UTF-8 JSON bytes, with no LF framing. Length must be 1–134,217,728 bytes. Read exactly the prefix and payload even when the socket fragments them. Each startup and request/reply exchange has a shared three-second deadline.

```json
{"protocol":"gateway-api-codec/v1","sequence":0,"value":{"result":"ready","apis":["responses","messages","chat_completions","gemini_interactions"],"replay_versions":[1]}}
```

Legacy codec v1/v2 ready must declare exactly that API array in that order and replay version array. Use the selected protocol in every envelope. Requests begin at sequence one, increment by one, and each reply echoes its request sequence. JSON field order/whitespace is not canonicalized for codec framing; duplicate keys and unknown fields are rejected. Unsigned integers use exact 64-bit values; host-emitted size/index fields must fit the configured size limits. Strings contain no implicit identifiers or callback addresses.

```text
ready(sequence=0)
  -> prepare(sequence=1) -> prepared
  -> json(sequence=2) -> json [stateless] / managed [managed]
  OR
  -> stream(sequence=2) -> progress
  -> event(sequence=3...) -> progress
  -> finish(next sequence) -> finished [stateless] / managed [managed]
```

The first operation is `prepare`. Its `request` is an admitted Responses JSON object, and `route` contains `api`, `model`, `profile_id`, `profile_version`, `reasoning_contract`, `support`, `context_window`, `max_output_tokens`. Absent support entries mean unsupported. `managed`, `pending_tools`, `history`, `max_output_bytes` specify replay mode, pending control state, authenticated history spans and the output bound (1–67,108,864). Return `prepared.payload` as the provider JSON object; its model must equal the admitted model. It cannot choose an HTTP destination, method, credentials or headers.

The `json` operation carries provider body text and host `response_id`; `stream` starts a stream with that identity. Each `event` contains the host-parsed SSE `event` name and `data` text. `progress.events` contains Responses event objects; `complete` reports parser completion, not transport or recorder success. `finish` validates end-of-input, even after completion was reported. Stateless finish returns `finished`; managed finish returns the final managed object. `rejected`, invalid operations, out-of-order messages, early EOF, timeouts or invalid output poison that request without fallback or retry.

<a id="codec-중첩-타입과-불변-조건"></a>

## Codec nested types and invariants

Optional nullable fields permit missing or `null` according to each schema; host serialization normally includes explicit nulls except explicitly omitted extension fields. Codec v1 forbids the `editing` field entirely, including `null`. Codec v2 and v3 permit it; a non-null policy must satisfy the [editing contract](editing-design.md). Its `version` is one, separate from IPC versions two and three. No codec may infer an editing policy from tool names.

Schemas enumerate supported feature and bridge names. A bridge must match its feature: instruction envelopes bind instruction hierarchy and their API; custom-tool JSON binds custom tools; tool namespaces bind namespaced tools; code-mode text parts bind structured tool output; patch/registered grammar bridges bind custom grammar. Registered grammar requires native custom tools on Responses. Provider parallel permission requires Chat Completions with the explicit DeepSeek reasoning contract. Unsupported combinations fail admission; a plugin cannot widen them.

The `reasoning_contract` is the selected typed declaration, not provider discovery. DeepSeek and OpenRouter contracts require Chat Completions; Claude adaptive/manual require Messages. Version is one. Effort defaults must belong to their declared supported set; OpenRouter budget and effort modes are exclusive; manual Claude budgets are at least 1,024. Reasoning contracts require native reasoning summary/items. The [reasoning guide](chat-completions.md) specifies allowed effort/format combinations and wire behavior.

Every history span has `start`, `end`, `native`: nonoverlapping ascending half-open input-item indexes with start less than end and end within the request input array. Nonempty history requires managed mode; stateless mode forbids pending tools. Native replay version is one: `gemini_steps` carries nonempty `steps`; `chat_assistant` carries `dialect` (`deep_seek` or `open_router`), object `assistant` and object `controls`; `messages_content` carries nonempty `blocks` and object `controls`. These provider-native values are private state, not public Responses output. Preserve the selected API/dialect and authenticated span identity; the host alone admits, protects and persists continuity. See [continuity](continuity.md).

Managed results contain `response`, `native`, `outcome` and `accounting`. Managed outcome is `completed` or `awaiting_tools` and must match actual tool outputs. Accounting contains `usage`, nullable `model`, nullable `response_id`, and `upstream`; usage follows the shared usage-event schema and normalization rules. The host derives usage from actual provider bytes and compares codec claims. It independently verifies terminal status, tool identity/arguments/grammar and event order. Managed progress may publish text/reasoning only and must agree with final text; native state and tool execution arguments remain behind terminal validation.

<a id="독립-검증과-배포"></a>

## Validate and release independently

Start with an Observer: implement ready and exact acknowledgements, package your own executable, inspect the trusted digest, explicitly enable it, and restart an already-built gateway with the selected lock. Exercise synthetic loopback traffic and failure cases. Then implement the larger roles against their complete message and semantic contracts. Keep provider calls and operational credentials out of default tests.

The [contract vectors](../schemas/plugin-vectors.json) include schema-valid/invalid objects, exact canonical package bytes/digests and Observer frames. They are synthetic examples, not executable packages or proof of host acceptance. The `cases` list names a local `schema`, a `value` and expected structural `valid` result. The `canonical_package` contains `utf8`, `sha256` and `files_utf8`; `canonical_usage_event` includes the LF-excluding event digest and matching `ack`; the `observer_frames` list contains newline-delimited bytes encoded as JSON strings. Schema validity does not prove message order, byte identity, safe native behavior or provider correctness.

Before release, record artifact digest, protocol, exact tested host release, platform, test-tool version, passed/failed/not-run scenarios, runtime prerequisites and provenance/notices. Test startup failure, malformed fields, fragmented/oversized frames, sequence mismatch, EOF, deadlines and cancellation. Recorder additionally needs duplicate/conflicting events and failed commits; codec needs JSON/SSE parity, tool validation, replay and usage mismatch tests. A transport-only or Observer-only check cannot certify another role.

Maintain a protocol change log and runnable examples beside each release. Keep package hash tests, schema tests, state-machine tests and actual host integration distinct. A new schema or example must be reviewed against both language editions before their documentation hashes are recorded. Schema evolution does not silently change the running host. No marketplace, publisher signature validation, automatic update, hot reload or hostile-code containment is promised by this contract.
