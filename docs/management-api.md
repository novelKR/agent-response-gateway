<a id="cli-사용과-검증"></a>
<a id="http-인터페이스"></a>
<a id="관리-http-및-cli-계약"></a>
<a id="브라우저-조회-세션"></a>
<a id="사전-검증시작-기록작업-증거"></a>
<a id="인증과-호스트-경계"></a>

# Management HTTP and CLI contracts

[English](management-api.md) | [한국어](ko/management-api.md)

The optional `gateway-management-api` package provides a loopback HTTP router,
local authentication adapter and `gateway-management-cli` client. A trusted host
supplies its initialized journal, reader and dispatcher. This package starts no
server automatically and does not assemble runtime, package, usage or team
services by itself. The default gateway does not depend on it.

## Authentication and host boundary

`Service::new` requires the actual bound numeric loopback address and one registered
target. HTTP Host must match its canonical address and port. Any Origin must match
the same HTTP origin. Browser session creation and logout require that Origin
explicitly. There are no wildcard CORS grants, redirects or environment proxies.
Hosts embedding this router retain responsibility for binding and authentication;
claiming a loopback address does not secure an independently exposed listener.

An `Authenticator` returns the current immutable actor, credential kind and
authorization version. Neither role nor subject is taken from request bodies or
identity headers. Mutation authority is refreshed after obtaining the writer,
including for queued requests. Each operation still needs its exact target/action
grant and host support. Unsupported host operations are refused before admission.

`LocalAuthenticator` accepts explicitly provisioned high-entropy credentials and
stores their hashes in memory. It provides management and read-only kinds; a
read-only credential cannot contain mutation/reconciliation grants. This adapter
is not a password, enrollment, email or MFA system. Credential provisioning and
host authentication remain outside this transport package.

## HTTP surface

The envelope schema is `gateway-management-http/v1`, independent of Responses and
manifest/readiness v7. Successful responses carry a schema and observation timestamp. Errors
use fixed codes without reflecting headers, credential values or submitted data.
The dispatcher supplies authorized module views and declares available features;
its missing/unobserved values are not converted to zero by the transport.
State uses `gateway-management-state/v1` with up to 16 named module views. Each
view declares its own contract and an `observed`, `unobserved` or `unsupported`
observation. Observed data has its own timestamp and a matching schema. A custom
host can provide generic module metadata without introducing its business types
into the management package.

| Path under `/management/v1` | Method | Required authority |
|---|---|---|
| `/capabilities` | GET | Read state; reports host-supported and actor-allowed actions |
| `/state` | GET | Read state and host support |
| `/usage` | GET | Read usage and host support |
| `/continuations/{id}` | GET | Read state and host-provided continuation query |
| `/operations` | GET | Read operations for the target |
| `/operations/{id}` | GET | Read operations for the target |
| `/preflight` | POST | Management credential and the requested action grant |
| `/operations` | POST | Management credential and the requested action grant |
| `/operations/{id}/reconcile` | POST | Management credential, reconciliation grant and host support |
| `/session` | POST, DELETE | Optional read-session boundary described below |

Read requests include the registered target. Operation listing uses a local row
cursor, a default page size of 20 and a maximum of 100. Usage takes `from_ms`,
`to_ms` and `timezone`; a request covers at most 366 days. Module adapters retain
their own query validation and observation semantics. Request bodies are bounded
to 64 KiB and responses to 2 MiB. At most 64 blocking requests/jobs are admitted;
capacity exhaustion returns an explicit busy response without applying a change.

Wire commands have a closed set of registered-ID fields for runtime start/stop/
restart, configuration staging/selection, package install/enable/disable/selection
and host continuation transitions. They cannot name a server executable, shell
or arbitrary server path. Package family and exact selection are explicit.
Unknown fields and unsupported removal commands reject. A host must independently
verify continuation workflow conditions; client-supplied pending flags do not
transfer tool approval or business-state ownership to the gateway.

## Preflight, admission and operation evidence

Preflight accepts schema, target, idempotency key and a typed command. It checks
current authority and host support, observes a current snapshot, and validates
the command against that snapshot. It returns a complete proposed submission
without creating an operation or applying a change. It is not future authorization.

Submission includes that expected snapshot. The dispatcher must bind the exact
canonical wire intent to its prepared operation and retained evidence. It must not
silently substitute an unrelated parameter digest when adapting a module command.
The journal rechecks the prepared state, commits authority and intent, then commits
start before application. A returned operation ID means durable admission or a
previously recorded identical request; it does not mean successful application.
The request is never automatically retried by this HTTP router or CLI.

Operation queries return an `operation` containing the unchanged durable record,
an `observed_state`, and an optional `uncertainty` reason. If a worker exits after
admission without a result record, the current process reports uncertainty instead
of claiming that the worker is still running. That observation contains IDs only
and does not rewrite audit history. After restart, the journal's existing recovery
rules mark incomplete records. Reconciliation is separately authorized and reads
adapter evidence; it never repeats the original command or infers model completion.

Read-only operation queries use a separate database reader and remain available
while an effect holds the writer. Native package and configuration adapters must
retain their own synchronization and source checks. A callback fixture passing
these HTTP checks does not establish that a concrete adapter has been assembled.

## Browser read sessions

Read sessions are explicitly enabled by the host; otherwise their route is absent.
Only a dedicated read-only credential can create one. A management credential is
refused for this flow. The browser receives an opaque HttpOnly, SameSite=Strict
cookie scoped to the management path, with an origin-specific cookie name and a
15-minute lifetime. At most 256 live sessions are retained; there is no background
session worker. Cookie values are stored as hashes, never returned by query APIs.

The server refreshes credential identity and authorization version on each session
use. Revocation, a version change, expiry or an identity mismatch rejects the
session. Cookies never authorize preflight, mutations or reconciliation. Logout
removes only that read session. Supplied dashboards must use real read APIs and
must not receive, persist or send management mutation credentials.

## CLI use and verification

The CLI accepts an explicitly selected numeric loopback HTTP endpoint. Supply
`GATEWAY_MANAGEMENT_TOKEN` from a protected host credential source; the token itself
is not a command-line argument. Redirects, inherited proxies and automatic retries
are disabled. Output is bounded versioned response metadata, not request headers
or credential values. A client HTTP response is not evidence of model completion.

```sh
gateway-management-cli --endpoint http://127.0.0.1:47100 capabilities --target gateway
gateway-management-cli --endpoint http://127.0.0.1:47100 state --target gateway
gateway-management-cli --endpoint http://127.0.0.1:47100 preflight --file preflight.json
gateway-management-cli --endpoint http://127.0.0.1:47100 submit --file submission.json
cargo test -p gateway-management-api --locked
```

The endpoint, target and input filenames above are examples, not automatically
created service state. Review the preflight response's submission before sending
it. Synthetic tests cover authorization, Host/Origin boundaries, read sessions,
queued permission changes, stale state, admission/result-record failures,
idempotency and a real CLI/loopback HTTP exchange. Deployment archives, provided
Web assets, team access and actual adapter composition have separate validation.
