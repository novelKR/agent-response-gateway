<a id="네이티브-확장-아키텍처"></a>

# Native extension architecture

[English](extensions-design.md) | [한국어](ko/extensions-design.md)

The [product overview](index.md) distinguishes core, optional extensions and
execution applications. Pooling, credential management and provider continuity
can belong to this project's optional modules without becoming mandatory core
dependencies. A standalone configuration may own their state; a backend
configuration may obtain policy or state through explicit external contracts.
These possibilities do not grant the current observer protocol new permissions.

Current observers and the Usage Recorder have different delivery contracts.
Observers receive best-effort numeric HTTP metadata; the recorder receives usage
events and can require local commit acknowledgements. Future providers, brokers
and continuity modules need role-specific interfaces. They must preserve common
capability admission, origin binding, item identity, event order and terminal
validation. This does not require dynamic adapter registration or a particular
repository or process layout.

The gateway supports explicitly installed, trusted native metadata observers and an optional usage recorder. This design keeps model transport in a small Rust core and separates package management from process execution. Account pooling, credential plugins and dynamic protocol adapters are future extensions, not capabilities of the current observer protocol.

<a id="선택한-방식과-대안"></a>

## Decision and alternatives

Use internal Rust modules for code organization and separately installed executables for optional functionality. A Cargo feature controls a build; it is not a user installation mechanism. The first public package role is a numeric metadata observer. It proves installation, permission approval, activation, identity binding and failure handling before any credential-bearing interface is opened.

Subprocesses allow independent package selection and avoid exposing Rust internal data layouts as a plugin ABI. This is not a guarantee that every later feature can update independently of the core. A new transport or security semantic can require a new core and protocol version. Native dynamic libraries, a WASM runtime, remote package discovery, hot reload and a public marketplace are not implemented. Consider sandboxed policies only after concrete extensions establish the required interfaces.

One installable feature can contain several internal modules. A future account-pool package need not make users install authentication, refresh, quota and scheduling components separately. Keep tightly coupled credential ownership within one reviewed lifecycle.

<a id="코어와-확장의-책임"></a>

## Core and extension responsibilities

```text
Consumer / agent
    |
    | Responses HTTP
    v
Gateway core ---------------------------------> Provider
    |  authentication, admission, transport       HTTPS / SSE
    |  capacity, cancellation, no implicit retry
    |
    +-- bounded metadata queue (no model bodies)
            |
       Extension supervisor
            |  private socket-backed stdin/stdout
            v
       Trusted observer executable
            |
       Per-package private state
```

The core retains final authority over local authentication, route/capability admission, provider destinations, credential use, body limits, streaming, cancellation and retry policy. The observer is not a second model proxy. It cannot request a tool call, reroute traffic, rewrite a response or acquire credentials through this protocol.

The supervisor sends only numeric HTTP status and elapsed time until response headers are available. This includes health checks and authentication failures; it excludes URL, model/route names, request IDs, headers, bodies, token usage and tool results. An HTTP 200 observation is not proof of successful model completion, and header timing is not full response duration. Observations are best effort, not billing or a durable audit trail.

The consuming application still owns tool execution, approvals, conversation history and reconciliation. Observer state does not implement Responses storage, response lookup, remote compact, background execution or account-bound resume. See the [embedding contract](embedded-design.md) and [continuity contract](continuity.md).

<a id="패키지와-권한-계약"></a>

## Package and permission contract

The offline manager reads a flat directory, not an archive or installation script. It verifies an exact manifest digest supplied through a trusted channel, the complete file inventory and each file digest. The canonical package is `gateway-extension-package/v1`; activation uses `gateway-extension-lock/v1`. Unknown fields, unsupported roles, duplicate identities, noncanonical JSON, invalid targets, symlinks and hard-linked package files are rejected.

Only the explicitly selected compiler input may have hard links during package preparation. Its bytes are copied to a new package inode. Installation and runtime checks still reject hard links. No inspection command executes package code, uses the shell, scans personal credentials or contacts a provider.

Requested permissions and granted permissions are distinct records. This version requires exactly `observe_http_metadata` and `write_private_state`; subsets or added permissions fail. The grant record authorizes the gateway interface and acknowledges native execution. It does not constrain arbitrary system calls. The manager serializes activation writes, increments a checked generation, and atomically replaces the private lock file.

A digest binds bytes, not a publisher identity. Signature verification, trust-root distribution and remote updates require separate implementation. Keep package provenance and applicable project/third-party notices with the exact version; a valid digest is not license clearance. See the [installation guide](extensions.md).

<a id="프로세스-수명과-장애-정책"></a>

## Process lifecycle and failure policy

Without `--extensions-lock`, no extension store is read and no observer starts. With it, `check-config` and `manifest` validate installed bytes without executing code. `serve` acquires exclusive runtime ownership of the store, checks executable integrity again, and starts each exact absolute executable with an empty inherited environment, a private working directory and socket-backed standard input/output. Child standard error is discarded; it is never forwarded as trusted diagnostics.

Startup requires the declared protocol handshake before gateway readiness. Failure of any requested observer at startup rejects that opted-in launch and cleans up already started direct children. After readiness, a malformed reply, exit or deadline failure stops that observer without disabling ordinary model routes. There is no automatic observer restart, observation replay or inference retry. Idle child exit can be noticed on the next observation or shutdown rather than immediately.

Each running process uses its loaded activation snapshot. Enabling, disabling or selecting another package while it runs only changes a subsequent start. Stop the gateway to stop its observers, then restart with the reviewed lock. One store has one runtime owner; the management lock is separate so next-start changes can be prepared during execution.

Shutdown signals all observer workers, stops delivery, then kills and waits for the owned direct children. Pending observations may be dropped. Protocol I/O deadlines do not bound an uninterruptible kernel wait or a descendant process tree. Trusted extensions must not daemonize or spawn descendants; the consuming host remains responsible for an outer process deadline. Abrupt gateway termination and hostile native code are not covered by a process-tree containment guarantee.

<a id="관측-메시지와-제한"></a>

## Observer messages and limits

```json
{"type":"ready","protocol":"gateway-observer/v1"}
{"type":"http","sequence":1,"status":200,"headers_ms":8}
{"type":"ack","sequence":1}
```

These are separate newline-delimited UTF-8 JSON messages: child readiness, host observation, then child acknowledgement. The sequence starts at one per process and acknowledgement must match exactly. Unknown message types and fields fail; credentials and reverse callbacks are unsupported.

| Resource | Implemented bound |
|---|---|
| Enabled observers | 4 |
| Queue per observer | 64 observations; nonblocking drop on full/disconnected |
| Reply frame including newline | 4,096 bytes |
| Startup handshake | 3 seconds per observer, started sequentially |
| Observation write and acknowledgement | One shared 1-second deadline |
| Idle supervisor poll | 50 milliseconds |
| Package or activation JSON | 65,536 bytes |
| Executable / each notice file | 128 MiB / 256 KiB |
| Listed package files | 2–8, excluding the manifest |

The frame deadline covers partial input and the complete write/read exchange. A fast sender cannot extend it indefinitely by trickling bytes. Queue bounds are not OS memory, CPU or network quotas for native code. More observers increase aggregate startup and resource costs. The example persists only counters; observations never wait for its disk writes on the HTTP path.

<a id="신뢰와-실행-정체성"></a>

## Trust and execution identity

Native code is trusted code, not an OS sandbox. Empty environment inheritance and narrow messages reduce accidental disclosure; they do not stop an executable from reading user files, opening the network or attacking another same-user process. Permission declarations cannot enforce those prohibitions. Run as the intended unprivileged user, protect store ancestors from other users, and do not treat this loader as protection against hostile same-user mutation. Windows native execution fails explicitly until equivalent filesystem, lock and IPC contracts are implemented; the extension-free gateway remains available on its existing targets.

Observer-only launches use `gateway-extended-manifest/v1` and `gateway-extended-ready/v1`. The `execution_sha256` binds the base manifest and extension configuration: absolute store location, activation generation, exact package manifests/digests and approved grants. Mutable counter files are excluded. The base `configuration_sha256` continues to describe gateway configuration alone.

This combined digest does not hash the gateway executable or attest a publisher. The consuming host must verify the core binary separately and bind both identities. Validate the extended schema and compare the offline `execution_sha256` with readiness before starting work. Older consumers must reject an unsupported extended schema, not validate only the old configuration field and ignore active code.

This is not an automatic migration of the existing host continuity record. Hosts using observers must retain the extension execution identity alongside their existing verified run binding. A later credential-bearing extension requires an explicit versioned continuity change; an environment name or opaque handle alone is not proof of credential ownership.

<a id="codex-pool-확장-제안"></a>

## Codex Pool extension proposal

```text
Future account-pool request (proposal; not observer/v1)

Host binds route + account realm + policy revision
    -> Core admits request and validates existing session binding
    -> Pool proposes an eligible account and credential lease
    -> Core validates destination, entitlement, binding and attempt budget
    -> Core sends one qualified model request directly to the provider
    -> Core reports a classified outcome; credential owner releases the lease

Unknown acceptance / partial output
    -> no transparent replay
    -> explicit host reconciliation or a new portable-context thread
```

Account pooling is a separate proposed role, not an extra permission that can be added to the observer manifest. Its broker needs request-owned credential leases, model/workspace eligibility, quota evidence and account-bound sessions. Core transport must remain outside the pool process; do not forward every SSE fragment through a scheduler.

One authoritative owner must refresh each credential grant. Generation-checked writes must fence deleted, replaced or reauthenticated accounts and late quota/health results. Unknown or stale quota is not zero usage. Shared limits stay shared; selecting another credential does not grant permission to evade workspace or provider restrictions.

New-work account selection and migration of an established conversation are different operations. Do not assume encrypted reasoning, compaction blobs or provider-local identifiers move across accounts. Lost or expired bindings after restart require an explicit host transition or a new thread with verified portable context. Token revision and account/workspace origin are separate concepts; their compatibility needs a new reviewed schema, not a reinterpretation of existing checks.

A pool crash is not evidence that inference did no work. Preserve no-retry defaults and explicitly classify pre-dispatch failure, verified rejection and uncertain acceptance. Partial SSE, exposed tool output and cancellation must not trigger transparent replay. Any later failover policy needs an attempt budget and total deadline coordinated with the host.

Separate token/schema migration from executable rollback. Never run two refresh owners for one grant or restore obsolete credentials merely to return to an old binary. Do not automatically import the user's ordinary Codex login. These requirements remain unimplemented until the credential and continuity contracts are reviewed and separately tested.

<a id="검증과-후속-확장"></a>

## Validation and evolution

The [manager](../scripts/extension_manager.py), [Rust contracts](../src/extensions/mod.rs), [supervisor](../src/extensions/runtime.rs) and [native smoke probe](../scripts/extension_smoke.py) are the implementation sources. The smoke probe checks actual executables with synthetic loopback traffic; success is not real-provider qualification or permission to run an arbitrary third-party package.

Keep the first role narrow. Extend contracts only for a demonstrated need: a pool requires credential and continuity review; an external wire adapter requires independent streaming/backpressure/cancellation validation; untrusted code requires real isolation. Broader SDK, signatures, remote discovery, Windows supervision and a management UI remain separate work. Keep third-party rights and corresponding-source requirements explicit when distributing extensions; subprocess separation is not a license determination.

Usage accounting and the optional recorder are described in the
[token usage accounting guide](usage-accounting.md). Recorder installation,
local commit guarantees and external delivery are separate from HTTP metadata observation.
