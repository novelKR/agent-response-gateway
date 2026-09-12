<a id="로컬-합성-패키지-준비와-활성화"></a>
<a id="버전-변경과-복구"></a>
<a id="선언-가져오기와-시작"></a>
<a id="패키지-계약"></a>
<a id="호환성-프로파일-팩"></a>

# Compatibility profile packs

[English](profile-packs.md) | [한국어](ko/profile-packs.md)

Reuse capability declarations and tool compatibility policies through explicitly
installed, non-executable data packages. Host configuration retains provider URLs,
model names, authentication, routing, continuation storage and usage recording.
Profile packs work on the gateway's supported Linux, macOS and Windows platforms.

## Package contract

A `gateway-profile-pack/v1` package is one UTF-8 JSON file. It contains an `id`,
three-part numeric `version`, named `capabilities` and `policies` exports,
`evidence`, and original `LICENSE` text with optional `NOTICE` text in `notices`.
The package command normalizes source JSON to sorted compact JSON with a final
newline. SHA-256 covers those exact bytes. Duplicate keys, unknown fields and
unsupported contracts fail validation. A package cannot contain an executable,
entrypoint, permission grant, credential reference, provider binding or file path.

Capability exports contain `api`, optional `reasoning_contract`, `context_window`,
`max_output_tokens`, `tested_codex_version` and the existing `support` map.
Their profile version is the package version. Policies use the existing
[tool policy contract](protocol.md#checked-responses-tools). Missing feature
declarations mean unsupported; the package cannot add an adapter or grammar rule.
Normal route validation still rejects incompatible API, model and policy bindings.

Evidence entries contain a bounded `description`, optional HTTPS `source_url`
and optional `artifact_sha256`. They are publisher claims, including the claimed
tested Codex version. The gateway does not fetch evidence, run qualification tests
or attest provider behavior. Obtain expected digests through a trusted channel
when selecting someone else's package. Installation and byte verification do not
establish publisher identity or approve redistribution rights.

Limits are 256 KiB per package, 64 total exports, 16 evidence entries and 16 active
packs. Export names and package IDs use lowercase ASCII letters, digits and
hyphens, start with a letter, and exclude Windows device names. Notice content is
embedded data and never interpreted as a path. Use an operator-controlled local
store with ordinary filesystem access controls; packs contain no secrets. Static
symlinks, Windows reparse points and nonregular package files are rejected. This
is not isolation against a hostile user who can concurrently rewrite that store.

## Prepare and activate a local fixture

From a source checkout, build the gateway and prepare a synthetic package source.
The following POSIX shell example uses Python only to prepare data; the gateway's
`profile-pack` commands themselves require no Python installation. Windows hosts
can use the same JSON and CLI arguments with their native executable path.

```sh
cargo build --locked
mkdir -p .local/profile-demo
python3 - <<'PY'
import json
from pathlib import Path
source = {
    "schema": "gateway-profile-pack/v1", "id": "local-fixture", "version": "1.0.0",
    "capabilities": {"functions": {
        "api": "responses", "context_window": 8192, "max_output_tokens": 2048,
        "tested_codex_version": "synthetic-not-qualified",
        "support": {"function_tools": "native", "tool_choice": "native"}}},
    "policies": {"tools": {"version": 1, "tools": {
        "custom_input": "function_json", "namespaces": "flatten"}}},
    "evidence": [], "notices": {"LICENSE": Path("LICENSE").read_text(encoding="utf-8")}}
Path(".local/profile-demo/source.json").write_text(json.dumps(source), encoding="utf-8")
PY
target/debug/agent-response-gateway profile-pack package \
  --source .local/profile-demo/source.json --output .local/profile-demo/pack.json
target/debug/agent-response-gateway profile-pack inspect \
  --package .local/profile-demo/pack.json
target/debug/agent-response-gateway profile-pack install \
  --package .local/profile-demo/pack.json --store .local/profile-demo/store
```

Package creation refuses an existing output. Installation stores immutable bytes
at `packages/<id>/<version>/<sha256>.json` and leaves them inactive. Installing
the same verified bytes again is harmless. Select the exact digest printed by
the package command for this locally generated fixture:

```sh
target/debug/agent-response-gateway profile-pack enable \
  --store .local/profile-demo/store --id local-fixture --version 1.0.0 \
  --sha256 <exact-package-sha256>
target/debug/agent-response-gateway profile-pack status --store .local/profile-demo/store
```

Enable writes `active.json` using `gateway-profile-pack-lock/v1`, a monotonically
increasing `generation` and sorted exact ID/version/digest entries. One binding
per package ID can be active. There is no registry lookup, automatic download,
version range, implicit activation or hot reload. Status verifies the saved
active snapshot; it does not inspect a running process or list inactive packages.

## Import declarations and start

Save a host configuration using explicit import aliases. The inactive loopback
endpoint below is suitable for offline inspection; it has no inference service.

```toml
[providers.demo]
base_url = "http://127.0.0.1:9/v1"
api_key_env = "ARG_DEMO_KEY"
[models.writer]
provider = "demo"
upstream_model = "host-selected-model"
auth = "bearer"
capability_profile = "local-functions"
compatibility_policy = "local-tools"
[capability_profile_imports.local-functions]
pack = "local-fixture"
export = "functions"
provider = "demo"
upstream_model = "host-selected-model"
[compatibility_policy_imports.local-tools]
pack = "local-fixture"
export = "tools"
```

Use `--profile-packs-lock .local/profile-demo/store/active.json` with `check-config`,
`manifest` and `serve`, together with the normal `--config` argument. Without the
flag, unresolved imports fail. Inline and imported declarations may coexist under
different aliases; duplicate aliases fail instead of overriding or deep-merging.
Importing an inactive package, missing export or wrong export kind also fails.

The explicit lock selects `gateway-embedded-manifest/v5` and `gateway-ready/v5`,
including when the active pack list is empty. The configuration binds the frozen
activation, complete packages, evidence status and host imports without local
package paths. Each affected route binds its selected export and package digest
in its adapter identity. Changing selected package bytes invalidates that route's
existing continuation origin. A generation change alone changes the configuration
digest but does not change the route origin. Already running processes retain
their original verified snapshot.

With `--extensions-lock`, the wrapper uses `gateway-extended-manifest/v5` and
`gateway-extended-ready/v5`. Existing observer and usage-recorder permissions,
protocols and publication barriers remain in force. Packs introduce no native
process and do not change managed replay formats. A host must support v5 and
compare the inspected and ready digests before starting its agent. Configurations
without a profile-pack lock keep their existing manifest and readiness versions.

## Change versions and recover

Disable the selected ID, enable the explicitly chosen new version/digest, inspect
the resulting host manifest, then restart the gateway. No command replaces an
existing active binding implicitly. Disabling retains installed bytes and can
remove a damaged package binding; remaining active packages must still verify.
To roll back, explicitly reselect the earlier retained bytes and restart. The
host remains responsible for deciding whether an earlier conversation may resume.

```sh
target/debug/agent-response-gateway profile-pack disable \
  --store .local/profile-demo/store --id local-fixture
```

Activation writers are serialized by `activation.writer`. A new lock is written
and synced to `activation.next`, then atomically renamed over `active.json`.
Unix also syncs the directory; this is not a cross-platform power-loss durability
guarantee. An interrupted writer leaves a marker or temporary file and later
writes fail explicitly. Stop management commands, inspect the canonical active
lock and retained package hashes, preserve evidence of the interrupted operation,
then remove the stale marker/temporary file before retrying. Startup never repairs
or substitutes package bytes. Malformed locks require explicit operator recovery.
