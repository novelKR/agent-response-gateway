<a id="chat-completions-어댑터-지원"></a>

# Chat Completions adapter support

[English](chat-completions.md) | [한국어](ko/chat-completions.md)

The Chat Completions adapter converts Responses requests, JSON responses and SSE
streams through a declared capability profile. Use the
[configuration example](../config.chat.example.toml) to select the route and features.

Tools use the standard function-tool format. Custom freeform input requires the
explicit JSON bridge used by Messages, with request-scoped name, choice and result
validation. Chat Completions' separate native custom-tool format is unsupported;
a Native custom declaration is rejected rather than converted implicitly.

| Feature | Behavior |
|---|---|
| Top-level instructions | Leading system message |
| system/developer/user/assistant messages | Original role fields and order, including later instructions |
| Ordered text | Preserve part order; assistant text can share a message with following parallel calls |
| HTTPS user images | image_url with optional auto/low/high detail; no download; images in other roles rejected |
| Function schemas/calls/results | Preserve schema JSON, raw argument strings, call IDs and result association |
| Parameterless functions | Empty object schema with additionalProperties false |
| Custom text/namespaces/registered grammar | Require declared conversion rules; share one mapping across definitions, choices, history and output |
| Tool choice and parallel control | Map fields and validate returned identities/counts |
| max_output_tokens | Bounded max_completion_tokens; no max_tokens fallback |
| Temperature/top_p | Preserve values within 0–2 / 0–1 |
| Streaming | Incremental text, bounded tool fragments, text-before-tools output and explicit finish + [DONE] |
| Strict function schemas | Preserve the strict flag when declared supported |
| JSON/text output formats | response_format with original schema name, rules and optional strict flag |
| Reasoning effort | reasoning_effort accepts none/minimal/low/medium/high/xhigh/max without renaming or fallback |
| Reasoning summaries/state and verbosity | Unsupported |
| Non-streaming output | Exactly one choice; validate text and tool output |
| stop/tool_calls finish | Complete only with consistent tool count and choice |
| length finish | Incomplete/max_output_tokens |
| Refusal/filter/function_call/unknown semantic output | Explicit conversion error |
| Usage | Checked optional prompt/completion/total; cached/reasoning details retained; missing values remain null |

The provider's created timestamp is preserved as created_at. Response and item IDs
are derived from the provider response ID; original tool call IDs and namespace/name
pairs are retained. The gateway does not store responses or provide ID lookup.
Unknown extension fields and cross-protocol opaque input are rejected.

Returned tool names, choices and counts must match the request. Provider JSON is
checked for duplicate keys. Custom input also rejects duplicate or extra wrapper
fields and invalid registered grammar.

Strict output and reasoning effort require explicit profile support. The adapter
preserves the schema rules and effort level without weakening them or replacing
them with a prompt. The host validates the returned data; the actual provider/model
must be tested for schema support, instruction behavior and output quality.

Primary contracts: [Chat Completions](https://developers.openai.com/api/reference/typescript/resources/chat/subresources/completions/methods/create)
and [custom tools](https://developers.openai.com/api/docs/guides/function-calling#custom-tools).

<a id="스트리밍-계약"></a>

## Streaming contract

The decoder handles arbitrary byte/UTF-8 boundaries. A stream fixes one response
ID, model, created value and choice index. Tool fragments are collected by provider
index, including split IDs and names; final tool indices must be contiguous.
These identifiers cannot change within the response.

Text is sent immediately. Tools are buffered until finish_reason so later text
cannot change already emitted output indices. Complete tool arguments, choices
and required grammar are validated before tool completion. Text and tool bytes
count toward the aggregate output limit. Accumulated arguments are limited to
8 MiB and individual text deltas to 1 MiB. Large final text is split at UTF-8 boundaries.

The adapter emits response.completed/incomplete only after a valid finish_reason
and final data: [DONE]. A final usage-only chunk is supported. Missing usage stays
null; inconsistent or decreasing counters are rejected.

Missing terminal markers, provider errors, unknown semantic deltas, changed
response identity and data after finish fail the stream. Invalid truncated tool
JSON fails even when finish_reason is length; the adapter never repairs it into
an executable argument object.

Tests cover byte splits, parallel calls, exact streaming/non-streaming values,
truncation, malformed custom input, identity/order failures, usage and output
limits. HTTP tests cover credentials, pre-dispatch rejection, cancellation and
request-capacity release.

The [common suite](conformance.md) runs thirteen Chat Completions scenarios with
actual Codex and a mock provider. It uses the restricted [Messages test profile](messages.md),
including explicit effort/schema controls, grammar rejection, approval denial and
both cancellation modes. These results validate the protocol path; operational
use requires the selected real model and application to be tested.

Usage accounting and the optional recorder are described in the
[token usage accounting guide](usage-accounting.md). Recorder installation,
local commit guarantees and external delivery are separate from HTTP metadata observation.
