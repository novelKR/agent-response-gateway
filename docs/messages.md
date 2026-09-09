<a id="messages-어댑터-지원"></a>

# Messages adapter support

[English](messages.md) | [한국어](ko/messages.md)

G07–G12 provide Messages request, JSON response and incremental stream conversion
behind an explicitly declared Messages route. The pinned actual-Codex/mock and
HTTP cancellation suites pass for the profile below. Tests use synthetic data
only; no actual provider model has been qualified.

| Contract | Implemented codec behavior |
|---|---|
| Top-level instructions | Messages system text |
| Leading system/developer messages | Explicit approved `bridged_instruction_envelope` only |
| Late system/developer messages | Reject; never move them before conversation content |
| Ordered user/assistant text | Preserve ordered content blocks; adjacent equal roles combine as Messages specifies |
| HTTPS image references without detail | Map to URL source; no downloads; other image inputs reject |
| Flat function definitions/calls/results | Schema, parsed JSON values, call IDs and result association retained |
| Function argument schema absent | Empty object schema with additionalProperties false for a parameterless function |
| Tool choices and parallel restrictions | Map to Messages tool_choice/disable_parallel_tool_use and validate returned calls |
| Custom text tools | Explicit `bridged_custom_tool_json`, one-string JSON wrapper and exact restoration |
| Ordered namespace groups | Explicit `bridged_tool_namespace`, flat aliases shared by definitions, choices, history, results and output; group/member descriptions retained |
| Registered patch grammar | Explicit `bridged_codex_patch_grammar`, exact SHA-256/version selection and post-generation syntax validation |
| Strict function tools | Native strict flag, only with declared strict_tool_arguments support |
| Strict JSON schema output | Native output_config.format with unchanged schema rules; strict_structured_output and structured_output required |
| Reasoning effort | Native output_config.effort for low/medium/high/xhigh/max; other values reject |
| Loose JSON schema/json_object, reasoning summaries/state, verbosity | Explicitly unsupported |
| Non-streaming text/tool output | Validate through EventIR and produce Responses JSON; untrusted bytes reject duplicate JSON keys |
| Streaming text and function arguments | Incremental Responses events, stable IDs/indices and ordered sequence numbers |
| Streaming custom input | Buffer the envelope per tool; emit restored input only after wrapper and grammar validation |
| SSE boundaries | Arbitrary UTF-8/byte splits, LF/CRLF/CR, BOM, comments and multiline data |
| Stream error, truncation or unknown semantic event | Fail; never synthesize response completion or retry |
| max_tokens finish | Incomplete response; never report token truncation as completion |
| Unknown blocks, citations, state or finish behavior | Explicit error; never drop semantic output to claim success |
| Usage | Input plus cache-read/cache-created input tokens, output, and checked total |
| Provider errors | Existing HTTP status sanitization; codec errors never contain provider bodies |

The instruction bridge preserves text, role provenance and prefix order, but
cannot guarantee the native role priority behavior of Responses. It is opt-in
only on Messages. User/tool content stays outside the envelope. Host tool
permissions and approvals keep their existing owner. See the approved
[instruction bridge design](messages-instruction-design.md).

A profile declaration cannot activate unsupported encoder features. The codec
rechecks its implemented subset after pure capability admission. Configuration
and profile validation do not certify a provider or a real model's output quality.

A returned model must match the frozen upstream model; configure a concrete
qualified model identifier. Response/item IDs identify the translated response and retain original call IDs
for tool results. They provide no lookup or persisted response semantics. The
created_at field is the gateway's conversion time; the Messages response does not
provide a corresponding creation timestamp. Cache-token accounting is retained
without claiming that provider token counts are comparable across models.

Primary wire contracts: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons),
and [Responses](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).
No upstream implementation code or consumer data was copied into the codec/tests.

<a id="도구스트림-한도"></a>

## Tool and stream limits

The custom/namespace bridges require native function-tool support in the declared
profile. These bridges do not claim native namespace semantics or constrained
sampling. A member's namespace and original name remain its canonical identity;
plain and grouped tools may share a leaf name, but duplicate effective identities
and nested groups reject. Unknown aliases, mismatched choices/results, duplicate
wrapper fields and non-string/extra wrapper fields reject.

The initial grammar registry is `codex-patch/1`, selected only when the declared
Lark grammar hashes to
`d6367f4826ed608c424b0a308f3d6163527df63c22513d089b91863552f8bfeb`.
This declaration was observed from the pinned synthetic Codex runtime. The public
registry contains its fingerprint and an independently written syntax checker;
no upstream grammar source is vendored. SHA-256 uses the explicitly approved
existing locked `ring` 0.17.14 package without new packages or feature activation.
Only patch syntax is checked: file permissions, existence, content applicability
and execution stay with the host. Unknown grammar definitions reject.

The SSE framer returns one event at a time so its caller can yield downstream
before consuming another event from the same network chunk. The caller supplies
positive framing and aggregate output byte limits. Event validation additionally
bounds deltas to 1 MiB, accumulated arguments to 8 MiB and output items to 4096.
Text and raw tool fragments share the aggregate output budget. No response is
completed without message_delta followed by message_stop; EOF or dropping the
state is not a successful terminal event. Actual HTTP disconnect/cancellation is
owned by the transport. G09 tests both socket closure and permit release, including
a source sending only SSE comments. The existing max_response_bytes configuration
also bounds each converted SSE event and aggregate retained output. Native SSE
keeps its existing unbuffered passthrough behavior.

The regression suite checks every byte split of a text/parallel-function/custom
stream, exact custom text restoration, namespace collisions and choice/history
mapping, duplicate JSON, malformed wrappers, early EOF, error/unknown/order
failures and aggregate limits. These codec tests complement the actual-Codex checks below and do not establish
provider qualification. The streaming contract follows the primary
[Messages streaming documentation](https://platform.claude.com/docs/en/build-with-claude/streaming).

<a id="검증한-합성-codex-프로필"></a>

## Qualified synthetic Codex profile

The verified test artifact is the temporary `0.154.0-alpha.6` / macOS ARM64 baseline
in [the runtime lock](../tests/codex/runtime-lock.json). The host uses its bundled
`gpt-5.4` catalog entry to select the test tool contract; the gateway routes every
model request to a synthetic upstream model. This is not a GPT-5.4 provider call.
The generated catalog retains all original prompts and other model fields, with
these explicit capability overrides:

| Catalog field | Test setting |
|---|---|
| support_verbosity | false |
| default_verbosity | null |
| default_reasoning_level | null |
| supported_reasoning_levels | empty array |
| supports_search_tool | false |

The isolated host config disables model reasoning metadata, tool_search,
search_tool, multi_agent and web search. Function tools, the registered freeform
patch tool, dynamic namespace tools and approval handling remain enabled. Generic
config flags alone did not remove all unsupported fields from the pinned runtime;
the suite verifies the effective wire request after applying this catalog. The
canonical catalog digest for this pinned profile is
`5730ed50d14b2432b960cfb821c6de91edcdc70665e650f71f0dff032ab14b8a`.
The catalog is derived at runtime and is not copied into public fixtures.

Message and tool-call status and output-text annotations are typed IR fields.
Native round trips retain them. Converted history admits an absent/completed
status and absent/empty annotations; unfinished messages/calls and meaningful
annotations reject. A tool-use/text/tool-use assistant block sequence is retained
before its results. Assistant continuation after only some results rejects.
Unknown required fields, reasoning summaries, verbosity, opaque state and hosted
search remain unsupported.

The thirteen actual-Codex scenarios cover text, functions, namespaces, patch
application/result replay, approval denial, two parallel calls, a subsequent text
turn, eventful/heartbeat cancellation, transport loss and grammar failure before
tool execution, explicit high effort plus strict output schema, and mixed
tool/text result replay. The default CI command runs all 35 scenarios across the
three API routes; see the [common matrix](conformance.md). Local HTTP tests additionally cover non-streaming JSON,
auth/version headers, pre-dispatch rejection, incremental bytes, sanitized errors,
EOF and aggregate limits.

Results emit payload-free timing fields: turn_elapsed_ms, first_client_text_ms
when text is observed, and interrupt_to_upstream_close_ms for cancellation.
These measure the synthetic control/HTTP path, including host processing; a
function scenario's first text follows its tool round trip. They are not model
latency, isolated gateway overhead or a production service-level guarantee.
One local qualification run on 2026-09-08 observed first text at 27.520 ms for the
text scenario and upstream closure at 109.056 ms / 104.698 ms for eventful /
heartbeat interruption, against the required 5000 ms bound measured from the
interrupt. CI reruns the bounds and records fresh timings.

Provider-specific context counting, real-model behavior, consumer integration,
long-term continuity and release acceptance remain separate stages. A profile
configuration or this synthetic success does not certify an operator's model.
See [the Messages configuration example](../config.messages.example.toml).

Messages and Chat Completions share returned-tool selection, call-count and
original-identity validation. Each API retains its own finish and instruction-hierarchy contract.

<a id="native-출력-제어"></a>

## Native output controls

Explicit profile support is required for strict tools, reasoning effort and
strict structured output. Messages uses output_config.effort without renaming
levels and output_config.format with the original JSON schema. Only source
json_schema with strict=true is supported: json_object, loose/unspecified strict
schemas and unsupported effort labels reject before dispatch. The source format
name is a descriptor with no Messages wire field; it remains in the canonical IR
and returned Responses text.format. It is never injected into schema rules or a
prompt. The schema and strict tool constraints are provider/host contracts, not a
second gateway JSON Schema implementation.

Equal effort labels do not establish equal reasoning, compute or cost across
providers. Schema dialect support, model-specific capabilities and actual output
adherence require provider qualification and host validation. The pinned synthetic
scenario verifies explicit controls reach the wire and its known valid JSON reaches
Codex; it does not certify model compliance. Primary contracts are
[structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)
and [effort](https://platform.claude.com/docs/en/build-with-claude/effort).
