# Chat Completions adapter support

G10–G12 provide request, JSON response, stream and HTTP conversion behind an
explicit profile. The pinned actual-Codex/mock and HTTP suites pass for the
function-wire profile. No live provider model is qualified by these fixtures.

This initial profile uses the standard function-tool wire shape. Custom freeform
tools use the explicitly declared CustomToolJson bridge, with the same request-
scoped identity/choice/result validation as Messages. The current Chat Completions
API also defines native custom tools; that separate wire dialect is not part of
this adapter's implemented/qualified subset. A Native custom declaration fails
explicitly and is never silently converted into the JSON bridge.

| Contract | Implemented behavior |
|---|---|
| Top-level instructions | Leading system message |
| Explicit system/developer/user/assistant messages | Native role fields in the original order, including later instructions |
| Ordered text content | Text/part order retained; assistant preface can share a message with its following parallel tool calls |
| HTTPS user image references | image_url with optional auto/low/high detail, no download; images in other roles reject |
| Function schema/calls/results | Preserve schema JSON values, raw argument strings, call IDs and result association |
| Parameterless function | Explicit empty object schema with additionalProperties false |
| Custom/namespace/registered grammar | Require the corresponding approved bridges; one mapping covers definitions, choices, history and output |
| Tool choice/parallel control | Map native fields and validate returned identities/counts |
| max_output_tokens | max_completion_tokens, bounded by the declared route; no legacy max_tokens fallback |
| Temperature/top_p | Preserve values in Chat's 0–2 / 0–1 range |
| Streaming | Incremental text; bounded tool ID/name/argument fragments; canonical text-before-tools output and explicit finish + [DONE] |
| Strict function schemas | Preserve the native strict flag when explicitly declared supported |
| JSON/text output formats | Map to response_format; schema name, rules and optional strict flag are retained without rewriting |
| Reasoning effort | Map the native none/minimal/low/medium/high/xhigh/max value to reasoning_effort; no renaming or fallback |
| Reasoning summaries/state and verbosity | Explicitly unsupported in this codec profile |
| Non-streaming output | Exactly one choice; text and function/custom output validated with EventIR |
| stop/tool_calls finish | Completed only with a consistent tool count/choice |
| length finish | Incomplete/max_output_tokens; never successful completion |
| Refusal/filter/legacy function_call/unknown semantic output | Explicit conversion error |
| Usage | Optional prompt/completion/total with checked arithmetic; cached/reasoning details retained when present; absence stays null |

Chat created is preserved as Responses created_at. Translated response/item IDs
are derived from the provider response ID, while tool call IDs and namespace/name
identities are preserved. The gateway does not create response lookup/storage or
history ownership. Unknown extension fields and cross-protocol opaque input remain
errors. Provider-specific model/role behavior needs a separately qualified profile.

The shared PreparedTools helper owns returned tool selection/count checks and
output identity restoration for Messages and Chat; each adapter retains its own
wire/finish semantics. Parsing provider bytes rejects duplicate JSON keys before
Value conversion, and the custom wrapper additionally validates its raw argument
string for duplicate/extra fields and registered grammar syntax.

The codec and streaming tests cover role/order/options, namespace/custom named choices and
parallel history, required effort/strict schema controls, raw numeric arguments,
cache/reasoning counters, missing usage,
token truncation, unsupported inputs and malformed outputs. Existing Messages
regressions remain required after shared-helper changes. Native Responses keeps
its raw JSON/SSE contract. The common dispatch state uses each translated
request's admitted plan once and owns its stream/permit until completion or drop.

Primary contracts: [Chat Completions create](https://developers.openai.com/api/reference/typescript/resources/chat/subresources/completions/methods/create)
and [custom tools](https://developers.openai.com/api/docs/guides/function-calling#custom-tools).
The fixtures are newly authored synthetic data; no reference implementation code,
tests or prompts are copied.

Explicit reasoning effort and output schemas are necessary for hosts that supply
these controls on every turn. The codec forwards their native Chat fields only
when the declared profile supports the required features. It does not lower effort,
rewrite schema rules or replace strict generation with a prompt. Schema dialect
support, output adherence and effort behavior remain native-provider contracts;
the gateway does not introduce a second general JSON Schema validator. The host
must still validate its resulting data. G12 verifies explicit high effort and
strict output schema with the actual pinned Codex and a synthetic upstream;
consumer operational acceptance remains separate.

## Streaming contract

The SSE framer handles arbitrary byte/UTF-8 boundaries. A stream fixes one response
ID/model/created value and one choice index. Tool fragments are collected by their
provider index, including split IDs and names, and final tool indices must be
contiguous. Stable metadata cannot change the bound response identity.

A Chat response has one assistant content field and an ordered tool-call array.
Text streams immediately. Tools are buffered until finish_reason so text arriving
after an early tool fragment cannot change already emitted output indices or tool
identity. The complete response codec and EventIR validator check every tool,
choice and required grammar before tool completion is emitted. Both text and tool
identity/argument bytes count toward the caller's aggregate output limit; argument
fragments additionally share the 8 MiB IR argument bound. Converted text deltas
are limited to 1 MiB, and accumulated text is divided at UTF-8 boundaries when
validated as a final JSON response.

The adapter emits response.completed/incomplete only after a valid finish_reason
and the final data: [DONE]. A final usage-only chunk is supported. If optional
usage is absent, counters remain null; inconsistent or decreasing counters reject.
Missing terminal markers, provider errors, unknown semantic deltas, response
identity changes and data after finish reject and poison the state. A truncated
invalid tool JSON string fails conversion, even when finish_reason is length; it
is never completed or executed as a repaired argument object.

Tests include every byte split of a late-text/parallel-tool stream, independent
fragments for IDs/names/arguments, exact non-streaming/streaming equivalence,
truncated streams, malformed custom envelopes, identity/order failures, missing
usage, large accumulated text and aggregate argument/output limits. HTTP tests
cover credential isolation, pre-dispatch errors, JSON, split streams, sanitized
errors, EOF, aggregate limits, cancellation and capacity release for both adapters.

The [common conformance matrix](conformance.md) records thirteen actual-Codex Chat
scenarios, including tool/namespace/custom and parallel round trips, mixed output
replay, subsequent text turns, explicit effort/schema, approval denial, grammar
failure, transport loss and two cancellation modes. The test host derives the same
restricted catalog described by [Messages](messages.md), with optional defaults
disabled but explicit per-turn controls retained. Configuration example:
[Chat Completions](../config.chat.example.toml).
