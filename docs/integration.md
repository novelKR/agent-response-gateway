<a id="consumer-integration-boundaries"></a>
<a id="소비자-통합-경계"></a>
<a id="애플리케이션-통합"></a>

# Application integration

[English](integration.md) | [한국어](ko/integration.md)

<Callout>

Use the gateway for model requests while your application manages tools, approval
and history. The diagram below shows how tool calls return to the application.

</Callout>

<DiagramFigure kind="tool-roundtrip" />

For concrete request/response and tool-result examples, follow the
[usage guide](usage.md). Diagnose startup, authentication, capacity and streaming
failures with [troubleshooting](troubleshooting.md).


<a id="공통-경계"></a>

## Shared responsibilities

The gateway owns HTTP transport, declared model routes, provider credential
selection and API compatibility. Consumers own tool execution, user approval,
business data and workflows. Do not add consumer-specific domain types or
repository paths to the gateway.

`/v1/models` is a configuration list, not a capability certificate. `/readyz`
reports local readiness, not a live-model call, consumer test or operational
acceptance. Public fixtures contain synthetic inputs only.

Native package targets are Linux x64/ARM64, macOS ARM64 and Windows x64.
Select the [package format](packaging.md) and use the
[platform shutdown contract](embedded-design.md#process-lifecycle) when supervising
its executable. Packaging does not add remote service or installer behavior.

Execution is currently loopback-only. An HTTP consumer can run on the same host
or in the same network namespace. Communication between separate Docker
containers and exposing a public service are outside this version's support.



Interactions requires a host-created session, independent control token, stable
protection key and private SQLite store. The host pins a session header in the dedicated
Codex provider configuration and owns explicit recovery/compaction decisions. Follow
the [Interactions host contract](interactions.md) for initialization, backups and
compaction into a fresh Codex thread. This adds provider-state persistence without
moving tool execution or approval into the gateway.

<a id="에이전트-런타임에-내장"></a>

## Embedding in an agent runtime

The application's runtime manager starts the gateway and connects the actual
readiness address to the agent's dedicated provider configuration. A Codex App
Server host also keeps that configuration separate from the user's personal
settings. The host owns file, network and tool permissions and user approval.

Pin the agent executable and protocol schema, gateway version and source commit,
and the platform executable hash. Bind compatibility results to the exact verified
combination. An update failure restores the previously verified combination.

Test structured output, real tool round trips, cancellation/disconnection, model
selection and approval denial with the selected agent version. Extending existing
provider authentication or qualification belongs to the consumer integration.
Hosts own Codex history, local compaction, resume and uncertain-request recovery
under the [continuity contract](continuity.md). Classify only the verified contract
and consumer execution path as eligible for long-running work.



<a id="백엔드-서비스에서-호출"></a>

## Calling from a backend service

The consumer workflow service owns execution order and resume. Its credential
and external-call layers own key selection and egress policy. Adopting the gateway
does not transfer those duties or bypass existing authorization. Choose the
integration point and authentication contract after inspecting the consumer's
actual call path.

| Credential ownership | Current scope and further decisions |
|---|---|
| Service-owned provider keys | Can be registered in process configuration; connect the consumer's external-call authorization and audit path |
| Per-tenant provider keys | Not implemented; first define tenant authentication, key selection, permissions, billing and audit contracts |

The current version accepts no per-request API-key injection or tenant routing.
A single local Bearer token and process-wide provider configuration are not a
per-tenant security boundary. Adding service deployment requires decisions about
network addresses, authentication, key ownership, egress, cancellation and error
propagation. Follow the consumer's existing deployment and environment management.



A synthetic backend example uses an application with authentication, policy and
business storage services. The application checks access and workflow policy,
then calls the configured loopback gateway on the same host or in the same
network namespace. This does not introduce a public gateway service.

```text
Application access and policy checks
    -> Local Responses gateway -> Configured model provider
    <- Responses output / tool calls
Application tool approval, execution and business storage

Gateway usage -> Optional Usage Recorder -> PostgreSQL / HTTP collector
```

The backend owns tool execution and business approval. Its trusted deployment
supplies process-configured provider keys; client-supplied tenant identity does
not select credentials. The recorder's [generic export contract](usage-accounting.md)
is optional and separate from workflow storage. This example claims no deployed
integration or new endpoint.

| State | Ownership in this example |
|---|---|
| Business data and approval records | Backend application |
| Conversation history and recovery | Host under the current continuity contract |
| Provider-specific continuation | Opt-in gateway continuation under host-owned origin and recovery contracts; independent module deployment remains future work |
| Public Response objects and lineage | Future optional service with access and retention contracts |
| Usage ledger and delivery outbox | Optional Usage Recorder |

Dynamic credential leases, tenant-specific selection, account pools and provider
continuity are extension directions, not capabilities enabled by this example.
Standalone applications may eventually supply these modules themselves; an
external policy service is not a mandatory controller of the core. See the
[product overview](index.md) for composition choices.

<a id="further-acceptance"></a>
<a id="통합-검증"></a>
<a id="후속-수락"></a>

## Integration verification

Test the selected agent or backend through its actual call path, including
credentials, model selection, tool approval, cancellation and recovery.
Mock-provider tests cover the reusable contract; production use also requires
the real model and the application's deployment to be verified.



<a id="경로-선언과-입력-한도"></a>

## Declared routes and input limits

Hosts verify the declared API, authentication and model profile alongside Codex
settings. A per-request RouteSnapshot is a routing declaration, not authorization
to resume stored history. Connect the profile's context_window to host compaction
settings; actual input-token admission needs model-specific counting evidence.
Output limits apply before HTTP dispatch. Messages and Chat Completions can be
enabled with explicit profiles. The [conformance suite](conformance.md) tests these
profiles using actual Codex and mock providers. Validate the real model's output,
tools and context behavior in the application before operational use.

The Messages instruction bridge is explicit profile opt-in. The host must qualify
its limits relative to native role separation. Model interpretation of instructions
does not replace real tool permissions or user approval.

Converted routes provide per-request namespace/custom-tool mappings and bounded
stream conversion. Hosts must not treat a grammar bridge as native constrained
decoding. Syntax checking, approval and file execution are separate responsibilities.
See [Messages support](messages.md) for the verified host profile and byte bounds.



<a id="내장-프로세스-manifest"></a>
<a id="내장-프로세스-명세"></a>

## Embedded process manifest

`manifest --config <path>` emits `gateway-embedded-manifest/v1` JSON without reading
credentials or accessing the network. It normalizes resolved routes, profiles,
limits and environment-variable references and includes configuration_sha256.
No key values or history are included. Hosts retain real configuration addresses
and identifiers privately. A digest is not a signature or provider qualification.

Readiness contains schema, manifest_schema, configuration_sha256, event, address,
base_url and version. Read this from the verified child process's stdout, compare
it with the offline manifest, and only then start Codex. Changing raw TOML after
inspection must change readiness when its effective configuration differs.
Changing credential values cannot be detected from a configuration-reference
digest alone; resume needs a separate credential generation. Follow the
[embedded contract](embedded-design.md) for lifecycle, access, failure and recovery.

Usage accounting and the optional recorder are described in the
[token usage accounting guide](usage-accounting.md). Recorder installation,
local commit guarantees and external delivery are separate from HTTP metadata observation.
