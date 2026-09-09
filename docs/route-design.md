<a id="g05--model-routes-and-capability-admission"></a>
<a id="g05--모델-경로와-기능-admission"></a>
<a id="모델-경로와-지원-기능-검사"></a>

# Model routing and capability checks

[English](route-design.md) | [한국어](ko/route-design.md)

A model alias selects a configured provider, actual upstream model and API. Consumers
call the alias through the Responses endpoint; the gateway selects the provider key
and applies the route's capability checks before contacting the provider.

<a id="소비자용-모델-이름"></a>

## Consumer model names

The consumer sends a registered alias in the request's `model` field. Its
`provider` setting selects an entry in `providers`, and `upstream_model` is the
model ID sent to that provider. Each provider entry holds one `base_url` and one
`api_key_env` reference. A provider entry represents a connection and credential
binding; several entries can refer to the same provider service.

For example, suppose `A-Provider` offers `Model-L`, `Model-M` and `Model-S`.
Either naming scheme below can be exposed to consumers:

| Upstream model | Alias with provider name | Alias by size |
|---|---|---|
| `Model-L` | `A-Provider/Model-L` | `Large-Model` |
| `Model-M` | `A-Provider/Model-M` | `Medium-Model` |
| `Model-S` | `A-Provider/Model-S` | `Small-Model` |

The following complete configuration registers both schemes at once. Keep the
aliases you intend to offer. The provider, URL and model names are illustrative;
replace them with a verified provider endpoint and model IDs. These examples use
the default Responses API and Bearer upstream authentication.

```toml
listen = "127.0.0.1:0"
local_token_env = "ARG_LOCAL_TOKEN"

[providers.A-Provider]
base_url = "https://api.a-provider.example/v1"
api_key_env = "A_PROVIDER_API_KEY"

[models."A-Provider/Model-L"]
provider = "A-Provider"
upstream_model = "Model-L"

[models."A-Provider/Model-M"]
provider = "A-Provider"
upstream_model = "Model-M"

[models."A-Provider/Model-S"]
provider = "A-Provider"
upstream_model = "Model-S"

[models.Large-Model]
provider = "A-Provider"
upstream_model = "Model-L"

[models.Medium-Model]
provider = "A-Provider"
upstream_model = "Model-M"

[models.Small-Model]
provider = "A-Provider"
upstream_model = "Model-S"
```

Set `A_PROVIDER_API_KEY` and a separate `ARG_LOCAL_TOKEN` in the gateway process
environment. Save the configuration as `config.local.toml` and follow the
[startup steps](../README.md#getting-started). Keys do not belong in the TOML file.

Aliases are exact, case-sensitive lookup keys. The gateway does not split
`A-Provider/Model-L` at the slash or infer a provider from its prefix. Nor does
`Large-Model` automatically select the largest available model. Each name follows
its explicit mapping. Different aliases can resolve to the same provider and
model; an unregistered alias returns `404 model_not_found` without an upstream call.

<a id="모델-호출"></a>

### Call a model

Use the gateway's local token and readiness address. Replace the example port
below with the port reported by the running gateway.

```sh
curl --noproxy '*' http://127.0.0.1:43127/v1/responses \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"Large-Model","input":"Reply with hello.","store":false,"stream":false}'
```

For this configuration, the gateway sends `model: Model-L` to the provider's
`/v1/responses` endpoint using the key referenced by `A_PROVIDER_API_KEY`.
Changing the request's model to `A-Provider/Model-L` selects the same destination
and key. `Medium-Model` and `Small-Model` select the other declared models.
The local token is replaced by the selected upstream key, and consumer-supplied
provider authentication headers are not forwarded.

List the names available to the consumer with:

```sh
curl --noproxy '*' http://127.0.0.1:43127/v1/models \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}"
```

This lists all six aliases in the example, without the provider URL or key
references. It does not query the provider's model catalog. On native Responses
routes, the response body's `model` is not rewritten to the alias: a request for
`Large-Model` can return `Model-L`. Keep the requested alias if the consumer needs
it for display or its own request records.

<a id="같은-공급자의-여러-api-key"></a>

## Multiple API keys for one provider

To call the same provider and model with different keys, register one provider
entry per key and connect a separate model alias to each entry. Each entry has
one `api_key_env`; a model cannot override that reference or select from a key pool.

The following is a separate complete example for two keys with the same endpoint
and `Model-L`:

```toml
listen = "127.0.0.1:0"
local_token_env = "ARG_LOCAL_TOKEN"

[providers.A-Provider-key-a]
base_url = "https://api.a-provider.example/v1"
api_key_env = "A_PROVIDER_KEY_A"

[providers.A-Provider-key-b]
base_url = "https://api.a-provider.example/v1"
api_key_env = "A_PROVIDER_KEY_B"

[models.Large-Model-key-a]
provider = "A-Provider-key-a"
upstream_model = "Model-L"

[models.Large-Model-key-b]
provider = "A-Provider-key-b"
upstream_model = "Model-L"
```

Set both `A_PROVIDER_KEY_A` and `A_PROVIDER_KEY_B`, along with the local token,
before starting this configuration. The consumer selects the key through the alias:

| Request model | Provider entry | Upstream model | Key reference |
|---|---|---|---|
| `Large-Model-key-a` | `A-Provider-key-a` | `Model-L` | `A_PROVIDER_KEY_A` |
| `Large-Model-key-b` | `A-Provider-key-b` | `Model-L` | `A_PROVIDER_KEY_B` |

For example, this request selects the second key:

```sh
curl --noproxy '*' http://127.0.0.1:43127/v1/responses \
  -H "Authorization: Bearer ${ARG_LOCAL_TOKEN}" \
  -H 'Content-Type: application/json' \
  -d '{"model":"Large-Model-key-b","input":"Reply with hello.","store":false,"stream":false}'
```

Key values are loaded when the gateway starts. Updating an environment variable
does not reload a running instance. The request has no separate `provider` or
`credential_id` selector; routing uses the registered `model` alias. `auth` controls
the upstream header format, not which key is selected. Authentication failures or
rate limits do not trigger a retry with another registered key.

A local Bearer token permits access to every configured alias in that instance.
Choosing a key through an alias does not enforce per-user or per-tenant permissions.
See the [access scope](embedded-design.md#authentication-and-access) and
[integration contract](integration.md#calling-from-a-backend-service).

The same alias and key selection applies to Messages and Chat Completions routes.
Those routes also require the explicit API, authentication and capability settings
described below. Each profile must match the selected provider entry and actual
model, including when separate provider entries use different keys for one service.

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
