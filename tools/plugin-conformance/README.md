# Independent plugin conformance tool

`conformance.py` is a standalone, standard-library-only Python 3.11+ command for
Linux and macOS. Copy this directory without the gateway checkout. The runner
consumes package bytes and public protocol messages; it imports no gateway code.
Its executable interface is independent of the implementation language.

Static inspection checks `gateway-extension-package/v1` and explicit v2 manifests,
canonical bytes, exact trusted digest, role permissions/state schema, target
recognition, flat file inventory, regular files, links, size limits and hashes.
It accepts another supported target for static inspection, but refuses to execute
it on an incompatible host. It neither installs nor activates packages.

```sh
python3 -B conformance.py --package /absolute/package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256
python3 -B conformance.py --package /absolute/package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch
```

Obtain the expected digest through a trusted channel. A digest computed from
untrusted input does not authenticate its publisher. Installation must separately
reverify the package and enforce its private-store ownership and permissions.
The package directory must remain unchanged during inspection.

Execution requires explicit `--execute`. Tool version `2.0.0` provides three
profiles; none installs or selects a package in a gateway:

| Profile | Executable checks | Coverage limits |
|---|---|---|
| `wire` (default) | Observer v1 Ready and ACKs; provider v1 exact Ready; Recorder v2 Ready and event checks with an explicit state fixture | Generic provider semantics remain `not-run`; codec and Recorder v1 executable runners are unavailable |
| `synthetic-provider/v1` | Exact Ready, JSON, incremental SSE, function calls/results, numeric unknown/zero/invalid values and opaque state messages | Requires the explicitly selected synthetic example contract; state wire round trips are not host persistence or recovery |
| `recorder-events/v2` | Exact Ready, canonical V1/V2/invalid-observation ACKs, duplicate ACKs and restart | Requires a disposable initialized fixture; restart proves stable producer and repeat ACK, not payload retrieval or power-loss durability |

Static checks support Observer v1, Recorder v1/v2, codec v1/v2/v3 and provider
v1. Package v2 validates sorted API/features, exact required host contracts and
role-specific provider identity; legacy package v1 cannot be reinterpreted as v2.
An unsupported requested role or missing fixture produces `not-run`, never pass.

```sh
python3 -B conformance.py --package /absolute/provider-package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch --profile synthetic-provider/v1
python3 -B conformance.py --package /absolute/recorder-package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch --profile recorder-events/v2 \
  --recorder-state-fixture /absolute/private-disposable-fixture
```

Initialize the Recorder fixture according to its own documented contract. For
this repository's independent Recorder, run its built executable with `init` in
an empty private directory. The runner does not guess initialization commands or
open operational storage. It copies a flat fixture of at most 64 regular files
and 32 MiB into disposable state, rejects links and overlapping package/scratch
locations, and never writes back to the supplied fixture. Its digest covers the
copied bytes and event vectors. The provider suite digest binds its suite version
and exact tool bytes. Python script examples require an explicitly provisioned
interpreter at the build-time absolute path; nothing downloads a runtime.

Provider frames are bounded to 1 MiB within the host's larger contract ceiling;
startup and complete exchanges have three-second deadlines. Recorder line frames
and exchanges are bounded too. Synthetic assertions are specific to the selected
fixture contract, not a universal test of an arbitrary supplier's semantics.

The Observer runner copies all verified payload files into a temporary package directory,
uses a separate temporary working directory, clears the environment and connects
stdin/stdout to the same Unix stream socket, like the host. It validates readiness
within three seconds, then sequential acknowledgements for three synthetic numeric
HTTP observations within a shared one-second write/read deadline per event.
UTF-8 JSON replies reject duplicate fields and nonfinite constants; frames are
limited to 4096 bytes including newline. Unexpected output, wrong acknowledgements,
premature exit and stalled/partial frames fail. The direct child is killed and
reaped on success or failure. Observer v1 does not promise graceful EOF handling.

This is trusted native execution, **not a sandbox**. The process retains the user's
OS authority. Do not provide real credentials, operational data or Docker sockets.
Use an isolated verification environment for code that has not been reviewed.
The harness does not certify publisher identity, descendant-process containment,
private-store installation, observer persistence, actual gateway integration,
arbitrary provider behavior or production suitability. Windows native plugin
execution is unsupported; static target recognition is separate evidence.

Each invocation emits `gateway-plugin-conformance-report/v2`, described by the
[report schema](../../schemas/gateway-plugin-conformance-report-v2.schema.json)
and [vectors](../../schemas/plugin-conformance-report-vectors.json). It records
package/tool/fixture digests, tool version, package/role contracts, target, host
and explicit profile, with per-check `pass`, `fail` or `not-run` and `required`.
The `host.integration` check is always non-required and `not-run`: standalone
protocol success never establishes installed gateway execution. Every required
check must pass for overall pass. A failed attempted check remains failed when
later checks cannot run. Cleanup pass is recorded only after process and temporary
state cleanup finish. Report v1 consumers must explicitly adopt this v2 shape.

Diagnostics are fixed codes; paths, package stdout/stderr, credentials and fixture
bodies are excluded. Static-only invocation reports incomplete runtime coverage
and exits 0 when requested static checks pass. Exit 1 denotes failure; exit 2
denotes requested but incomplete execution. These are local profile-scoped test
reports, not signatures or externally published attestations.

## Independent Observer example

`examples/observer` is a portable project using Python's standard library only.
It accepts numeric metadata and acknowledges its sequence; it does not persist
state. Its builder generates a directly executable script with an absolute Python
interpreter path in the shebang. That Python 3.11+ runtime must already exist at
the same path on the execution host; the builder and runner never install one.
This is an explicit script-runtime example, not a self-contained native binary.

```sh
python3 -B examples/observer/build.py --output /absolute/build/observer
```

The output location must not already exist. Package this executable using the
separately distributed extension manager and the applicable license evidence,
then run the commands above. The example can be implemented in another language
or compiled to a self-contained executable without changing its wire protocol.
For redistribution, preserve the project license and supply corresponding source
and any runtime/dependency notices required for the exact delivered artifacts.

Repository validation exercises the copied project outside the checkout, malformed
frames, deadlines, early exits, empty environment, digest/inventory errors and
cross-target behavior:

```sh
python3.14 -B -m unittest discover -s scripts/tests -p 'test_plugin_conformance*.py' -v
```
