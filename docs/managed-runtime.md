<a id="감사-증거와-외부-변경"></a>
<a id="검증과-플랫폼-제한"></a>
<a id="소유-수명과-readiness"></a>
<a id="소유-실행-및-구성-adapter"></a>
<a id="호스트-등록과-구성"></a>

# Owned runtime and configuration adapter

[English](managed-runtime.md) | [한국어](ko/managed-runtime.md)

The optional `gateway-management-runtime` library stages and selects validated
configuration snapshots and supervises its own `gateway-managed-child` process.
It depends on the gateway and management contracts. The default gateway has no
reverse dependency and keeps its existing CLI, loopback listener and archive.
A management HTTP server, command interface and dashboard require separate
composition; installing this adapter does not provide them automatically.

## Host registration and configuration

A trusted host registers one target, a private state directory, an absolute child
executable with its SHA-256, local configuration source IDs and optional extension
and profile-pack lock paths. The host supplies an explicit credential environment
and a credential-generation ID. Registration is not an HTTP request schema.
Untrusted commands cannot select a program, shell, PID or arbitrary server path.

`Runtime::initialize` creates a new private store and refuses to overwrite state artifacts.
`Runtime::open` requires `gateway-runtime-state/v1` without migration or repair.
Only one manager holds the directory lease. Windows host ACLs must make it private;
Unix permissions and owner-matched regular files are checked. Symlinks, reparse
paths and parent-path traversal reject. Protect registered binaries and sources
against concurrent replacement by other host administrators.

`Command::Stage` checks the exact source-byte digest, uses the existing startup
parser and preserves an immutable private candidate. Candidate IDs select those
bytes; they are never used directly as filenames. Raw configuration is limited to
1 MiB and at most 128 candidates are retained. `Command::Select` validates a
candidate again and advances the saved revision. Selecting an earlier candidate
is an explicit new operation, not database reversal or request replay.

Native extension and profile-pack locks retain their existing startup semantics.
The selected raw configuration is frozen, while changes in registered activation
locks affect its next validation and launch. Desired configuration and execution
digests are separate from the digests reported by the owned running process.
Selection alone never starts, stops or restarts a process.

## Owned lifecycle and readiness

`Command::Start`, `Command::Stop` and `Command::Restart` operate on the actual
owned child handle. An occupied port or a held runtime lease is not ownership.
A reopened controller never adopts an existing PID or endpoint. Status distinguishes
`owned`, `stopped` and `unowned`; absent readiness is unknown, not a zero digest.

The private stdin protocol `gateway-managed-launch/v1` supplies the registered
launch and expected configuration/execution digests. The child validates them
before credential resolution and binding. It uses the ordinary gateway router,
provider transport and extension runtime. The parent checks the complete
`gateway-managed-process/v1` envelope and gateway readiness, including instance ID,
numeric loopback address, fixed or ephemeral port, schemas and effective digests,
before publishing an endpoint. Duplicate or unknown readiness fields reject.
The existing manifest/readiness versions through v7 remain separate contracts.

Only the selected configuration's local credential, provider credential and
continuation credential references are passed. The child does not inherit the
manager environment, proxy variables or unrelated management/team keys. A trusted
host must supply distinct credential realms and advance its credential-generation
ID when bindings change. Secrets never appear in the launch frame or status.

Parent-channel EOF or a stop frame initiates the configured gateway shutdown grace.
The parent separately bounds startup and shutdown and can kill/reap only its own
child. Forced cleanup is not reported as successful graceful termination. An
unconfirmed cleanup remains uncertain. `Runtime::stop_owned` and object cleanup
remain available without a working audit database. Neither shutdown nor an HTTP
response proves that a model completed. Failed starts and interrupted requests
have no automatic retry or fallback configuration.

## Audit evidence and external changes

`Runtime::bind` connects a typed command to the [management journal](management.md).
Expected snapshot and command digest are checked under the exclusive manager
lease, with source and execution identity checked again at application. An epoch
changes after each admitted operation and child termination, so an apparently
identical later state does not validate an earlier request.

The journal records authorization and start before effects. The adapter stores a
separate immutable completion receipt containing operation ID, request digest and
an audit-safe observed state. Normalized manifests are retained privately as
configuration artifacts, not copied into operation history. Raw configuration,
credential values and model content are absent from receipts and journal events.

A state-file edit is exposed as `external_change` without guessing its actor;
normal changes reject until the host inspects and deliberately reopens a valid
state. Tampered candidates or invalid desired activation are exposed as invalid.
Emergency owned-process stop remains possible. Files, process control and SQLite
are not one atomic transaction. An interrupted effect without a complete receipt
remains uncertain; reconciliation reads exact retained evidence without replay.
Historical application evidence does not claim that an endpoint is still alive.

There is no automatic cleanup, candidate deletion, package removal, continuation
rewrite or usage-data deletion. Back up private configuration state separately
from the management journal. Recovery uses compatible executable, configuration,
packages and retained state; a partial initialization requires inspection and a
new destination rather than overwrite.

## Validation and platform limits

```sh
cargo test -p gateway-management -p gateway-management-runtime --locked
cargo test -p agent-response-gateway --test cli --locked
```

Synthetic real-process tests cover model forwarding, active SSE shutdown, parent
loss, failed bind, pending selection, explicit restart, external changes, tampering,
admission failure and missing result records. Unix fixtures additionally test
silent/malformed children, environment isolation and forced cleanup. CI runs native
lifecycle checks on the four existing targets. Native extension execution remains
Linux/macOS only; this adapter does not extend that support to Windows. Passing
these checks is not final product composition, live-provider validation or external
consumer acceptance.
