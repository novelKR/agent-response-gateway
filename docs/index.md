<ProductIntro>

<a id="에이전트와-모델-사이의-명확한-연결"></a>

# A clear connection between agents and models.

[English](index.md) | [한국어](ko/index.md)

A local Responses gateway for agent runtimes and backend services. Declare a
route, separate your credentials, and carry JSON or streaming responses through
one interface.

[Get started](../README.md) · [Explore the protocol](protocol.md)

</ProductIntro>

<a id="하나의-진입점-명시적인-경로"></a>

## One entry point. Explicit routes.

Keep the client on Responses while selecting a declared upstream API. Native
forwarding preserves response bodies; converted routes enforce their documented
compatibility profiles.

<CardGrid kind="api" />

<a id="책임이-분명한-통합"></a>

## Integrate with clear responsibilities

The gateway owns transport, routing and upstream credentials. Your application
owns tools, approvals and conversation continuity. Start with synthetic inputs
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
| Tools and approvals | Executed and authorized by the consumer |
| Continuity | Host-owned history, local compaction and recovery |
| Remote state | Gateway storage, `previous_response_id` and remote compact API are unsupported |

</SupportTable>

<Callout variant="note">

Synthetic tests and pinned Codex conformance provide repeatable evidence.
Real-provider qualification, production activation and formal release remain
separate acceptance stages.

</Callout>

<a id="다음-단계"></a>

## Find your next step

<CardGrid :ids="['getting-started', 'packaging', 'contributing']" />
