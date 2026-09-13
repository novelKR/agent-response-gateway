<a id="감사-변경과-일회성-전달"></a>
<a id="검증과-연동-제한"></a>
<a id="선택형-주체-credential과-팀-권한"></a>
<a id="저장소-소유권과-복구"></a>
<a id="정체성과-용도"></a>

# Optional named credentials and team authority

[English](team-access.md) | [한국어](ko/team-access.md)

The optional `gateway-team-access` workspace package provides registered subjects,
high-entropy credentials and exact route/action permissions. It depends on the
optional management contracts and authentication interface; the default Gateway
does not depend on it. Its library opens no listener, authentication session or
background worker. A host explicitly initializes its separate private store and
connects the adapter when Team Access is enabled. This package alone does not
supply model forwarding, usage attribution, credential-delivery HTTP endpoints
or a standalone service.

## Identity and purpose

A subject has an enabled flag, a revision, exact allowed route aliases, explicit
management target/action grants, and a separate permission for all-subject usage.
There are no role shortcuts, route wildcards or tenant provider keys. The model
admission adapter must check the exact route and use the same scope for model
listing. The usage adapter must enforce subject scope; `read_all_usage` does not
calculate quotas, prices or charges. These fields are contracts for their consuming
adapters, not proof that forwarding or accounting has been assembled.

| Credential purpose | Model admission | Management authentication |
|---|---|---|
| Model | Explicit `authenticate_model`; route permission still required | Refused |
| Management | Refused | Only the subject's explicit management grants |
| ReadOnly | Refused | Read grants only; eligible for host-enabled read sessions |

`Authenticator` implements the existing management authentication/refresh trait,
and separately offers model authentication/refresh. It accepts a credential value,
not a client-supplied role or subject. A principal contains server-verified identity,
purpose, permissions and an authorization-version digest. Changes to the subject's
permissions invalidate its previous versions; rotating/revoking a credential
invalidates that credential. Another subject's independent credential changes do
not invalidate an unchanged subject. A host still refreshes authority immediately
before its operation or model admission; authentication is not permanent permission.

Read-session cookies remain owned by the management transport. Their existing
refresh check observes Team Access permission changes and revocation. Model keys
cannot create those sessions, and read-only keys never authorize management effects.
No password login, signup, email, MFA or external identity system is provided.

## Audited changes and one-time delivery

`Manager::execute` takes a trusted actor, a management request and a closed domain
command. Commands register subjects, change permissions, issue/revoke credentials
or rotate to a new registered credential ID. The request binds the exact domain
command digest, target, expected snapshot and idempotency key. A SQLite IMMEDIATE
transaction holds current generation/digest stable across journal admission and
start. Actor authorization and durable start precede every intended team change.

The credential token is generated inside the admitted effect with 256 random bits
and a purpose prefix. Only its SHA-256 verifier is stored. Subject/credential
metadata, permissions and operation-linked receipts persist separately from the
management journal. Receipts bind the request, before/after snapshots and exact
issued credential metadata/verifier. The immutable journal records digests and
outcomes, never the key value. Tokens are not password hashes or ciphertext, and
there is no decryption or old-token retrieval path.

A `Secret` is returned only after the journal confirms success. It has no Debug or
Serialize implementation, is zeroized when dropped, and exposes bytes only through
an explicit delivery method. A trusted host must use a protected one-time output
path. Repeated idempotent requests return the recorded operation with no secret.
A connection loss or lost response does not permit replaying the token; inspect
the credential ID and explicitly revoke or rotate it.

Authentication requires successful issuance/rotation evidence from the matching
management journal and the exact receipt binding. An insertion whose result
record is missing is not usable merely because a database row exists. Explicit
reconciliation may confirm retained effect evidence without issuing or delivering
another secret. A mismatched or absent receipt remains unverified. Positive immutable
issuance evidence is cached within the authentication adapter, so established
credentials do not require new management writes or repeated audit reads for each
model admission. Fresh credential/subject status is still checked on every use.

Revocation and permission reduction take effect once their team transaction commits,
even if the subsequent audit result cannot be written. That failure never restores
old access. Rotation commits old-key revocation and new-key insertion together. An
intermediate transaction failure rolls both back; an unconfirmed commit remains
uncertain. No operation is automatically replayed after interruption.

## Store ownership and recovery

Initialize an existing private directory explicitly with `Manager::initialize`;
`Manager::open` requires the supported schema and exact target. SQLite reuses the
repository's pinned Rust/SQLite versions with WAL/FULL, an exclusive writer lease,
bounded records and an explicit size limit. There is no automatic schema repair,
cleanup, continuation/usage migration or credential-environment discovery.

The initial bound is 128 subjects, 1,024 retained credential records, 128 route
aliases and 256 management grants per subject, with at most 48 KiB of serialized permissions. Revoked records still count and
are retained for evidence. Capacity conflicts require explicit operational review;
this package has no removal endpoint or automatic retention policy. Identifiers,
receipt hashes and permissions are queryable metadata; key verifiers and values
are excluded from the public inventory. Hosts must separately authorize inventory
access and protect the directory/ACL and delivery paths.

`Manager::backup` creates a coherent SQLite backup in a selected private location
without overwriting a file. Retain the matching management journal as well. Restore
a compatible verified program and coherent stores; do not restore an older team
store to resurrect revoked credentials. Loss of evidence requires explicit recovery
and reprovisioning, not guessing the prior key. No ciphertext rewriting, data
reverse-migration or completed model request replay is involved.

## Validation and integration limits

```sh
cargo test -p gateway-team-access --locked
cargo clippy -p gateway-team-access --all-targets --locked -- -D warnings
```

Synthetic tests cover purpose/route/subject separation, one-time output, absence of
plaintext in SQLite/WAL/journal metadata, actor denial, stale snapshots, admission
and result-record failures, explicit reconciliation, atomic rotation rollback,
revocation, credential-proof tampering, coherent backup and wrong schemas/targets.
A real management router fixture verifies read-session invalidation after rotation
and permission change. This is separate from model transport, concrete standalone
assembly, external deployment and consumer acceptance. Turning this module off
keeps the normal CLI/configuration flow and creates no team resources.
