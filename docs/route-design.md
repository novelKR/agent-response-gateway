<a id="g05--model-routes-and-capability-admission"></a>
<a id="g05--모델-경로와-기능-admission"></a>
<a id="모델-경로와-지원-기능-검사"></a>

# Model routing and capability checks

[English](route-design.md) | [한국어](ko/route-design.md)

A model alias selects one configured provider, upstream API and capability profile.
The gateway fixes that selection for the request and rejects unsupported conversion
requirements before contacting the provider.

<a id="evidence-and-recommendation"></a>
<a id="경로-처리-규칙"></a>
<a id="근거와-권고"></a>

## Routing rules

Responses routes preserve the original JSON values and SSE stream within the
[HTTP contract](protocol.md). Messages and Chat Completions routes use the
[intermediate representation](ir.md) to check features and convert protocols.
Keeping these paths separate lets Responses forward fields that the converted
APIs cannot represent.

A capability profile describes direct support, support through a conversion rule,
or no support. Declaring a feature cannot enable an unimplemented conversion.

<a id="public-configuration-additions"></a>
<a id="공개-설정-추가"></a>
<a id="설정"></a>

## Configuration

| Field | Meaning |
|---|---|
| `api` | `responses` by default, `messages`, or `chat_completions` |
| `auth` | `bearer` or `api_key`; defaults to bearer for Responses and must be explicit for converted routes |
| `capability_profile` | Reference into `capability_profiles`; required for converted routes |
| `messages_version` | Upstream version header required for Messages |

Profiles declare their version, provider, model, API, features, context window,
output limit and tested Codex version. Missing feature support means unsupported.
Profile and model mappings must agree; unknown fields and invalid references fail
configuration validation.

The operator sets the provider base URL and `api_key_env`. The selected API appends
`/responses`, `/messages` or `/chat/completions`. Authentication uses Authorization
for `bearer` or `x-api-key` for `api_key`. Messages also sends its declared version
header. Clients cannot supply new upstream URLs, keys or arbitrary headers.

`/v1/models` lists configured aliases. It does not test model availability or
capabilities. Connections remain loopback-only, without implicit proxies,
redirects, retries or fallback routes.

<a id="runtime-flow-and-ir-additions"></a>
<a id="실행-흐름과-ir-추가"></a>
<a id="요청-처리"></a>

## Request processing

1. Validate local authentication, request size and stateless request constraints.
2. Resolve the alias once. Fix its provider, model, API, credential reference,
   adapter/profile versions and limits for the request.
3. Forward a Responses request under the original JSON/SSE contract. A route
   without a profile provides unqualified passthrough.
4. For a converted route, decode the request, determine required features and
   apply only declared conversion rules. Reject missing support before dispatch.
5. Encode one upstream request and validate the response events. A failure after
   streaming starts closes the stream without manufacturing completion.

Tool groups preserve their namespace, member names, descriptions and order.
Converted names use one reversible mapping shared by definitions, choices, calls
and results. Duplicate effective identities and nested namespaces are rejected.

Custom text tools use an explicit JSON wrapper. Required patch grammar is selected
by its exact hash and version, not by a tool name. The supported grammar and limits
are listed in [Messages](messages.md). Validation checks generated syntax only;
the host owns file access, approval and execution. Unknown grammar or invalid
arguments fail the response before successful tool completion is emitted.

<a id="codex-transport-fields-and-unsupported-semantics"></a>
<a id="codex-전송-필드와-미지원-의미"></a>
<a id="프로토콜-필드"></a>

## Protocol fields

| Input | Converted-route handling |
|---|---|
| model, stream, store | Route and transport selection; `store:false` |
| instructions, ordered input, tools, choices, limits | Typed conversion and capability checks |
| client_metadata, prompt_cache_key | Transport/cache hints; omitted from the other API and never interpreted as instructions |
| include reasoning.encrypted_content | Optional opaque-output request; converted routes provide no opaque output |
| Opaque reasoning input | Cross-protocol reuse rejected |
| Hosted web/tool search | Unsupported |
| Unknown extensions or include values | Rejected |

Required output formats, images, reasoning options and parallel-tool restrictions
are not dropped to obtain a successful response. Use a host profile compatible
with the selected route's documented features.

<a id="model-limits-and-ownership"></a>
<a id="모델-한도"></a>
<a id="모델-한도와-책임"></a>

## Model limits

The gateway checks requested maximum output tokens against the configured model
limit before dispatch. Byte limits and token limits are separate.

`context_window` supplies the host's context and compaction settings. The gateway
does not count input tokens. The host must use a model-specific counter or estimator
and reserve enough context for output and compaction.

<a id="validation-compatibility-and-rollback"></a>
<a id="검증과-복구"></a>
<a id="검증호환성복구"></a>

## Validation and recovery

Tests cover original Responses forwarding, precise JSON values, raw SSE,
namespace identities and conversion failures. Missing features, invalid tool
relationships, excessive output limits and incompatible opaque state must be
rejected before an upstream request.

The [Codex conformance suite](conformance.md) checks the declared profiles against
mock providers. Real-model behavior and host operation require their own tests.
Restore a verified executable/configuration pair when reverting an integration;
there is no persistent gateway state to migrate.
