<a id="검증"></a>
<a id="관리-작업의-영속-감사"></a>
<a id="복구와-저장"></a>
<a id="소유권과-접근"></a>
<a id="작업-증거"></a>
<a id="audited-management"></a>

# Audited management operations

[English](management.md) | [한국어](ko/management.md)

The optional Rust library `gateway-management` provides typed management
contracts and durable operation evidence. The default gateway does not depend
on it. This foundation starts no listener or process and discovers no credentials.
A standalone management server, dashboard, package adapter and team service are
separate integrations; this library does not make them available automatically.

<a id="management-ownership"></a>

## Ownership and access

A trusted host constructs an immutable `Actor` from its current authentication
and permission decision. An actor cannot be deserialized from an HTTP body.
Grants bind an exact `Action` and target. Capability reporting intersects host
support with those grants. Hosts must obtain a fresh actor at execution time,
including after queueing or credential changes; possession of an old Rust value
does not perform authentication or check external revocation.

`Request` contains an idempotency key, target, action, expected `Snapshot` and
`parameters_sha256`. The adapter must derive the parameter digest from the exact
validated private command. This audit-safe request is not a public interface for
arbitrary paths or programs. Payloads and secrets never belong in identifiers.

`Backend::prepare` must have no intended target effects. Its
`PreparedOperation` holds the target lock or equivalent host lease while the
journal checks the snapshot, commits authorization and intent, and starts the
effect. Conditions not protected by that lease must be rechecked at application.
The library does not add filesystem, process, package or credential authority.
Hosts retain tool approval and workflow responsibilities.

<a id="operation-evidence"></a>

## Operation evidence

The contract is `gateway-management/v1`. Each operation retains its initiating
subject and credential IDs, granted action, request fingerprint and ordered
events. The journal stores no command body, configuration text or credential value.

| State | Meaning |
|---|---|
| queued | Authorization and intent committed; application has not started |
| running | Start committed; the effect may have happened |
| succeeded | Adapter reported an applied effect with post-state and evidence digest |
| failed | Adapter or recovery can establish that the effect did not happen |
| uncertain | Available evidence cannot establish the actual effect |

Authorization precedes duplicate lookup. The same subject and idempotency key
with identical intent returns the recorded operation without another effect.
Conflicting reuse rejects. A rotated credential can retrieve the same intent
only through a newly authorized actor. Different subjects have separate keys.

Adapters return `Applied`, `NotApplied` or `Uncertain`. An applied result needs
the observed post-state and evidence digest; a digest alone does not prove the
adapter observed reality. Returning `NotApplied` requires evidence that no intended
effect occurred. Partial effects must remain uncertain.

The journal commits intent and start before calling the effect. Failure of either
write prevents application. Failure to record a result returns an uncertainty
error containing the operation ID. Files, processes and SQLite are not one atomic
transaction. The original operation is never automatically replayed.

<a id="recovery-and-storage"></a>

## Recovery and storage

`Journal::initialize` requires an existing private directory and refuses to
replace a database. `Journal::open` requires
`gateway-management-store/v1`; it never repairs or migrates an unsupported store.
The store uses SQLite WAL/FULL, an exclusive writer lock and a configured page
limit. Read-only `Reader` access requires an explicit target read grant.
Queries use bounded local cursors; timestamps do not establish event order.
A query page contains at most 100 operations, and each operation has at most
256 events. Reaching the evidence bound rejects further appends rather than
overwriting history. The page limit bounds database pages, not all WAL or
filesystem resource consumption.

On reopen, or when an idle writer handles an authorized retry or reconciliation,
queued operations become failed because application never started.
Running operations become uncertain. Automatic recovery events have no actor;
the original initiating identity remains on the operation. A separately
authorized reconciler uses a read-only backend inspection and appends evidence.
Reconciliation does not execute the original command or rewrite prior events.

Database triggers prevent ordinary updates/deletes of operation and event rows.
These are API integrity checks, not protection against an administrator or hostile
same-user code rewriting the database. Protect host permissions and backups.
Unix stores require private owner-matched regular files without extra hard links;
links and Windows reparse paths reject. Windows directory access remains a host
ACL responsibility rather than a POSIX mode guarantee.

Backup creates a new file and includes committed WAL data. Initialization and
backup are trusted host maintenance methods, not unauthenticated endpoints.
Failed initialization or backup may leave an incomplete artifact; inspect it and
choose a new destination, never silently overwrite or erase it. No automatic
retention, package removal, data deletion or inference retry is provided.

<a id="management-validation"></a>

## Validation

Run the focused library checks and the repository validation gates:

```sh
cargo test -p gateway-management --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Synthetic tests cover durable admission failure, interrupted start/result records,
conflicting retries, permission rejection, scoped readers, exclusive ownership,
reconciliation, backup and invalid storage. They do not prove external adapter
correctness, browser security, live provider behavior or consumer acceptance.
