# Independent Recorder example

Copy this directory elsewhere to build/package with explicitly provisioned Python
3.11+ and its standard library. No gateway crate, SDK, repository or download is
needed. The absolute interpreter selected by build.py must exist on the installation
host. Source, license and both event schemas are included in the flat package.

```sh
python3.14 -B package.py --output /absolute/new-package --target macos-arm64
mkdir -m 700 /absolute/scratch-state
cd /absolute/scratch-state
/absolute/new-package/extension init
/absolute/new-package/extension serve-v2
```

Use the actual host target. `init` creates a fresh, separate example SQLite ledger
and producer identity; never point this example at a production Recorder store.
The example supports `gateway-usage-recorder/v2` with both event versions. It is
trusted native code, not sandboxed. It does not export, access model bodies, execute
tools, migrate a core Recorder database or provide its CLI/retention facilities.

Each event is one canonical UTF-8 JSON line. The example checks bundled structural
schemas, closed source/value classes, revision/timestamp constraints, producer and
attempt identity, then stores exact bytes in a FULL synchronous WAL transaction.
It ACKs only after commit, with the SHA-256 of the exact event bytes excluding LF.
Duplicates return the same ACK; conflicting identity/revision/bytes fail. V1 and V2
remain discriminated by their schema and their payloads are never rewritten.
Invalid canonical observations remain Invalid rather than missing or zero. Raw
malformed numbers cannot pass structural validation or be stored as evidence.

This is an example of the public process/storage contract, not the gateway's
complete usage semantic verifier. It relies on the trusted host to normalize V1
provider paths, validate plugin arithmetic and attach interpretation identity.
Static schema checks alone cannot prove any of those facts. Production Recorder
and full conformance qualification require the host's additional semantic tests.
The bundled schemas must remain identical to their public counterparts.

The independent tests use numeric-only synthetic events, reject malformed frames,
verify V1/V2 byte hashes and deduplication, and restart the process to inspect exact
stored bytes. They do not certify a real provider or production database migration.

## Installing the example into a host

The complete package (eight payload files plus extension.json) must be installed with the standalone package manager;
packaging only the executable would omit the event schemas it loads. Use absolute
EXTENSION_STORE/PACKAGE_DIR paths and the reviewed PACKAGE_SHA256 from package.py.
Create fresh private usage and usage/independent directories under that extension
store, mode 0700. Put recorder.json (mode0600) in usage/independent:

```json
{"schema":"gateway-usage-recorder-config/v1","destinations":[]}
```

Prepare /absolute/private-recorder-binding.json with this object and the actual
SHA-256 of recorder.json bytes (including newline if present):

```json
{"store_id":"independent","mode":"durable_local","queue_capacity":256,"ack_timeout_ms":5000,"config_sha256":"<SHA-256 of the exact recorder.json bytes>"}
```

```sh
python3 -B /absolute/tools/extension_manager.py install --store "$EXTENSION_STORE" --package "$PACKAGE_DIR" --expected-sha256 "$PACKAGE_SHA256"
(cd "$EXTENSION_STORE/usage/independent" && "$EXTENSION_STORE/packages/synthetic-recorder/1.0.0/$PACKAGE_SHA256/extension" init)
python3 -B /absolute/tools/extension_manager.py enable --store "$EXTENSION_STORE" --id synthetic-recorder --version 1.0.0 --package-sha256 "$PACKAGE_SHA256" --grant export_usage --grant observe_usage --grant write_usage_store --recorder-binding /absolute/private-recorder-binding.json
```

Run init only after the fresh usage directory is prepared; it creates events.sqlite3
and the producer identity there. Install/enable never initialize plugin state.
The gateway obtains producer_id from Ready and later starts serve-v2 in the same
selected directory, so it does not need to understand the example's private SQL.
The pinned interpreter must already be present on that host. These commands are
source-verified instructions, not evidence of a completed installed-host smoke.

The optional management/team usage backend supports the shipped Recorder SQLite
layout, not arbitrary Recorder storage. Leave that usage-directory setting absent
for this example. Its events.sqlite3 and metadata(producer) are incompatible with
usage.sqlite3 and metadata(key,value). Configuring that query backend against the
example must fail explicitly without a replacement database, rewritten events or
invented statistics. Recording IPC can work while optional queries remain unavailable;
a successful ACK is not a query-compatibility claim. No generic query protocol or
implicit migration is supplied by the example.
