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

Execution requires explicit `--execute`. Only the Observer v1 executable runner
is currently supplied. Static checks support Observer v1, Recorder v1 and codec
v1/v2/v3 and provider v1 declarations. Package v2 validates sorted API/features,
exact required host contracts and role-specific provider identity; legacy package
v1 cannot be reinterpreted as v2. Provider execution reports the explicit
`provider_runtime_unavailable` code. Other role execution reports `not-run`, never a successful conformance
claim. Additional runners register by exact protocol in `RUNNERS`; changing the
report or wire contract requires an explicitly versioned change.

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
provider behavior or production suitability.

Each invocation emits one JSON report with stable identifiers, the selected
package digest, tool version, package/role contracts, package target, host target
and individual `pass`, `fail` or `not-run` checks. Diagnostics are fixed codes;
paths, package stdout/stderr and fixture bodies are not included. Overall
`not-run` means runtime coverage is incomplete, including static-only invocation.
Exit codes are 0 for successful requested static checks or successful execution,
1 for failed checks, and 2 for requested but unavailable role execution. Reports
are local test evidence, not signatures or externally published attestations.

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
python3.14 -B -m unittest discover -s scripts/tests -p test_plugin_conformance.py -v
```
