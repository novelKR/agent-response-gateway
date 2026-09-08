# Chat Completions adapter support

G10 provides a pure request/non-streaming response codec. The Chat Completions
server route remains disabled until G11 streaming and G12 actual-Codex/mock
qualification pass. No live provider model is qualified by these fixtures.

This initial profile uses the standard function-tool wire shape. Custom freeform
tools use the explicitly declared CustomToolJson bridge, with the same request-
scoped identity/choice/result validation as Messages. The current Chat Completions
API also defines native custom tools; that separate wire dialect is not part of
this adapter's implemented/qualified subset. A Native custom declaration fails
explicitly and is never silently converted into the JSON bridge.

| Contract | G10 codec behavior |
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
| Streaming request preparation | stream_options.include_usage true; decoder/HTTP activation remain later gates |
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

The seven focused tests cover role/order/options, namespace/custom named choices and
parallel history, required effort/strict schema controls, raw numeric arguments,
cache/reasoning counters, missing usage,
token truncation, unsupported inputs and malformed outputs. Existing Messages
regressions remain required after shared-helper changes. Native Responses HTTP
behavior and Chat's startup rejection remain unchanged.

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
must still validate its resulting data. G12 must exercise these controls with the
actual pinned Codex and synthetic upstream before consumer acceptance is claimed.
