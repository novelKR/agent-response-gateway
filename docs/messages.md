<a id="messages-어댑터-지원"></a>

# Messages adapter support

[English](messages.md) | [한국어](ko/messages.md)

The Messages adapter converts Responses requests, JSON responses and SSE streams
using an explicitly configured Messages route. Start with the
[configuration example](../config.messages.example.toml) and declare the features
your host requires. Protocol tests use mock providers; test the selected real
model before operational use.

| Feature | Behavior |
|---|---|
| Top-level instructions | Messages system text |
| Leading system/developer messages | Require `bridged_instruction_envelope` |
| System/developer messages after conversation content | Rejected; never moved to the beginning |
| Ordered user/assistant text | Preserve content order; combine adjacent equal roles under Messages rules |
| HTTPS image references without detail | URL source without downloading; other image inputs rejected |
| Function definitions/calls/results | Preserve schema, parsed JSON arguments, call IDs and result association |
| Missing function argument schema | Empty object schema with additionalProperties false |
| Tool choices and parallel restrictions | Map tool_choice/disable_parallel_tool_use and validate returned calls |
| Custom text tools | Require `bridged_custom_tool_json`; wrap one string in JSON and restore it exactly |
| Namespace groups | Require `bridged_tool_namespace`; share aliases across definitions, choices, history, results and output |
| Registered patch grammar | Require `bridged_codex_patch_grammar`; select by SHA-256/version and check generated syntax |
| Strict function tools | Set the provider strict flag when strict_tool_arguments is declared |
| Strict JSON schema output | Preserve output_config.format rules; require strict_structured_output and structured_output |
| Reasoning effort | output_config.effort accepts low/medium/high/xhigh/max; other values rejected |
| Loose JSON schema/json_object, verbosity | Unsupported |
| Thinking summaries and native state | Explicit managed Claude contract only; see [managed thinking](#managed-thinking) |
| Non-streaming text/tool output | Validate output events and produce Responses JSON; reject duplicate JSON keys |
| Streaming text and function arguments | Incremental Responses events with stable IDs, indices and sequence numbers |
| Streaming custom input | Buffer per tool; restore input after wrapper and grammar validation |
| SSE framing | Arbitrary UTF-8/byte splits, LF/CRLF/CR, BOM, comments and multiline data |
| Stream error, truncation or unknown semantic event | Fail without manufactured completion or retry |
| max_tokens finish | Incomplete response |
| Unknown blocks, citations, state or finish behavior | Explicit error |
| Usage | Input, cache-read/cache-created input, output and checked total tokens |
| Provider errors | Sanitized HTTP status/error; provider response bodies are not included |

The [instruction mapping](messages-instruction-design.md) preserves text, source
roles and order, but cannot enforce separate system/developer priority in Messages.
User/tool content stays outside the instruction envelope. The host owns tool
permissions and approval.

Returned model names must match the configured upstream model. Converted response
and item IDs identify the translated output; original tool call IDs are retained.
They do not provide stored-response lookup. created_at is the gateway's conversion
time because Messages supplies no equivalent creation timestamp.

Primary contracts: [Messages](https://platform.claude.com/docs/en/api/messages/create),
[stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons)
and [Responses](https://developers.openai.com/api/reference/typescript/resources/responses/methods/create).

<a id="도구스트림-한도"></a>

## Tool and stream limits

Custom and namespace conversion require provider function-tool support. They do
not provide native namespace semantics or constrained sampling. Tool identity is
the namespace/name pair: flat and grouped tools may share a leaf name, but duplicate
identities and nested groups are rejected. Unknown aliases, mismatched results,
duplicate wrapper fields, extra fields and non-string custom input are rejected.

The supported patch grammar is `codex-patch/1`, selected when the declared Lark
grammar hashes to
`d6367f4826ed608c424b0a308f3d6163527df63c22513d089b91863552f8bfeb`.
The validator checks syntax only. File permissions, existence, applicability and
execution belong to the host. Unknown grammar definitions are rejected.

Event validation limits deltas to 1 MiB, accumulated arguments to 8 MiB and output
items to 4096. Text and tool fragments share the aggregate output budget.
max_response_bytes also bounds each converted SSE event and retained output.
The decoder yields one event at a time so downstream output need not wait for
all events in a network chunk.

Completion requires message_delta followed by message_stop. EOF or a discarded
stream is not success. HTTP cancellation closes the upstream connection and
releases request capacity, including for streams carrying only SSE comments.
Responses passthrough does not buffer or reinterpret the complete stream.

Tests cover every byte split, parallel/custom tools, exact text restoration,
namespace collisions, duplicate JSON, malformed wrappers, event ordering,
truncation and limits. See [Messages streaming](https://platform.claude.com/docs/en/build-with-claude/streaming)
for the provider event format.

<a id="codex-시험-프로필"></a>
<a id="qualified-synthetic-codex-profile"></a>
<a id="검증한-합성-codex-프로필"></a>

## Codex test profile

The test profile uses `0.154.0` on macOS ARM64 from the
[runtime lock](../tests/codex/runtime-lock.json). Codex's bundled `gpt-5.4` catalog
entry selects the tool settings; model traffic goes to a mock provider.
The catalog retains its prompts and other fields with these overrides:

| Catalog field | Test setting |
|---|---|
| support_verbosity | false |
| default_verbosity | null |
| default_reasoning_level | null |
| supported_reasoning_levels | empty array |
| supports_search_tool | false |

The isolated host disables reasoning metadata, tool_search, search_tool,
multi_agent and web search. Function tools, patch input, dynamic namespaces and
approval handling remain enabled. The suite checks the effective HTTP request.
The catalog digest is
`5730ed50d14b2432b960cfb821c6de91edcdc70665e650f71f0dff032ab14b8a`.

Converted history accepts absent/completed message and tool-call status and
absent/empty output-text annotations. Unfinished calls, meaningful annotations,
reasoning summaries, verbosity, opaque state and hosted search are rejected in the default stateless mode.
Tool/text/tool assistant order is preserved before its results; assistant
continuation after only some results is rejected.

The [common matrix](conformance.md) covers thirteen Messages scenarios. Result
fields turn_elapsed_ms, first_client_text_ms and interrupt_to_upstream_close_ms
measure the mock-provider path, not real-model latency or isolated gateway overhead.
Cancellation must close upstream within 5000 ms of the interrupt.
See the [test guide](../tests/codex/README.md) for preparation and commands.

<a id="native-output-controls"></a>
<a id="native-출력-제어"></a>
<a id="출력-제어"></a>

## Output controls

Strict tools, reasoning effort and strict structured output require explicit
profile support. Messages uses output_config.effort without renaming levels and
output_config.format with unchanged JSON schema rules. Only source json_schema
with strict=true is supported. json_object, loose or unspecified strict schemas
and unsupported effort labels are rejected before dispatch.

The source format name has no Messages field. It remains in the intermediate
representation and Responses text.format, without being inserted into schema
rules or prompts. The host validates returned data against the requested schema.
Equal effort labels do not imply equal compute, cost or model behavior.
See [structured outputs](https://platform.claude.com/docs/en/build-with-claude/structured-outputs)
and [effort](https://platform.claude.com/docs/en/build-with-claude/effort).

<a id="관리형-thinking"></a>

## Managed thinking

Messages remains stateless by default. To enable Claude thinking, select
continuation_mode="managed" on the model, configure the [continuation store and host session](interactions.md#host-control-and-resume), and declare a reasoning_contract
on its capability profile. Database configuration alone does not enable reasoning.
The host creates the session and supplies x-gateway-session to Codex; it retains the
separate control token, stable protection key and database. The gateway uses the
common attempt/finalize barrier and ReplayV2; tools remain host-executed.

An Adaptive profile fragment is:

```toml
[models.claude]
provider = "claude"
upstream_model = "YOUR_QUALIFIED_MODEL"
api = "messages"
auth = "api_key"
messages_version = "2023-06-01"
capability_profile = "claude"
continuation_mode = "managed"

[capability_profiles.claude.reasoning_contract]
kind = "claude_adaptive"
version = 1
efforts = ["low", "medium", "high"]
default_effort = "medium"
allow_forced_tools = false
```

Retain the profile's provider/model/API and context/output limits. Its support table
must declare reasoning_summary="native" and reasoning_items="native", and
reasoning_effort="native" when clients request effort. The selected effort must be
in efforts. Forced tool selection requires an explicitly qualified Adaptive profile
with allow_forced_tools=true; the default rejects it.

For Manual thinking, replace the reasoning_contract table with:

```toml
[capability_profiles.claude.reasoning_contract]
kind = "claude_manual"
version = 1
budget_tokens = 2048
effort_budgets = { low = 1024, medium = 2048, high = 4096 }
interleaved_beta = true
```

An explicit effort selects its exact effort_budgets entry; an absent effort uses
budget_tokens. Every budget is at least 1024 and strictly below max_tokens. Manual
thinking rejects forced tools. interleaved_beta=true explicitly sends
anthropic-beta: interleaved-thinking-2025-05-14; no beta is inferred from a model name.
Both modes reject temperature and top_p controls in this contract. A pending tool
turn must retain its thinking mode, budget and effort; changes fail before attempt
creation or provider dispatch.

Both profiles request thinking.display="summarized". Public thinking text appears
in Responses reasoning summaries, including reasoning-only responses. Empty signed
thinking and redacted_thinking remain intact in encrypted native content blocks;
the gateway never displays signatures or redacted data, reconstructs hidden text or
calls another model to summarize. Native block order is preserved across tools.
reasoning.summary="auto" selects this public display; concise and detailed are
rejected. Unknown blocks, missing signatures, malformed indices and incomplete
streams fail explicitly. Standard Messages routes do not accept these extensions.

Usage adds cache-read and cache-creation input tokens to uncached input; output
already includes thinking and is not added twice. Missing counters remain unknown.
Unavailable optional details are omitted because Codex 0.154.0 cannot parse null
integers inside a details object. Visible text length is never a token estimate.

Synthetic Codex 0.154.0 tests cover reasoning notifications, tools, restart, payload
repair, host-managed compaction and explicit recovery for both profiles. The
[wire scope](../tests/reasoning/wire-lock.json) pins the reviewed contract and source
digests. This is mock protocol/recovery support, not qualification of a real Claude
model. Changing model, route or reasoning contract requires a new bound session.
