<a id="플러그인-검증"></a>

# Plugin verification

[English](plugin-verification.md) | [한국어](ko/plugin-verification.md)

Use separate evidence for package declarations, executable protocol behavior, installed gateway integration and each native platform. A successful declaration check does not prove a handshake, and a successful handshake does not prove managed persistence or compatible usage consumers. These procedures use synthetic fixtures; they do not qualify an actual external supplier or authorize an unreviewed package.

Read the [authoring contract](plugin-authoring.md), [provider contract](provider-plugins.md), [protected continuity](provider-continuation.md) and [usage provenance](usage-provenance.md) together. This page describes commands and result interpretation; it does not assert that a particular artifact has passed them.

<a id="독립-산출물-준비"></a>

## Prepare independent artifacts

Copy the complete provider or Recorder example project outside the gateway checkout before building. Include its license and source files. The independent projects import no gateway crate, reference engine or SDK. Any implementation language may satisfy the public protocol; an initial native package must provide an executable entrypoint for its declared platform.

The Python examples require an explicitly provisioned Python 3.11+ interpreter at the absolute path selected during building. Builders, installers and runners download no interpreter or package. A target declaration does not cross-compile an executable or prove its ABI. Installation and execution recheck the actual host.

Obtain the package manifest digest through a trusted channel and retain the exact manifest, all declared file hashes, package version, role protocol and capability declaration. A digest identifies bytes; calculating it from untrusted input does not authenticate the publisher. Review the requested grants before activation. Native processes retain the host user's OS authority even with an empty environment; the IPC boundary is not a sandbox.

<a id="독립-프로토콜-프로필"></a>

## Standalone protocol profiles

The standalone standard-library tool uses version `2.0.0`. Copy it independently and select `--execute` only for reviewed native code. Use a private scratch directory owned by the invoking user. Static inspection never installs, activates or executes the package.

```sh
python3 -I -B conformance.py --package /absolute/package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256
python3 -I -B conformance.py --package /absolute/provider-package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch --profile synthetic-provider/v1
python3 -I -B conformance.py --package /absolute/recorder-package \
  --expected-sha256 TRUSTED_MANIFEST_SHA256 --execute \
  --state-root /absolute/private-scratch --profile recorder-events/v2 \
  --recorder-state-fixture /absolute/private-disposable-fixture
```

| Profile | Checks | Limit |
|---|---|---|
| `wire` | Observer Ready/ACK, generic provider Ready, Recorder v2 with a supplied fixture | Generic provider semantic checks remain incomplete |
| `synthetic-provider/v1` | Synthetic JSON, SSE, tools, numeric observations and opaque state messages | Requires the synthetic contract, not an arbitrary supplier's semantics |
| `recorder-events/v2` | V1/V2 and invalid-observation ACKs, duplicate delivery and restart | Proves stable producer and repeat ACK, not stored payload retrieval or power-loss durability |

Prepare Recorder state using the selected Recorder's own documented initialization procedure. The runner copies the supplied fixture, rejects links and overlapping paths, and does not modify the original. It never guesses initialization or opens operational storage. Codec and Recorder v1 executable runners remain unavailable. See the [tool reference](../tools/plugin-conformance/README.md) for exact bounds and profile requirements.

<a id="버전별-증거-해석"></a>

## Interpret versioned evidence

The [report schema](../schemas/gateway-plugin-conformance-report-v2.schema.json) defines `gateway-plugin-conformance-report/v2`. Preserve `tool_version`, `tool_sha256`, `package_sha256`, `fixture_sha256`, package/role contracts, `target`, `host_target` and `profile` with the check list. Null fixture identity means no fixture digest was established; it is not evidence for another fixture. Report v1 readers must explicitly adopt v2.

Every required check must be `pass` for overall pass. Any failed check produces `fail`; incomplete required coverage produces `not-run`. The non-required `host.integration` check stays `not-run` even after standalone protocol success. Static-only success exits 0 while retaining incomplete execution coverage; requested incomplete execution exits 2; failed checks exit 1. Fixed diagnostic codes omit request/response bodies, credentials, child output and local absolute paths. Reports are local evidence, not signatures or attestations.

<a id="설치된-게이트웨이-수용-검증"></a>

## Installed gateway acceptance

Use the [installed acceptance harness](../scripts/provider_acceptance.py) with a prebuilt normal gateway and explicit manager/example paths. It copies public projects and tools into disposable state outside any checkout, builds and installs packages through the ordinary manager, selects explicit grants and model routes, and verifies that installation did not change the gateway binary digest.

```sh
python3 -B scripts/provider_acceptance.py \
  --binary /absolute/build/agent-response-gateway \
  --manager /absolute/tools/extension_manager.py \
  --provider-example /absolute/projects/provider \
  --recorder-example /absolute/projects/recorder \
  --query-recorder /absolute/build/gateway-usage-recorder
```

The harness uses generated synthetic credentials, numeric loopback HTTP, disabled proxy inheritance and redirects, and bounded child cleanup. It exercises native JSON/SSE/tool exchanges, actual gateway process restart and managed state resume, preserved event bytes and exact plugin usage identity. Additional assertions cover invalid/missing numeric observations, rejected stale or altered continuity input, package replacement and unfinished-attempt recovery. Inspect the emitted check list for what completed; a source test or copied project alone does not establish host execution.

The separate `gateway-plugin-acceptance-report/v1` report uses `pass_positive_scope` only for its completed scope. Its remaining checks stay explicitly `not-run`; do not equate this status with complete qualification. Omitting `--query-recorder` leaves the incompatible query-backend probe unexecuted. Retain this report with exact binary/package hashes and platform, separately from standalone reports, CI results and actual-provider evidence.

<a id="플랫폼과-오프라인-컨테이너"></a>

## Platforms and offline containers

Run native executable checks separately on each supported Linux or macOS target. Windows native plugin execution is unsupported. The local full validation therefore remains incomplete with exit code 2 for this native acceptance check on Windows; CI explicitly skips the native step there. Static acceptance of a foreign target and rejection on an incompatible execution host are separate checks. A Linux container does not validate a macOS executable or its interpreter path.

Use the [tool distribution procedure](../tooling/plugin-tools/README.md) to assemble exact source and the selected conformance script. Review and preload the pinned base image before an offline build. Run with network disabled, a read-only root, minimal synthetic input mounts, dedicated writable scratch, a non-root numeric user and CPU/memory/PID limits. Never pass an operational store, credentials, home directory or Docker socket. Package bytes are trusted native code inside this verification environment; the container does not change the product runtime trust model. Keep cross-target static evidence and native execution reports distinct.

<a id="recorder-소비자와-롤백"></a>

## Recorder consumers and rollback

An independent Recorder may implement event/ACK IPC using its own storage. That compatibility does not implement a generic SQL query contract. Management and team readers support the shipped Recorder's SQLite layout only; configuring an incompatible external store must fail explicitly. The independent fixture's ledger assertions do not prove management/Web query support. Keep local commit, remote export receipt and consumer interpretation separate.

Before replacement or rollback, stop affected writers and preserve original databases, sidecars, configuration, exact previous package bytes and matching continuation keys. Disable the new model path and reselect a compatible binary/package, or start a new session. Package replacement never automatically migrates old sessions. Older binaries are not guaranteed to read new state or V2 usage: use the documented compatible backup/upgrade procedure, never lower version markers or rewrite recorded provenance. An unfinished attempt requires explicit host recovery rather than automatic inference retry. See [continuity recovery](provider-continuation.md) and [usage storage compatibility](usage-provenance.md).
