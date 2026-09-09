<a id="ir-v1--요청-의미와-출력-상태-계약"></a>

# IR v1 — Request semantics and output-state contract

[English](ir.md) | [한국어](ko/ir.md)

This document defines the internal Rust library contract at `ir::VERSION = 1`.
Native Responses HTTP forwarding is preserved. The IR provides request codecs,
capability admission, tool bridges and event-state validation used by the Messages
and Chat Completions adapters. The library itself executes no network requests
and defines no new HTTP endpoint or persistent storage format.

<a id="설계-목적과-경계"></a>

## Design goals and boundaries

Represent shared API semantics in one place without reducing them to the common
subset of message strings. Preserve order, instruction roles, tool identity and
the permitted origin of opaque state. Consumer domain types, user approval,
tool execution and publication state are outside this contract.

| Design concept | Implementation |
|---|---|
| CanonicalRequest | `request::RequestIR` |
| Instructions | `instructions()` derived from top-level instructions and ordered messages |
| OrderedItems | `Input`, `Item`, `Content`, `Part` |
| ToolDefinitions | `ToolDefinition`, `ToolIdentity`, `ToolKind`, `ToolInput` |
| GenerationOptions | Output limits, sampling, tool choice, structured output and reasoning options |
| RequiredCapabilities | Derived from a validated request by `capability::requirements()` |
| RouteSnapshot | Frozen actual route and complete capability-profile declaration |
| OpaqueProviderState | `continuity::OpaqueState` and `ContinuityBinding` |
| Output state | `event::EventIR`, `EventValidator` |

HTTP admission and the codec share the same stateless validation function.
Native forwarding is not forced through the IR's narrower subset or additional
structural checks. A request that can pass through native HTTP can therefore
still be rejected by the IR codec.

<a id="요청-표현과-responses-codec"></a>

## Request representation and Responses codec

`responses::decode(value, binding)` interprets a Responses request as `RequestIR`.
`responses::encode(request, binding)` restores JSON values for the same protocol.
Within the supported subset, JSON values and array order survive a round trip,
apart from default normalization to `store:false`. Whitespace, object-key order
and identical serialization bytes are not promised. Function arguments retain
their original JSON string, including large-number precision.

```rust
use agent_response_gateway::ir::{IrError, responses};
use serde_json::json;

fn main() -> Result<(), IrError> {
    let request = responses::decode(
        json!({"model":"example/writer", "input":"Synthetic text"}),
        None,
    )?;
    let wire = responses::encode(&request, None)?;
    assert_eq!(wire["store"], false);
    Ok(())
}
```

Input is a string or ordered item array. Items include system, developer, user
and assistant messages, function/custom calls and results, reasoning summaries
and opaque state. Content is a string or an array containing text or `image_url`
references. The codec does not download/upload images or resolve file IDs.

Top-level instructions use `ProtocolDefault`. Message instructions retain their
original System/Developer role and input position. This read-only view is derived
from the original items. Do not merge instructions into another role's strings
or reorder calls and results into separate arrays. Capability admission and the
target adapter determine whether that protocol can preserve the hierarchy.

`ItemId`, `CallId`, `ResponseId` and `ToolIdentity` are distinct. Tool identity is
namespace plus name; function and custom inputs retain different kinds. Reject
duplicate item/call IDs, results without a preceding call, mismatched call kinds
and duplicate results. This v1 scope requires complete call context within each
independent request.

Tool definitions support flat function/custom declarations and ordered namespace
groups with descriptions. Children use namespace/name identity. Nested groups and
duplicate identities reject. Hosted tools are not interpreted. JSON Schema is
preserved as a value, without verifying the whole schema or a real model's adherence.

<a id="확장-필드"></a>

### Extension fields

Unknown fields retain their source protocol in `Extensions`. They cannot overwrite
known typed fields. Explicit nulls in optional fields survive a same-protocol round
trip. Adding a typed value while retaining an explicit-null representation requires
explicit cleanup; the encoder rejects a conflict.

Unknown input items, content, output formats and tool choices can remain
source-bound extensions. Preserving them for the same API does not qualify the
provider's behavior. Cross-protocol translation currently rejects them because
no explicit conversion rule exists. Neither source labels nor capability claims
derived from client JSON are an authentication boundary.

Store, background, response-ID history, conversation and compaction requests
remain constrained by stateless policy. This codec adds no actual storage,
compaction or resume service.

<a id="기능-판정과-실행-경로"></a>

## Capability admission and execution routes

`requirements(request)` derives required features once from a validated request.
`plan_translation(request, target_binding)` compares them with the target
`CapabilityProfile` and returns a frozen `RouteSnapshot`, requirements and selected
bridges. Callers cannot arbitrarily omit required features from a plan.

Features include instruction hierarchy, images, function/custom tools, strict
arguments, grammar, namespaces, tool choice/parallel control, structured/strict
output, output limits, temperature, top_p, reasoning options/items and opaque
continuity. Explicit restrictions such as `parallel_tool_calls:false` also count
as required capabilities.

Results are `Native`, `Bridged` or `Unsupported`; an absent declaration is
Unsupported. Valid bridges are `CustomToolJson`, `ToolNamespace`,
`CodexPatchGrammar` and Messages-only `MessagesInstructionEnvelope`. Each can be
declared only for its corresponding feature. Tool bridges require Native function
support; the grammar bridge also requires the custom JSON bridge. Unknown bridges
and implicit weakening, such as replacing strict output with a prompt, are forbidden.

A route contains provider ID, actual model, API, credential-binding reference,
adapter version, profile ID/version/full declaration and model limits. Declared
output limits are checked; input-token counting and context-fit measurement are
not performed. Alias resolution, live qualification, network requests and retries
are outside this pure planning function.

## Custom tool JSON bridge

`CustomToolBridge::new()` builds a deterministic mapping from request tool
definitions, choosing function names that do not collide with existing names and
wrapping the input in this schema:

```json
{"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false}
```

`lower_call` and `restore_call` preserve the original name/namespace, item ID,
call ID and string. `lower_choice` uses the same mapping for explicitly selected
custom/namespace functions. Result conversion takes the original call and checks
ID/kind association. Name collisions, unknown wrapper names, duplicate/extra
fields, invalid inputs and wrong call mappings reject.

One request registry flattens namespace groups while preserving group and child
descriptions. Name restoration does not prove a model respects namespace semantics.
Custom format may be omitted, exactly text, or the registered Codex patch grammar.
Select grammar by SHA-256 and version to validate history/output syntax. Unknown
grammars and extensions reject. No tool is executed and file applicability is not
checked. Messages streams collect partial wrappers, validate them, then emit the
restored freeform input.

<a id="불투명-상태와-연속성"></a>

## Opaque state and continuity

`OpaqueState` holds bytes, format and origin binding. The binding combines a full
route snapshot with caller-managed principal/auth-scope references, never actual
API keys or session tokens. `replay()` provides bytes only when target binding
and format match exactly. Changes to model, provider, API, auth scope, adapter or
profile reject, including changed profile contents under unchanged IDs/versions.

Decoding Responses `encrypted_content` requires an explicit origin binding.
Encoding it requires the same binding and `responses.encrypted_content/v1` format.
Do not merge opaque bytes into ordinary messages or reasoning summaries.

The container adds no encryption, signature or ownership authentication and
provides no default Debug or Serialize implementation. It is an internal contract
for restoring source-bound data to the same origin. A proxy-owned encrypted
envelope or client-facing resume token requires further design. Passing type
checks does not prove that the provider will accept the state.

<a id="event-ir과-상태-전이"></a>

## Event IR and state transitions

`EventValidator` validates events of one response in order: start, item start,
content start/delta/end, tool-argument delta, item end, usage update and terminal
state. Input strings are assumed to have completed wire UTF-8 handling. SSE
framing, provider parsers and client event encoders belong to the adapters, not
this pure validator.

| State/input | Rule |
|---|---|
| Before start | Only Started is allowed |
| Item start | Reject duplicate item IDs, output indices and tool call IDs |
| Content delta | Only an open content block in an open item |
| Tool-argument delta | Accumulate per open tool item; partial JSON is allowed |
| Item end | No content may remain open; function arguments must be complete JSON |
| Successful completion | No item may remain open |
| Incomplete/failure/cancellation/transport loss | Can terminate in failure with open items |
| After terminal state | Reject further events and duplicate termination |

Preserve terminal state and an optional reason. HTTP EOF is never converted to
model Completed. Invalid events return an error without changing previous state.
Text is not accumulated. Default limits are 4096 items, 16384 content blocks,
1 MiB per delta and 8 MiB total tool-argument buffers. Usage is cumulative and
cannot decrease; missing counters retain prior values. Cross-provider token
equivalence and cost are not calculated.

<a id="검증과-후속-범위"></a>

## Validation and further scope

Tests cover synthetic round trips, instruction/tool relationships, numeric/string
preservation, missing capabilities/extensions, binding changes, wrapper restoration,
interleaved events and every string split. They run alongside native HTTP tests
and work without private records.

Messages and Chat Completions codecs, SSE conversion and HTTP dispatch passed
G12's actual-Codex synthetic suite. Host-owned history, local compaction and
recovery follow the separate [continuity contract](continuity.md). Gateway storage
and tenant authentication remain unsupported. Live-provider qualification and
consumer production acceptance are separate; a valid library declaration is not
operational acceptance.

References: [OpenAI custom tools](https://developers.openai.com/api/docs/guides/function-calling#custom-tools),
[Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming).

<a id="http-경로-선언-연결"></a>

## HTTP route integration

`Config::resolve_route` freezes declared API/model profiles into the existing
RouteSnapshot. `ResolvedRoute::admit` runs after shared HTTP stateless checks.
Native Responses preserves original JSON/SSE passthrough; converted routes derive
requirements and a TranslationPlan from validated RequestIR. Profile declarations
do not prove live-model qualification or credential generation. Namespace and
grammar bridges connect to Messages and Chat Completions HTTP routes.

Messages has a separate pure codec. The approved `MessagesInstructionEnvelope`
retains the text, role and position of leading instructions in the system area,
without claiming native role-priority equivalence. It can be declared only for
instruction_hierarchy in a Messages profile. See [Messages support](messages.md)
for requests, JSON responses and streams.

Optional tool-call status is preserved as ToolCallStatus. Messages history accepts
omitted/completed status only, never converting in_progress/incomplete to completed.
HTTP conversion reuses the TranslationPlan from route admission, derives required
features once and additionally verifies the adapter's implemented subset.

Chat Completions has separate pure request, JSON response and streaming codecs.
It shares tool identity, choice/count and output-restoration validation with
Messages, while each adapter retains its own roles and finish semantics. See
[Chat support](chat-completions.md).

Completed-message status and output_text annotations are also typed fields.
Converted history accepts completed/omitted status and empty/omitted annotations.
Meaningful annotations, unfinished messages and unknown extensions explicitly
reject. Text following tool use in the same assistant segment retains Messages
block order. Assistant content cannot be interleaved after only some tool results.
