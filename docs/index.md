<ProductIntro>

<a id="에이전트와-모델-사이의-명확한-연결"></a>

# A clear connection between agents and models.

[English](index.md) | [한국어](ko/index.md)

Extensible proxy software that makes Non-Responses model APIs available to
Responses applications. Its lightweight Rust core preserves declared request,
response, tool and streaming semantics through explicit conversion contracts.
Optional extensions and application configurations support standalone use,
embedding and backend integration. The current runtime uses local connections.

[Get started](../README.md) · [Explore the protocol](protocol.md)

</ProductIntro>

The product has three responsibility layers: a reusable Proxy Core, optional
extensions, and the application that selects and operates them. These layers do
not require separate repositories, crates or processes. Lightweight means that
unselected features do not impose their runtime dependencies or operational work
on the basic configuration.

| Layer | Responsibility |
|---|---|
| Proxy Core | Shared semantics, capability admission, API conversion, transport, cancellation and common validation |
| Optional extensions | Selected provider access, credentials, account pooling, continuity, usage and operational capabilities |
| Execution application | Configuration, startup, packaging, management experience and host or backend integration |

This describes the product direction. Current extensions provide metadata
observation and usage recording; account pooling and provider continuity services
need new contracts and implementation. Extensions supply implementations without
bypassing common capability, identity or terminal-state validation. New semantics
may require a new core or protocol version.

Standalone configurations select the required adapters and extensions. Embedded
configurations let a host supervise execution. Backend configurations connect
reusable authentication, policy and storage contracts. These are composition
choices, not evidence that every proposed configuration is available today.
See [integration](integration.md), [extension design](extensions-design.md) and
[development direction](roadmap.md).

Preserve the semantics declared as supported and disclose differences that cannot
be preserved. A bridge is not by itself proof of semantic equivalence, and wire
compatibility does not guarantee a model's instruction following or output quality.
The [IR contract](ir.md) distinguishes representation, conversion and guarantees.

<a id="하나의-진입점-명시적인-경로"></a>

## One entry point. Explicit routes.

Keep the client on Responses while selecting a declared upstream API. Native
forwarding preserves response bodies; converted routes enforce their documented
compatibility profiles.

<CardGrid kind="api" />

<a id="책임이-분명한-통합"></a>

## Integrate with clear responsibilities

In the current basic configuration, the gateway owns transport, routing and
upstream credential use. Your application owns tools, approvals and history.
Start with synthetic inputs
and mock upstreams before qualifying a real provider.

Build locally and validate your route configuration before starting the process.
This check reads configuration without calling a provider.

```sh
cargo build --locked
cp config.example.toml config.local.toml
cargo run --locked -- check-config --config config.local.toml
```

<DiagramFigure kind="ownership" />

<CardGrid :ids="['integration', 'continuity', 'codex-contract']" />

<a id="현재-지원-범위"></a>

## Know the supported boundaries

<SupportTable>

| Capability | Current contract |
|---|---|
| JSON and SSE | Native forwarding and explicitly profiled API conversion |
| Credentials | Local Bearer authentication with separate provider credentials |
| Tools and approvals | Executed and authorized by your application |
| Continuity | Host-owned history, local compaction and recovery |
| Remote state | Gateway storage, `previous_response_id` and remote compact API are unsupported |

</SupportTable>

<Callout variant="note">

Protocol tests run with mock providers. Before production use, validate the
selected real model and your application's permissions and recovery.

</Callout>

<a id="다음-단계"></a>

## Find your next step

<CardGrid :ids="['getting-started', 'packaging', 'contributing']" />
