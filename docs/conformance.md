<a id="세-api-경로-적합성-검증"></a>

# Three-route conformance

[English](conformance.md) | [한국어](ko/conformance.md)

G12 qualifies the implementation against a pinned actual Codex executable and
synthetic loopback upstreams. The default CI run has 35 scenarios: nine native
Responses, thirteen Messages and thirteen Chat Completions. It uses the temporary
0.154.0-alpha.6 / macOS ARM64 artifact in the committed runtime lock. No live
provider model, consumer workflow or release is qualified by these results.

| Capability | Native Responses | Messages | Chat Completions |
|---|---|---|---|
| JSON/SSE | Original JSON values and raw SSE | Explicit codec and incremental blocks | Explicit codec, incremental text and bounded tool fragments |
| Instruction roles | Original fields | Approved leading instruction envelope; late instructions reject | Native role fields/order |
| Function tools and results | Original wire fields | Native tool_use/tool_result | Function tool_calls/tool messages |
| Namespace/custom/known patch grammar | Original wire fields | Explicit namespace, JSON wrapper and grammar bridges | Same explicit bridges in the function-wire profile |
| Strict tools | Forwarded | Declared native strict flag | Declared native strict flag |
| Strict JSON schema | Original descriptor/rules | Declared native schema rules; source name retained as response descriptor | Declared native descriptor/rules/strict flag |
| Loose schema or json_object | Forwarded | Reject | Declared native format |
| Explicit effort | Original value | low/medium/high/xhigh/max only | none/minimal/low/medium/high/xhigh/max only |
| Reasoning summaries/opaque state/verbosity | Forwarded within stateless policy | Reject | Reject |
| Token/context limits | Declared output bound when a profile exists | Declared output bound/default | Declared output bound/default |
| Input token counting | Unqualified | Unqualified | Unqualified |
| Completion | Provider's terminal semantics | Valid message_stop after consistent stop reason | Valid finish_reason and final [DONE] |
| Retry/fallback/storage | None | None | None |

Forwarding and a native profile declaration do not prove that a real model
implements a feature. Equal effort labels or token counters do not establish
equal compute, cost or quality across providers. Structured output is a native
provider contract; the host validates its data and the gateway preserves schema
rules without a second JSON Schema implementation or prompt substitution.

Every route exercises text, a function round trip, a namespace round trip, custom
patch application/result replay, host approval denial, eventful and heartbeat
cancellation, transport loss, and explicit high effort plus a strict JSON schema.
The converted routes also exercise two parallel calls, invalid patch grammar
before tool execution, a later text turn, and tool/text output followed by tool
results. Assertions check endpoint, model, authentication, native fields, original
tool identity/arguments, exact schema values, returned JSON, absence of retries,
and the 5000 ms interrupt-to-upstream-close bound.

The converted test host uses the explicit catalog/settings documented in
[Messages](messages.md). Default optional reasoning/verbosity/search capabilities
are disabled, while the output-control scenario supplies required effort and
schema per turn. This distinguishes a basic tool profile from hosts that always
require these controls. The original catalog prompts are derived from the pinned
runtime, retained unchanged, and not copied into public fixtures.

Shared HTTP regressions run for both adapters: JSON conversion, authentication
header isolation, admission before upstream access, arbitrary byte splits,
incremental output, sanitized errors, missing completion, aggregate limits,
heartbeat disconnect and capacity release. Pure codecs additionally test
malformed JSON/wrappers, semantic extensions, output order and identity, UTF-8,
partial tool arguments, grammar checks and message history metadata.

Run the full matrix with `python3 -B tests/codex/conformance.py` after the pinned
runtime is prepared and the current gateway binary is built. `--api` and
`--scenario` select narrower diagnosis; their success does not replace the full
default CI run. Output contains only synthetic counts, outcomes, host-profile
digest and timing metrics. It contains no request/response bodies or credentials.

Provider qualification needs an explicit model, credentials and cost budget.
Consumer startup/approval/cancellation and long-running continuity require their
separate acceptance paths. The [embedded](embedded-design.md) and
[continuity](continuity-design.md) contracts retain those boundaries.
