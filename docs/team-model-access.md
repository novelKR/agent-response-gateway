<a id="관리형-세션-소유권"></a>
<a id="사용량-증거와-실패-의미"></a>
<a id="인증대상전송"></a>
<a id="초기화백업검증"></a>
<a id="팀-주체별-모델-진입점"></a>

# Scoped team model entry point

[English](team-model-access.md) | [한국어](ko/team-model-access.md)

`gateway-team-http` is an optional HTTP router for
[named team credentials](team-access.md), model admission, managed-session ownership
and exact usage correlation. A trusted host explicitly provides its authentication
adapter, registered Gateway peer, separate request ledger and optional Recorder
reader. The default Gateway does not depend on this package. Constructing the
router starts no listener, provider, Gateway process or recurring task. Standalone
process ownership and distribution assembly require their own host configuration.

## Authentication, target and transport

The credential store, request ledger and each current peer must name the same
registered target. A peer contains a verified instance/configuration identity,
optional observed Recorder producer ID, exact route aliases and a numeric loopback
HTTP endpoint. Gateway model and continuation-control credentials are separate,
private, zeroizing values; Team token prefixes are refused for these credentials.
They never come from a model request URL, body, role or subject field.

The host implements `PeerSource` or supplies a fixed verified peer. It withdraws or
replaces that peer when its owned execution changes. The entry point does not
adopt a process from its PID or port, discover a service, or restart a Gateway.
Already admitted requests retain their peer identity; new calls check the current
registration. A stopped, mismatched or unavailable peer is an explicit error.

Only Model-purpose credentials authenticate this surface. Management and ReadOnly
keys, browser management cookies and injected `x-gateway-*` headers cannot authorize
it. Authorization versions are refreshed after acquiring the admission writer and
before effects. Exact route permission filters both calls and the actual Gateway
model list. The host-selected bind is numeric loopback, Host must match its address
and port, and any Origin must match the same HTTP origin. External HTTPS/identity
systems remain a separate host boundary.

| Path | Method | Behavior |
|---|---|---|
| `/v1/models` | GET | Read actual Gateway models, filtered by registration and subject permission |
| `/v1/responses` | POST | Forward one admitted JSON request; stream response data without model retries |
| `/team/v1/usage` | GET | Own request/usage evidence; all-team scope requires separate permission |
| `/team/v1/sessions` | POST | Create an owned managed session using a model and idempotency key |
| `/team/v1/sessions/{id}` | GET | Read only that subject's authorized session status |

Responses input and protocol compatibility remain owned by Core. Its existing
stateless policy refuses storage/background calls, upstream conversation/history
references and compaction. Team forwarding neither strips those semantics nor adds
another native-history heuristic. Other endpoints, WebSocket, image/audio entry
points, internal control routes and session transitions are not exposed here.

Requests use only the registered local Gateway credential and controlled headers.
The client disables redirects, environment proxies and retries. A team request ID
is committed before calling Gateway; successful response headers and failures before
Gateway headers expose `x-team-request-id`. A valid Gateway `x-request-id` can be
returned separately. Neither HTTP success nor a received ID establishes model
completion. Missing IDs are never reconstructed from model names or time proximity.

The defaults admit 32 in-flight requests and at most 64 blocking jobs, with 2 MiB
request and 16 MiB response limits, a 30-second request-body deadline and 120-second
header/stream-idle deadlines. Hosts can select bounded alternatives through
`Limits`. Dropping a stream cancels its upstream body before final evidence work.
EOF, client disconnection, connection failure, body loss, idle/header timeout and
response-size exhaustion are distinct transport observations. None becomes a
fabricated model outcome, usage value or billable completion.

## Usage evidence and failure semantics

The separately initialized `team-requests.sqlite3` uses the existing SQLite version,
WAL/FULL and a single writer. Immutable records retain admission identity/permission,
route, peer identity, timestamps, header correlation and transport end. It stores
no model body, prompt, tool content, credential value or inferred token count.
It is separate from credential, management audit, continuation and Recorder stores.

A producer/request pair has one team-request owner. `SqliteUsage` reuses the existing
Recorder's explicit read-only open and current-event query. It looks up the exact
producer and returned Gateway request ID, retains attempt IDs and checks route and
configuration identity. Duplicate attempts or mismatched evidence are unobserved,
not double-counted or reassigned. There is no Recorder schema migration or delivery
worker here. The built-in `SqliteUsage` adapter and its Recorder dependency are
available only on Linux/macOS. Other platforms report this capability unsupported
without opening a store; a host can explicitly supply another `UsageReader`.
Model transport and generic correlation remain independently available. Native
Recorder support is not expanded to Windows.

Usage queries take `from_ms`, `to_ms`, optional `after` and optional `all`. A query
covers at most 366 days and returns at most 100 request records by ascending cursor.
Its window is explicitly **team admission time**, distinct from the Recorder's
attempt-start aggregation clock. Each request contains original canonical attempt
observations, including null counters, source/finality flags, incomplete observation
and upstream/Gateway outcomes. No per-user charge or quota is calculated.

The default scope is the authenticated subject. `all=true` requires its explicit
`read_all_usage` permission and covers retained team requests; it does not claim
ownership of other Gateway usage. A supplied subject filter is rejected. Responses
are bounded to 2 MiB; one request lookup is bounded to 16 attempts. Recorder failure
stops further lookups in that query while retaining unobserved reasons.

Missing producer/request correlation is `unattributed`. A disconnected Recorder,
empty observation or invalid correlation is explicitly `unobserved`. An admission
write failure prevents the Gateway call. A later correlation/end-record failure
must not replay or discard a running model response: its missing evidence remains
unconfirmed. The active-request guard also belongs to the blocking admission result,
so a cancelled caller cannot leave an orphan marked active. Reopening a ledger does
not replay unfinished requests or assume their completion.

## Managed-session ownership

Create a session with `{ "model": "writer", "idempotency_key": "new-session-1" }`.
The entry point records an intent, then uses the separate host control token with
the configured route origin. The public opaque session ID maps to a Core session
UUID, subject, exact route and origin digest. It validates the returned control
observation before binding it and exposes only scoped status metadata.

Pass the public ID in `x-team-session` for a managed model call. Another subject's
ID, a different route or a changed origin rejects. Model clients never supply the
internal `x-gateway-session` header or control token. Core still verifies encrypted
replay against its session/origin and owns pending tool and continuation state;
this entry point does not approve tools or execute session transitions.

A failed or lost creation result stays `unconfirmed`. Retrying the same idempotency
key never automatically creates another Core session. A bound retry returns the
same public ID. Session status preserves unknown/pending states, epoch and revision;
it does not expose the control token, internal ID or full origin configuration.
Host reconciliation and explicit new-session decisions remain separate actions.

## Initialization, backup and verification

Initialize existing private directories through `Ledger::initialize`; opening an
existing ledger requires the exact target and supported schema. The initial limits
are 1,000,000 retained request records and 16,384 session intents plus the configured
store size bound. There is no automatic deletion, schema repair, retention policy
or conversion of an existing store. `Ledger::backup` creates a coherent private
SQLite backup without overwriting another file. Restore compatible verified code
and coherent related stores; do not interpret restoration as model request replay.

Build the Gateway binary before the actual-process fixture:

```sh
cargo build -p agent-response-gateway --locked
cargo test -p gateway-team-http --locked
cargo clippy -p gateway-team-http --all-targets --locked -- -D warnings
```

Tests use actual Core routers, a managed Gateway subprocess, synthetic credentials,
mock providers and the existing Recorder storage implementation. They verify
subject/route separation, encrypted-session replay refusal, exact ownership of
usage, null/partial observations, stream cancellation/failures, interrupted
admission, unavailable evidence and explicit backup/reopen behavior. Fixture tests
are separate from a supplied standalone service, external deployment or consumer
operational acceptance. Installing or enabling this optional module must remain an
explicit host decision; an ordinary Gateway starts none of its resources.
