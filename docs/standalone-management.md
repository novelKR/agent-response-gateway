<a id="team-credential과-모델-접근"></a>
<a id="web과-사용량-조회"></a>
<a id="검증배포물복구"></a>
<a id="구성-요소-선택"></a>
<a id="명시적-초기화와-운영"></a>
<a id="선택형-standalone-운영"></a>
<a id="소유할-대상-하나-등록"></a>

# Optional standalone operations

[English](standalone-management.md) | [한국어](ko/standalone-management.md)

The `gateway-management-app` package composes the actual configuration, owned
runtime, extension, audit and usage adapters behind the [management API](management-api.md).
It supplies `gateway-manager`, an optional static dashboard and an independently
selected Team build. The default `agent-response-gateway` binary and its dependency
path do not include management, Web or Team. Existing CLI and embedded use remain
available without these components.

## Select components

| Component | Runtime behavior |
| --- | --- |
| Gateway archive | Existing model transport and declared routes |
| Management archive | `gateway-manager`, `gateway-managed-child`, management CLI, local package driver and notices |
| Web archive | Verified static Vue application served by the manager; Node is only a build dependency |
| Team archive | `gateway-team-manager`; Team stores, authentication and listener open only when the registration selects Team |
| Embedded library | Host authentication and explicit delegation; the host retains business state and execution lifetime |

Use archives from the same source commit. Web and Team module manifests declare an
exact management source prerequisite. Selecting a different package version is not
a database downgrade, ciphertext rewrite or replay of model requests. None of the
programs automatically installs or activates a downloaded extension.

The ordinary management build has no Team dependency. The Team build can also run
with `team: null`; it creates no Team database, listener or worker in that mode.
Native extension management and the built-in SQLite Recorder reader support Linux
and macOS. Windows retains profile-pack management, runtime/configuration control,
Team model transport and request evidence; native extension/Recorder capabilities
remain unsupported. Native package management explicitly requires a registered
Python 3.11+ interpreter. The interpreter itself is not bundled.

## Register one owned target

A private JSON registration file uses `gateway-management-registration/v1`.
Registration is trusted local setup, separate from HTTP command bodies. Supply
absolute paths without symbolic links, an exact managed-child executable digest,
source IDs, environment-reference bindings and local credential identities.
Configuration candidates preserve exact source bytes and are checked against the
current activation files before a launch. The manager controls only a child it
started through the managed-parent protocol; it does not adopt a PID or occupied
port.

The following is a registration shape. Replace angle-bracket placeholders with
actual absolute paths and the SHA-256 of the selected managed-child binary. The
Gateway configuration must reference the explicitly supplied environment names.
This example selects management only and exposes no model route itself.

```json
{
  "schema": "gateway-management-registration/v1",
  "target": "gateway",
  "listen": "127.0.0.1:48080",
  "journal": "<absolute-private-journal-directory>",
  "runtime": {
    "directory": "<absolute-private-runtime-directory>",
    "executable": "<absolute-path-to-gateway-managed-child>",
    "executable_sha256": "<64-lowercase-hex-sha256>",
    "credential_generation": "local-generation-1",
    "sources": {
      "local-config": {
        "configuration": "<absolute-path-to-gateway-toml>",
        "extensions_lock": null,
        "profile_packs_lock": null
      }
    },
    "environment": {
      "ARG_LOCAL_TOKEN": "GATEWAY_MODEL_TOKEN",
      "PROVIDER_KEY": "SELECTED_PROVIDER_KEY"
    }
  },
  "credentials": [
    {
      "subject": "local:operator",
      "credential": "local:management-key",
      "token_env": "GATEWAY_MANAGEMENT_TOKEN",
      "read_only": false,
      "grants": [
        {"target": "gateway", "action": "read_state"},
        {"target": "gateway", "action": "read_operations"},
        {"target": "gateway", "action": "configuration_stage"},
        {"target": "gateway", "action": "configuration_select"},
        {"target": "gateway", "action": "runtime_start"},
        {"target": "gateway", "action": "runtime_stop"}
      ]
    }
  ],
  "native": null,
  "profile_packs": null,
  "usage": null,
  "continuation": null,
  "web": null,
  "team": null
}
```

Use independent high-entropy local credentials of at least 32 printable ASCII
characters. Values are read from named environment sources; the registration and
HTTP API contain references, not credential values. Child environment construction
includes only the Gateway configuration's references, plus explicitly registered
`SYSTEMROOT` on Windows. Management/read credential values and `gwt1_` Team keys
are rejected as Gateway/provider environment bindings. Provider credentials and
Gateway model credentials retain the Core's existing separation rules.

Local subject and credential IDs must start with `local:`. The standalone Team
adapter reserves that namespace and refuses an incompatible imported Team store.
Identity source cannot be inferred safely from an unchecked string prefix; the
manager validates local bindings and Team state before combining authenticators.
Embedded hosts retain responsibility for their own identity namespaces.

## Initialize and operate explicitly

Create private parent directories first. Windows directory ACL protection remains the host/operator responsibility; POSIX modes are not a Windows ACL guarantee. Initialize each selected new store
separately; existing stores are opened without automatic schema migration or
recovery by deletion. A repeated initialization fails and does not overwrite data.

```sh
gateway-manager --registration registration.json init --component management
gateway-manager --registration registration.json init --component runtime
gateway-manager --registration registration.json serve
```

Additional initialization components are `native`, `profile-pack`, `continuation`,
`team` and `team-requests`; they require corresponding registration entries.
Package stores and Recorder/continuation data stores are initialized through their
existing tools. Management initialization does not migrate or initialize those
existing data formats. `serve` opens initialized adapters but does not start a
Gateway until an audited start operation is submitted.

The manager prints `gateway-management-listeners/v1` metadata with the actual
numeric loopback endpoints. Port zero can request a fresh local port. No forwarding
headers, public listener or remote identity product are enabled implicitly.
A supervisor can run `serve --parent-stdin`; closing stdin stops the manager and
its owned Gateway. Interactive operation handles Ctrl-C. Shutdown first closes
management mutation admission, lets an executing effect finish under the owner
lock, and performs owned-child cleanup independently of audit storage health.
Queued changes cannot restart a Gateway after cleanup begins. Model requests are
never automatically resubmitted.

Use the CLI's preflight output `data.submission` as the exact input to `submit`.
Preflight neither grants permission nor applies a change. Persist or inspect the
returned operation ID and poll its state; a `202` acknowledgement proves admission,
not successful execution. Stage, select and start are separate commands:

```json
{"kind":"configuration_stage","source":"local-config","candidate":"candidate-1","source_sha256":"<exact-config-file-sha256>"}
{"kind":"configuration_select","candidate":"candidate-1"}
{"kind":"runtime_start"}
```

Wrap each command in the [versioned preflight envelope](management-api.md) with the
registered target and a distinct idempotency key. The resulting submission includes
the expected state revision/digest. Reuse the original submission for an uncertain
retry; changing its expected state or intent under the same key is a conflict.
Do not turn a timeout into an automatic restart or a new model request.

For local packages, registration supplies `directory`, `store`, registered `sources`
(`path` plus exact `package_sha256`), `recorder_bindings`, and `driver`. Native drivers
pin the absolute Python and existing manager script paths and both file hashes;
profile packs use `driver: null` and no Recorder bindings. Source/grant validation
reuses the existing managers inside their mutation boundary. Install, enable,
disable and exact version selection remain separate from runtime start/restart.
Profile version replacement still requires an explicit disable first. Uninstall,
data deletion, remote search/download and automatic updates are unsupported.

Installed, selected for next start and effective in the captured running manifest
remain separate observations. An effective codec entry describes execution
configuration; it is not evidence of a resident codec process. Disabled packages,
usage records and continuity data are preserved.

## Web and usage views

Select `web` with an absolute verified static directory and its exact
`web-manifest.json` SHA-256. The manager validates clean source provenance, compatible
API/state contracts and each allowlisted asset digest, then serves those immutable
bytes from its numeric loopback origin. Unknown files never become asset routes.
There is no Vite development server or Node runtime in the product.

Register a separate `read_only: true` credential with the required read grants.
The browser uses it only to establish a bounded HttpOnly/SameSite read session.
Management credentials cannot create that session. Host and Origin checks apply;
static content uses CSP, no-store, nosniff and a no-referrer policy. The dashboard
contains no setting, extension or credential mutation controls. Permission loss or
credential rotation invalidates a corresponding Team read session.

Select `usage` with the absolute directory of an existing Recorder store. Local
readers with `read_usage` see the existing aggregate grouped by attempt-start time.
Team management/read credentials use their own exactly correlated request history;
all-team access requires `read_all_usage`. Team ranges use admission time, preserve
raw canonical attempts, and paginate with `after`/`next_after`. The dashboard shows
the appropriate scope and clock. Missing counters stay null, transport EOF remains
separate from model completion, and missing producer/request evidence stays
unattributed. No cost, quota or billing result is inferred.

## Team credentials and model access

Use `gateway-team-manager` and select `team` with private `directory` and `requests`
directories plus its distinct numeric loopback `listen`. Explicitly initialize
`team` and `team-requests`. Add only the required Team administration grants to the
operator; no role field supplied by a client grants authority. Subject registration,
permission changes and credential revocation use the same audited dispatcher.
Team grants must name this registered target.

`team_credential_issue` and `team_credential_rotate` require the protected synchronous
`POST /management/v1/credential-delivery` endpoint. Generic asynchronous submission
rejects them. Browser Origin/cookie delivery is refused. Use the CLI to receive a
new key into a newly created private file, never standard output:

```sh
gateway-management-cli --endpoint http://127.0.0.1:48080 \
  --token-env GATEWAY_MANAGEMENT_TOKEN deliver-credential \
  --file issued-submission.json --output new-private-credential-file
```

The file's existing parent must be private and without links; the destination must
not exist. No output path is sent to the server. Raw credentials are only returned
after the result is durably successful. The transient output is consumed even when
the HTTP caller disconnects or result recording fails. Query, reconciliation and
idempotent retry cannot redisplay it. A missing delivery may require an explicitly
chosen replacement credential; it does not cause automatic reissuance. Failure can
leave an empty local output file; inspect operation evidence before another action.

The Team model endpoint supports the current HTTP Responses/model-list contracts.
Model keys cannot call management; management/read keys cannot call models.
Allowed routes filter both calls and model listings. The manager captures the
actual owned execution and uses its distinct Gateway model/control credentials.
The active Recorder store must match the captured extension binding before a
producer is attributed. No user is guessed from a time or model name.

Managed routes retain subject/route ownership of public Team session IDs and strip
access to internal Gateway headers/control tokens. The optional management
`continuation` adapter provides authorized host metadata reads and explicit
`compact_begin`, `compact_commit` or `recover` transitions against a known internal
session ID. It requires a separate private evidence directory, current revision,
exact intent and the Core's pending-state checks. It never executes a tool, invents
portable history, resends inference or repairs an uncertain transition by replay.

## Verification, artifacts and recovery

Build the independent Web with pinned Node 24.21.0/npm 11.19.0. Exported-source
builds use a verified source-file receipt; normal local builds retain Git's dirty
indicator. A self-consistent receipt/hash is not attestation or an external identity
assertion. The optional packaging recipe consumes the exact source archive from a
verified base candidate, preserves that source and original notices, builds the
management and Team feature sets separately, and runs extracted artifacts against
synthetic loopback providers.

```sh
python3 -B scripts/optional_package.py build --base .local/base-candidate --output .local/optional-candidate
python3 -B scripts/optional_package.py verify .local/optional-candidate --commit SOURCE_COMMIT --target NATIVE_TARGET
```

The existing base [release recipe](release.md) remains separate. Optional module
archives record exact files, source/lock hashes, supported contracts and dependency
requirements. Build/fixture verification is separate from hosted CI, provider
conformance, consumer operations, attestation and a formal release. These recipes
neither deploy a service nor publish a tag or Release.

Stop the manager before explicit coherent SQLite backups:

```sh
gateway-manager --registration registration.json backup --component management --destination new-management-backup.sqlite3
gateway-team-manager --registration registration.json backup --component team --destination new-team-backup.sqlite3
gateway-team-manager --registration registration.json backup --component team-requests --destination new-request-backup.sqlite3
```

Retain the associated validated configuration candidates, runtime/extension/control
evidence directories and compatible package bytes. Do not copy live SQLite/WAL files
individually, overwrite evidence, delete data to clear an error or downgrade a
schema implicitly. On reopening, unfinished audited effects remain uncertain until
explicit evidence reconciliation. Restore compatible verified executables and
coherent state; completed model calls are never a rollback mechanism.
