<a id="명시적-호스트-책임"></a>
<a id="선택형-호스트-모델-정체성"></a>
<a id="외부-endpoint런타임-검증"></a>
<a id="합성-호스트-예제검증"></a>
<a id="호스트-소유-관리접근-계약"></a>

# Host-owned management and access contracts

[English](embedded-management.md) | [한국어](ko/embedded-management.md)

The optional `gateway-management-embedded` package connects a host's verified
identity, declared permissions and implementation callbacks to the existing
[management API](management-api.md). The host keeps authentication, execution
lifetime and business state. This package requires no consumer domain types and
starts no process, listener, local user store or background activity by itself.
The default Gateway does not depend on it.

## Declared host responsibilities

`HostContract` uses `gateway-embedded-management/v1`, one registered target, an
explicit operation set and either `HostOwned` or `Delegated` lifecycle. A
`HostOwned` declaration cannot contain start/stop/restart operations. Delegating
those operations requires an explicit declaration and an implementing backend;
permission alone does not create a process-control implementation.

`HostDispatcher` wraps the host's `Dispatcher`, rejects operations the backend did
not implement, and filters feature/operation reports to the declared set. It checks
the exact target, command action and wire-intent digest before preparation. Direct
reads still check actor grants. Reconciliation requires its separate declared
permission and delegates read-only evidence inspection, never automatic replay.
The host's prepared operation retains responsibility for its own synchronization,
expected state and truthful applied/not-applied/uncertain evidence.

The `service` factory takes the same contract's target, a bound numeric loopback
address, existing journal/reader and an identity verifier. It does not initialize
the stores or bind a socket. Hosts may use the library interfaces without HTTP;
when using the supplied HTTP router they retain its Host/Origin/session boundary.
One component does not acquire another process merely by sharing a PID or port.

`IdentityVerifier` must authenticate credentials through the host's trusted system
before constructing `VerifiedIdentity`. This evidence uses `gateway-host-identity/v1`
and has no Deserialize implementation. A role or subject field in an HTTP request
is not evidence. `HostAuthenticator` rejects unknown evidence versions, reduces
grants to the host contract and credential purpose, and requires refresh to preserve
the requested identity and authorization version. Unsupported operations remain
unavailable even when an upstream principal claims broader grants.

The host owns credential provisioning, identity namespaces, revocation and its
business-state decisions. Combining authentication sources requires unambiguous
subject/credential identities. The adapter neither supplies a login/password/MFA
product nor makes an unverified host principal trustworthy by wrapping it.

## Optional host model identity

The package's `team` feature is off by default. When selected,
`HostModelAuthority` connects a `ModelIdentityVerifier` to the
[team model entry point](team-model-access.md) without opening a local Team
credential database. The host supplies already verified Model-purpose identity and
versioned evidence (`gateway-host-model-identity/v1`), exact routes and usage scope.
The adapter intersects routes, clears management grants and restricts all-user
usage to the host's explicit allowance. Unknown versions, wrong purposes and
changed refresh identity/version reject.

`gateway-team-http::ModelAuthority` is the common target/authenticate/refresh
interface. The existing local Team authenticator implements it, so its credential
flow continues unchanged. The model service independently validates purpose,
enabled permissions, identity/version and matching target. Its request ledger
still records admission before Gateway transport and applies the same exact
usage/session ownership rules. Neither this interface nor the optional feature
adds a listener or storage when the host does not instantiate it.

## External endpoint and runtime verification

`ExternalAccess` uses `gateway-external-access/v1`: a registered target, HTTPS
public base ending in `/v1`, explicit Models/Responses paths and distinct external
access/Gateway credential references. It resolves only declared paths. Validate
actual protected credential values as well; different reference names do not prove
that values differ. Validation saves or returns no credential value/verifier.
There is no wildcard route proxy, WebSocket/image/audio promise, identity-provider
implementation or deployment action in this contract.

`ExpectedRuntime` consumes an **already trusted expected manifest**, checks its
configuration/execution digests and known schema, then compares the observed
readiness address, base URL, package version, manifest/readiness schema and digests.
It supports the current embedded manifest/readiness versions 1 and 3–7, and extended
versions 1–7. Unknown versions, non-loopback internal bindings, wrong digests or
inconsistent endpoints reject. This does not expand the Gateway's listener boundary.

For a managed child, `confirm_managed` additionally verifies the
`gateway-managed-process/v1` wrapper against a host-selected instance ID. The
host still owns the authenticated parent channel and bounded termination policy;
readiness metadata alone does not prove shutdown behavior. The existing
[owned runtime adapter](managed-runtime.md) supplies the standalone parent-loss
and explicit-stop implementation. An embedded host can retain its own lifetime
implementation instead of delegating control.

A self-consistent manifest, digest, contract or successful fixture is not identity
verification, artifact provenance or attestation. Obtain the expected manifest and
credential bindings through the host's trusted configuration/artifact process.
External HTTPS/access infrastructure is separate from the internal loopback service.
These fixtures prepare reusable endpoint, path, identity, readiness, termination and
credential boundaries; they do not establish an actual external deployment.

## Synthetic host example and validation

The `synthetic_host` Rust example serves actual management HTTP and a read-only
synthetic module. It uses an explicitly initialized disposable journal and a fixed
test-only read credential, exposes no lifecycle effects and exits when stdin closes.
It is not a supplied standalone product or a consumer integration.

```sh
cargo test -p gateway-management-embedded --all-features --locked
cargo clippy -p gateway-management-embedded --all-features --all-targets --locked -- -D warnings
cargo build -p gateway-management-embedded --example synthetic_host --locked
python3 -B scripts/embedded_host_smoke.py --binary target/debug/examples/synthetic_host
```

For manual inspection, create an empty private directory and pass it to
`cargo run -p gateway-management-embedded --example synthetic_host --locked -- DIRECTORY`.
Use the printed numeric loopback address with target `gateway` and the synthetic
read credential `synthetic-embedded-read-key-01234567890123456789`. The example
provides no Web assets or real Gateway process; do not use its fixed key for a
product configuration. Reuse real authenticated read APIs when attaching a host UI.

Tests cover host operation/lifetime refusal, reduced grants, identity refresh,
unknown contracts, body identity injection, exact runtime/endpoint matching and
distinct credential boundaries. With the optional `team` feature, a real model
router fixture uses host identity without a local credential database. The example
smoke test checks actual scoped reads, denied lifecycle change and bounded stdin
shutdown. These checks are separate from provider conformance, standalone assembly,
external deployment and consumer operational acceptance.

Existing configuration, activation, continuation, usage and credential formats are
not migrated. Restore compatible verified host code and coherent state for rollback.
An unsupported operation/version remains an error rather than silent degradation.
