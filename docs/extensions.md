<a id="네이티브-관측-확장-설치"></a>

# Installing native observers

[English](extensions.md) | [한국어](ko/extensions.md)

Install and enable an optional metadata observer without rebuilding the gateway core. Package management is currently a separate Python command supplied in the source distribution; execution uses the gateway binary. Read the [architecture and trust model](extensions-design.md) before granting native execution.

<a id="요구사항과-지원-범위"></a>

## Requirements and supported scope

Use Linux or macOS, Python 3.11 or later, and an exact compatible gateway build. The source example needs the repository's pinned Rust 1.98.0 toolchain. Packages must match the host OS/architecture: `linux-x64`, `linux-arm64`, `macos-x64` or `macos-arm64`. CI exercises Linux x64/ARM64 and macOS ARM64; recognition of macOS x64 is not separate hosted acceptance. Windows extension execution is unsupported; ordinary gateway operation is unchanged.

This guide covers `gateway-observer/v1`. This observer receives HTTP header-status/timing metadata, not prompts, tokens, response bodies, account quotas or tool results. Codex Pool, dynamic provider adapters, hot reload, remote registries, automatic downloads and credential access are not supported by this package role. The source manager and reference observer are not included as ready-to-install extension binaries in the ordinary gateway binary package.

Run the following example from a reviewed source checkout with absolute, non-symlink paths. It uses a unique private directory and a deliberately inactive loopback provider. No live credentials or paid requests are required. Installing or inspecting a package never starts its executable.

<a id="로컬-예제-패키지-준비"></a>

## Prepare a local example package

```sh
ROOT="$(pwd -P)"
mkdir -p "$ROOT/.local"
DEMO="$(mktemp -d "$ROOT/.local/extension-demo.XXXXXX")"
CARGO_TARGET_DIR="$ROOT/target" cargo build --locked
CARGO_TARGET_DIR="$ROOT/target" cargo build --locked --example metadata_observer
python3 -B scripts/extension_manager.py package \
  --binary "$ROOT/target/debug/examples/metadata_observer" \
  --license-file "$ROOT/LICENSE" --output "$DEMO/package" \
  --id metadata-counter --version 0.1.0 > "$DEMO/package-result.json"
SHA="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["package_sha256"])' "$DEMO/package-result.json")"
```

The helper copies compiler output into independent files and prints the SHA-256 of canonical `extension.json`, including its final newline. Here the digest is derived from your explicitly selected local build. For someone else's package, obtain the expected digest through a trusted independent channel; hashing untrusted bytes and trusting the result does not verify a publisher.

This local example includes the project license for private synthetic testing. The helper is not a release/license auditor. Before redistributing a binary, include its applicable third-party evidence and corresponding source using the [release procedure](release.md). A manually prepared flat package may include additional hashed notice files within the documented limits; do not omit required notices to fit a package. The helper creates a new output directory and never overwrites an existing package.

<a id="설치와-승인-및-활성화"></a>

## Install, approve and enable

```sh
python3 -B scripts/extension_manager.py inspect \
  --package "$DEMO/package" --expected-sha256 "$SHA"
python3 -B scripts/extension_manager.py install \
  --store "$DEMO/store" --package "$DEMO/package" --expected-sha256 "$SHA"
python3 -B scripts/extension_manager.py enable \
  --store "$DEMO/store" --id metadata-counter --version 0.1.0 \
  --package-sha256 "$SHA" \
  --grant observe_http_metadata --grant write_private_state
python3 -B scripts/extension_manager.py status --store "$DEMO/store"
```

Installation verifies all declared files and leaves the package inactive. Enabling requires both explicit grants, checks the installed bytes again, creates the per-package state directory and writes `active.json`. It does not execute code. The file uses `gateway-extension-lock/v1` and contains a monotonically increasing `generation` and exact package selections.

Status reports the saved activation configuration with `runtime_checked:false`. It is not a complete installed-package inventory, a process health check or proof that a running gateway uses the latest lock. Multiple versions can be installed; only one version/digest per package ID can be enabled. Unknown protocol fields and permission escalation are rejected rather than silently ignored.

<a id="선택형-gateway-검사와-시작"></a>

## Inspect and start the opted-in gateway

```sh
cat > "$DEMO/gateway.toml" <<'TOML'
listen = "127.0.0.1:0"
local_token_env = "ARG_LOCAL_TOKEN"
[providers.demo]
base_url = "http://127.0.0.1:9"
api_key_env = "ARG_DEMO_KEY"
[models.demo]
provider = "demo"
upstream_model = "synthetic"
TOML
GATEWAY="$ROOT/target/debug/agent-response-gateway"
"$GATEWAY" check-config --config "$DEMO/gateway.toml" \
  --extensions-lock "$DEMO/store/active.json"
"$GATEWAY" manifest --config "$DEMO/gateway.toml" \
  --extensions-lock "$DEMO/store/active.json" > "$DEMO/execution-manifest.json"
ARG_LOCAL_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')" \
ARG_DEMO_KEY="synthetic-unused-upstream-key" \
  "$GATEWAY" serve --config "$DEMO/gateway.toml" \
  --extensions-lock "$DEMO/store/active.json"
```

This configuration has no inference server; only gateway startup and local health inspection are intended. A model request to it should fail, not fall back to a real service. Start-up does not probe that provider. The first output line identifies the selected port and extended execution digest. From another shell, a request to the reported loopback address's health endpoint creates a numeric observation. Stop the gateway with its normal termination signal before continuing the remaining commands.

Before an embedding host starts its agent, compare the offline and readiness `execution_sha256`, require the supported extended schemas and verify the core executable separately. Do not publish real manifest paths or configuration references. Without `--extensions-lock`, the existing manifest/readiness formats and extension-free route behavior remain unchanged; installed packages are not auto-discovered.

The example writes `counts.json` under the selected private state directory after the first observation. It contains `schema`, `process_id`, `observed` and `status_counts`, never bodies or tokens. Header status counts are lossy operational examples, not successful-inference totals. The inherited environment is cleared even when the gateway has provider keys.

<a id="비활성화와-버전-변경-및-복구"></a>

## Disable, change versions and recover

```sh
python3 -B scripts/extension_manager.py disable \
  --store "$DEMO/store" --id metadata-counter
python3 -B scripts/extension_manager.py status --store "$DEMO/store"
```

Disabling changes the next-start snapshot; it cannot terminate an already running extension. Stop and restart the gateway to apply it. An empty activation lock still selects the extended execution contract; omit `--extensions-lock` to use the legacy contract. The manager does not delete package versions or private state.

To upgrade, prepare/install another exact package, approve its grants and enable its version/digest, then stop/start the gateway. To roll back, enable the retained prior package explicitly and restart. State is separated by package digest; switching back uses that package's retained state, not a merge of counters from the newer version. There is no automatic migration, uninstall, purge or disk-retention policy. Never edit an active package in place.

A corrupt activation file is an error, not permission to reset to an empty configuration. Restore a reviewed compatible lock only after stopping its owner; do not delete lock files to override a live runtime. These observer-counter recovery rules must not be reused for future OAuth tokens without a separate credential migration design.

<a id="검증과-문제-해결"></a>

## Validation and troubleshooting

```sh
python3 -B -m unittest discover -s scripts/tests -p 'test_extension*.py' -v
cargo test --locked extensions::
cargo build --locked --example metadata_observer
python3 -B scripts/extension_smoke.py \
  --binary target/debug/agent-response-gateway \
  --observer target/debug/examples/metadata_observer
```

The probe packages and runs the actual observer against the actual gateway with a synthetic loopback upstream. It checks offline installation/inspection, unchanged default manifests, exact native JSON/SSE forwarding, authentication separation, frozen activation, exclusive runtime ownership, observer exit/stall isolation and direct-child cleanup. No real OAuth, quota or model service is used. The standard repository format, Clippy, Rust, Python, license and publication checks remain required.

A package error can indicate a wrong trusted digest, unsupported platform/protocol, missing notice, unlisted file, nonprivate mode or a link in the path. Do not bypass checks to make it run. A startup protocol error rejects that opted-in gateway launch. A runtime protocol failure is logged as a fixed diagnostic and disables that observer, not model routes. The smoke test names only a fixed failure phase; it does not print supplied paths, credentials or fixture bodies. Read [the implementation limits and planned roles](extensions-design.md) before treating the foundation as a general plugin SDK.

Usage accounting and the optional recorder are described in the
[token usage accounting guide](usage-accounting.md). Recorder installation,
local commit guarantees and external delivery are separate from HTTP metadata observation.
