<a id="보호된-provider-연속성"></a>

# Protected provider continuation

[English](provider-continuation.md) | [한국어](ko/provider-continuation.md)

The host implements authenticated, versioned persistence for opaque
`gateway-provider/v1` state alongside existing native replay. **Production provider
route activation remains unavailable.** Synthetic qualification can exercise this
machinery; public operation still requires integrated continuity, usage provenance
and Recorder acceptance. A plugin's managed capability declaration alone does not
activate a route or authorize a session.

Read the [provider wire contract](provider-plugins.md),
[managed execution and recovery contract](managed-continuation.md), and
[host-owned history contract](continuity.md) together. The plugin interprets vendor
state; the host owns admission, HTTP/auth, session approval, encryption, persistence
and publication. Plugins receive no storage handle, encryption key or host control token.

<a id="상태와-정확한-결합"></a>

## State and exact binding

A managed request carries only host-authorized history spans, pending-tool status
and the existing admitted request/route values. Each state has a format label,
positive unsigned 32-bit version, and canonical padded standard base64. The host
checks decoded bytes against a one-MiB limit and rejects noncanonical encodings.
The complete serialized replay record has a separate two-MiB limit, including
public output, binding metadata and base64 expansion. A state within its individual
limit can still exceed the complete record limit.

The first durable checkpoint pins format/version. Later checkpoints must preserve
that pair and the exact plugin role protocol, provider protocol, package ID/version,
package digest and executable digest. The record also binds the resolved provider,
model and route through the session origin, plus credential-owner realm/generation,
session, epoch, parent response, input length and input digest. The plugin cannot
replace any of those host-selected bindings with an identity from its reply.

Managed completed and awaiting-tools results require valid opaque state. Stateless
results require explicit state absence. An incomplete managed result is not a
resumable checkpoint. Tool outputs must match the admitted tool contract and known
pending calls. The gateway does not execute those tools or approve their actions.

A managed response may contain at most one reasoning item with an empty summary,
and that item must be the first reasoning item, which receives the host envelope.
The host rejects other empty-summary arrangements before committing a checkpoint;
JSON and SSE use the same rule. Responses without an empty-summary item retain
all public reasoning summaries and receive the envelope through the existing carrier.

<a id="replay-버전과-durable-장벽"></a>

## Replay versions and durable barriers

New provider records use `gateway-continuation/v3` and the `arg-continuation-v3.`
envelope prefix. The version prefix and key ID participate in authenticated data,
so changing a prefix cannot reinterpret ciphertext as another version. Existing
V1/V2 records retain their original bytes, envelope rules and finalized digest.
They are authenticated and compared with the authoritative stored digest before
being converted into an internal representation. No bulk row or ciphertext
migration is performed, and the continuation SQLite tables remain unchanged.

With an explicitly configured managed provider path, the manifest version is
`gateway-embedded-manifest/v10`, readiness is `gateway-ready/v10`, and an extended
manifest/readiness uses `gateway-extended-manifest/v10` / `gateway-extended-ready/v10`.
When Recorder v2 is also selected, the outer contracts become
`gateway-extended-manifest/v11` / `gateway-extended-ready/v11`; the nested gateway
manifest remains v10. The replay declaration is:

```json
{"read":[1,2,3],"write_builtin":2,"write_provider":3}
```

Builtin-only managed configurations retain their existing read/write declaration.
A host must understand the complete declared schema and exact configuration and
execution digests; an unknown version is an error, never an implicit downgrade.
The version declaration does not waive the production provider activation gate.

The existing attempt reservation and SQLite finalization barrier are reused.
The host checks current session revision, origin, epoch and parent before finalizing.
An authenticated parent must be a compatible V3 record with the same state binding.
Public text/reasoning progress can precede durable finalization; executable tool
completion, recovery envelopes and successful terminal output cannot. A storage or
publication-check failure leaves an unfinished attempt requiring reconciliation.
It does not authorize automatic inference retry or state fabrication.

<a id="복원복구롤백"></a>

## Restore, repair and rollback

Restart requires the same database, stable key/key ID, exact package and compatible
host origin/history. Restore compares the supplied session snapshot with the
current authoritative session, including revision, head, status, pending tools and
portable-history binding. Stale snapshots, wrong origins, changed packages, altered
state format/version, corrupt payloads and missing execution records fail explicitly.

A finalized record whose stored payload is missing may be repaired from a supplied
envelope only after authentication, exact origin/session checks and equality with
the original finalized digest. A present authoritative payload is authenticated
and compared instead of being silently overwritten. Pending or unknown attempts
cannot be repaired into a completed state; retain the existing explicit recovery
and epoch-transition procedure.

Package replacement does not migrate prior sessions. Restore the exact previous
package or start a new session. Preserve the original database, sidecars, matching
key, host binding and history before rollback. Disable the new route and select a
compatible binary/package or restore a compatible backup. Older binaries are not
promised to read V3; do not rewrite new records as V2 or lower their version markers.
There is no automatic state conversion, expiration or retry.

<a id="rust-소스-호환성"></a>

## Rust source compatibility

`ReplayRecord` now includes `V1`, `V2` and `V3`. Code matching the enum must handle
the new variant explicitly. The former public `ReplayRecord::normalize()` method,
which always produced `ReplayV2`, is removed. `NormalizedReplay` is an internal,
non-serializable representation reached through crate-private `into_normalized()`.
It is not an alternate public storage or plugin wire format.

Rust callers should retain the original `ReplayRecord` returned by
`Protector::open_record` or `Runtime::restore_record` and use version-specific fields
only after the required authenticity/session checks. `ReplayRecord::validate()`
checks structure and binding consistency; it does not by itself authorize a
session. Preserve original-version serialization for digest/envelope operations.
Public plugin authors continue to use wire messages and need no gateway crate.

Internally, native history distinguishes builtin replay from provider state rather
than placing provider bytes in a builtin variant. `VerifiedProviderHistory` no
longer exposes mutable history segments to external source callers. Authorized
history is assembled at the host boundary and converted explicitly for the selected
role. These source changes do not alter legacy codec V1/V2 wire meanings.
