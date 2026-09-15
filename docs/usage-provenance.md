<a id="버전이-있는-사용량-출처"></a>

# Versioned usage provenance

[English](usage-provenance.md) | [한국어](ko/usage-provenance.md)

Usage event versions distinguish the interpreter of provider semantics. Existing
builtin adapters continue to produce `gateway-usage-event/v1`; their original
normalization, canonical bytes, event hashes and ACK meaning are preserved.
`gateway-usage-event/v2` records an explicitly trusted provider plugin's numeric
interpretation without representing it as builtin parser verification. Provider
route activation remains unavailable until continuity and recording acceptance
are integrated. Installing a Recorder v2 package does not enable provider routes.

The [event schema](../schemas/gateway-usage-event-v2.schema.json) and
[Recorder schema](../schemas/gateway-usage-recorder-v2.schema.json) describe the
public wire. The [independent Recorder example](../tools/plugin-conformance/examples/recorder/README.md)
shows package creation and native IPC without importing the gateway.

<a id="근거와-이벤트-식별"></a>

## Evidence and event identity

V2 preserves the common host-authored producer, request, attempt, event, revision,
provider/model, configuration, time, outcome and finality fields. It replaces V1
`profile` with required `interpretation`: `kind: trusted_provider_plugin`,
`protocol: gateway-provider/v1`, `provider_protocol`, `package_id`,
`package_version`, `package_sha256` and `executable_sha256`.
The host copies this identity from the exact inspected selection; plugin replies
cannot assign producer IDs, package identity, credentials, commit or transport outcome.

Canonical numeric counters retain `reported`, `derived`, `not_reported`,
`not_applicable` and `invalid` sources. Malformed provider numbers and arithmetic
violations become typed invalid evidence, never raw rejected values or error text.
Normal numeric siblings remain available. Unknown is not zero, and an invalid
observation cannot authorize successful terminal or tool output. V2 `reported` is
an empty object and `cache_write_details` an empty array: provider-specific raw
usage paths and cache TTL buckets are not part of provider v1 interpretation.

All events remain canonical UTF-8 JSON without a trailing LF in their byte identity.
IPC adds exactly one LF. A committed ACK contains `type`, `event_id`, `sha256`, and
hashes the original canonical event bytes excluding LF. V1 and V2 events remain
separate versions within a shared ledger; there is no reserialization of old rows,
hash replacement or implicit interpretation change within an existing attempt.

<a id="recorder-패키지와-시작-호환성"></a>

## Recorder package and startup compatibility

Recorder v2 uses package v2, `gateway-usage-recorder/v2`, `usage-store/v2`, and the
existing `export_usage`, `observe_usage`, `write_usage_store` grants. Its required
capability object is exactly:

```json
{"schema":"gateway-plugin-capabilities/v1","apis":[],"features":["usage_event_v1","usage_event_v2"],"requires":["usage_recorder_ipc_v2"]}
```

These feature names belong only to the Recorder role. Arrays are ordered and
unique. The exact declaration is repeated in Ready alongside `type: ready`,
`protocol` and `producer_id`. The fixed invocation is `serve-v2`; legacy `serve`
continues to use Recorder v1. A manifest edit cannot upgrade an executable.

```sh
python3 -B scripts/extension_manager.py package \
  --binary /absolute/path/recorder --license-file /absolute/path/LICENSE \
  --output /absolute/new-package --id recorder --version 1.0.0 \
  --role usage_recorder --recorder-protocol gateway-usage-recorder/v2 \
  --capabilities /absolute/path/recorder-capabilities.json
```

Static inspect/install does not execute code. Startup checks exact Ready capability
compatibility. An active Recorder v1 is incompatible with a selected provider path;
reject that combination before model work rather than dropping provenance. Native
execution remains trusted same-user execution, not an OS sandbox. Installation
never downloads language runtimes or dependencies.

Selecting Recorder v2 uses `gateway-extension-configuration/v5` and outer
`gateway-extended-manifest/v11` / `gateway-extended-ready/v11`. Its outer
configuration advertises `usage_event_schemas` containing both event versions;
it does not label all events as V2. `usage_profiles` continues to list builtin V1
profiles. Old selections retain their prior schemas and singular `usage_contract`.
Installed, selected and observed-effective state remain distinct.

<a id="저장내보내기복구-경계"></a>

## Storage, export and recovery boundaries

The ledger stores exact event payloads and hashes. Before acknowledging a live duplicate, exporting, or returning an event through the shipped query readers, it verifies the original stored hash and canonical bytes. A mismatch fails without rewriting the row or assigning a replacement hash. Retention tombstones keep their existing hash-only duplicate semantics. V2 event interpretation does not
require new event columns or bulk conversion of V1 rows. Storage compatibility
must be checked explicitly; a newer event contract is not permission to downgrade
a database marker or rewrite historical payloads for an older binary. Preserve the
original database and backup before any documented compatibility upgrade.

Initialize a new V2 ledger in a prepared private directory, or stop the existing writer and explicitly upgrade with a new backup file.

```sh
gateway-usage-recorder init-v2 --store /absolute/new-private-ledger
# For an existing ledger, stop its writer first and choose a new backup file:
gateway-usage-recorder upgrade-v2 --store /absolute/existing-private-ledger --backup /absolute/private-backups/before-v2.sqlite3
```

The upgrade sets SQLite `user_version` to 2 as a compatibility guard without rewriting existing event rows. Older writers reject it. Recovery disables the new route and selects a compatible binary/package, or separately restores the pre-upgrade backup. Never lower the marker to 1 on a store containing V2 data.

Recorder configuration remains `gateway-usage-recorder-config/v1`. A V2 HTTP destination explicitly uses `kind: http_v2` with the existing `id`, `url`, `bearer_file` fields. Wire envelopes use `gateway-usage-batch/v2` and `gateway-usage-batch-receipt/v2`; per-event hashes continue to identify the original event bytes.

V1 HTTP and PostgreSQL export retain their existing contracts. Provider V2 export
requires an explicitly selected V2 HTTP destination and V2 batch/receipt support.
Legacy HTTP and PostgreSQL v1 destinations reject or block unsupported V2 delivery
before transmission, retaining the outbox and provenance. No automatic PostgreSQL
DDL migration or V2 PostgreSQL support is implied. An invalid or unsupported receipt
cannot be treated as successful export, and a remote receipt is distinct from the
local durable commit ACK.

Management and team consumers must understand the event version and interpreter
identity; aggregation cannot erase that distinction under a shared provider/model
label. Reports must keep invalid/missing observations, unfinished attempts and
local commit versus export status distinct. Native and container checks, synthetic
provider fixtures and actual provider qualification remain separate evidence.

Team queries retain the `gateway-team-http/v1` envelope. If any attempt is V2,
`usage_event_schemas` must be exactly `["gateway-usage-event/v1","gateway-usage-event/v2"]`.
V1-only responses do not add that marker. The Web consumer validates event kinds,
identity, numeric shape and marker, then preserves the marker when either page of
a merged view contains V2. Original request records and attempt objects are unchanged.

HTTP receiver authors should use the [V2 batch schema](../schemas/gateway-usage-batch-v2.schema.json)
and [V2 receipt schema](../schemas/gateway-usage-batch-receipt-v2.schema.json) together.
Schemas alone cannot validate original event hashes or exact complete matching receipts.

<a id="외부-recorder-상태와-조회-지원"></a>

## External Recorder state and query support

An external Recorder can use its own private storage while implementing the event/ACK
contract. The gateway obtains producer identity from Ready; its Recorder start path
does not open the plugin's database. The management and team query backend is a
separate implementation that reads only the shipped Recorder's SQLite layout.
Recorder IPC compatibility and `usage-store/v2` do not declare compatibility with
that query layout or provide a generic query protocol.

For the independent example, first copy/build its complete package and inspect the
reported digest. Set the absolute `EXTENSION_STORE`, `PACKAGE_DIR`, and reviewed
`PACKAGE_SHA256` values below. After installation and before initialization, create
private directories at the store's usage parent and usage/independent with mode 0700.
Place a mode-0600 recorder.json in usage/independent with this empty export config:

```json
{"schema":"gateway-usage-recorder-config/v1","destinations":[]}
```

Create a private binding file containing the following object, replacing the hash
placeholder with the digest of the exact recorder.json bytes, including a final
newline if present. The configuration must remain unchanged while selected.

```json
{"store_id":"independent","mode":"durable_local","queue_capacity":256,"ack_timeout_ms":5000,"config_sha256":"<SHA-256 of the exact recorder.json bytes>"}
```

```sh
python3 -B /absolute/tools/extension_manager.py install --store "$EXTENSION_STORE" --package "$PACKAGE_DIR" --expected-sha256 "$PACKAGE_SHA256"
(cd "$EXTENSION_STORE/usage/independent" && "$EXTENSION_STORE/packages/synthetic-recorder/1.0.0/$PACKAGE_SHA256/extension" init)
python3 -B /absolute/tools/extension_manager.py enable --store "$EXTENSION_STORE" --id synthetic-recorder --version 1.0.0 --package-sha256 "$PACKAGE_SHA256" --grant export_usage --grant observe_usage --grant write_usage_store --recorder-binding /absolute/private-recorder-binding.json
```

The example's fixed init command is run in the actual selected usage directory,
not the package directory or an unrelated scratch directory. Initialization is an
explicit preparation action; install and enable do not run it automatically.
The host later invokes the installed executable with serve-v2 in that same usage
directory. All packaged schema/source resources must remain present. These are
source-verified preparation instructions, not a claim that a host acceptance run
has already completed.

For this example, omit the management application's optional usage directory setting.
Its events.sqlite3 and metadata producer column deliberately differ from the shipped
usage.sqlite3 schema. Selecting it as the shipped SQLite query backend fails explicitly;
it must not fabricate counts, initialize a replacement database, rewrite stored events
or infer attribution from the model name. Without a compatible configured query
backend, management usage is unavailable and team attribution remains unobserved or
unattributed as appropriate. Event recording can still operate independently.
An external implementation may separately implement the documented shipped SQLite
layout, but that requires its own query-compatibility validation.
