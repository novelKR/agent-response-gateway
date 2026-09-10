<a id="세-api-경로-적합성-검증"></a>

# Three-route conformance

[English](conformance.md) | [한국어](ko/conformance.md)

The conformance suite runs the actual pinned Codex executable against the gateway
and mock HTTP providers. It covers 35 scenarios: nine for Responses and thirteen
each for Messages and Chat Completions. The test runtime is 0.154.0 on
macOS ARM64, pinned in the [runtime lock](../tests/codex/runtime-lock.json).

<a id="프로토콜-지원-범위"></a>

## Protocol coverage

Responses forwards the original fields within the stateless HTTP contract.
Converted routes require a profile declaring each supported feature.

| Capability | Responses | Messages | Chat Completions |
|---|---|---|---|
| JSON/SSE | Original JSON values and SSE bytes | Converted JSON and incremental events | Converted JSON and incremental events |
| Instruction roles | Original fields | Leading instruction envelope; late instructions rejected | Original role fields and order |
| Function tools/results | Original fields | tool_use/tool_result | tool_calls/tool messages |
| Namespaces/custom text/registered patch grammar | Original fields | Declared name mapping, JSON wrapper and grammar validation | Same conversion rules using function-tool fields |
| Strict tools | Forwarded | Declared strict flag | Declared strict flag |
| Strict JSON schema | Original format and rules | Declared schema rules; name retained in the Responses descriptor | Declared schema name, rules and strict flag |
| Loose schema or json_object | Forwarded | Rejected | Declared format |
| Reasoning effort | Original value | low/medium/high/xhigh/max | none/minimal/low/medium/high/xhigh/max |
| Reasoning summaries/opaque state/verbosity | Forwarded within stateless restrictions | Rejected | Rejected |
| Context/output limits | Declared output bound when profiled | Declared output bound/default | Declared output bound/default |
| Input token counting | Not implemented | Not implemented | Not implemented |
| Completion | Provider terminal event | Valid message_stop after a consistent stop reason | Valid finish_reason and final [DONE] |
| Retry/fallback/storage | None | None | None |

Provider support and output quality must be tested with the selected model.
Matching effort labels or token counts do not imply equal compute, cost or quality.
The host validates structured output; the gateway preserves schema rules without
replacing them with a prompt or a second general schema validator.

<a id="시험-항목"></a>

## Test coverage

All routes test text, function and namespace round trips, custom patch application,
approval denial, explicit effort/strict output, transport failure and cancellation.
Cancellation covers both active output and heartbeat-only streams, with upstream
closure required within 5000 ms of the interrupt.

Converted routes also test parallel tools, grammar failure before execution,
follow-up text and mixed tool/text results. Checks include route and model selection,
credential isolation, tool identities and arguments, response values and no retry.
HTTP and codec tests additionally cover malformed input, arbitrary byte/UTF-8 splits,
truncation, output limits and release of request capacity.

The test host uses the restricted catalog described in [Messages](messages.md):
optional reasoning, verbosity and search defaults are disabled, while explicit
per-turn output controls are tested separately.

<a id="시험-실행"></a>

## Running the suite

Prepare the pinned runtime and build the gateway as described in the
[test guide](../tests/codex/README.md), then run
`python3 -B tests/codex/conformance.py`. Use `--api` and `--scenario` for a narrower
local check; CI runs the complete set. Results include counts, statuses, profile
digest and timings without request/response bodies or credentials.

These tests establish protocol compatibility with mock providers. Before production
use, test the actual model, application permissions and recovery under the
[embedding](embedded-design.md) and [continuity](continuity-design.md) contracts.
