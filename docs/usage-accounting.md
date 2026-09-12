<a id="recorder-설치"></a>
<a id="외부-전송"></a>
<a id="전달-보장"></a>
<a id="조회와-유지보수"></a>
<a id="토큰-사용량-계측"></a>
<a id="토큰-의미"></a>
<a id="usage-accounting"></a>

# Token usage accounting

[English](usage-accounting.md) | [한국어](ko/usage-accounting.md)

The gateway extracts provider-reported usage before protocol conversion. An optional
native Usage Recorder commits an operational ledger to SQLite and exports committed
events to PostgreSQL or an HTTP collector. It does not estimate tokens, calculate
bills, execute tools, or store conversations. Provider invoices remain independent.

<a id="token-meaning"></a>

## Token meaning

Each counter in `usage.counters` contains nullable `value` and `source`. Sources are
`reported`, `derived`, `not_reported`, `not_applicable`, or `invalid`. Missing values
are not zero. The wire schema is [UsageEvent](../schemas/gateway-usage-event-v1.schema.json).

| Counter | Meaning |
|---|---|
| `input_tokens` | Whole input, including cache reads and writes |
| `output_tokens` | Whole generated output, including reported reasoning |
| `total_tokens` | Input plus output, checked for overflow and disagreement |
| `input_regular_tokens` | Input outside the known cache read/write partition |
| `cache_read_input_tokens` | Input read from cache; a subset of input |
| `cache_write_input_tokens` | Input written to cache; a subset of input |
| `reasoning_output_tokens` | Reasoning subset of output; never added again |

`cache_write_details` retains supplied TTL breakdowns. `reported` retains only
allowlisted numeric usage paths and their validity, never arbitrary provider JSON.
Negative, fractional, oversized and inconsistent counters remain invalid; valid
independent fields remain available. The recorder uses exact integer arithmetic.

Profiles `responses_v1`, `chat_v1`, and `messages_v1` define API-specific meaning.
A route may explicitly select its matching `usage_profile`; mismatched profiles
are rejected. The default matches the route API. Profiles and the event contract
are bound into the recorder execution manifest.

Responses reads cached/write details and reasoning directly. Chat preserves cache
reads and reasoning but leaves unreported cache writes unknown. Messages preserves
ordinary input, cache reads, cache creation and supplied TTL details separately.
The Messages profile conservatively leaves canonical total input unknown when a
cache component is absent. Its existing client conversion still treats absent
optional cache components as absent contributions to its legacy wire total.
Client Responses usage includes supported cache details, not ledger-only fields.

Messages ordinary input 10, cache read 5 and cache write 2 produce input 17.
Input minus cache read is 12: **input excluding cache reads**, not necessarily a
cache miss. Chat input 10 and cache read 4 yield 6 excluding reads; missing writes
do not establish ordinary input. Output minus reasoning is not necessarily visible
text. Cumulative streaming snapshots replace counters; 1, 3, 8 finish at 8.

<a id="delivery-guarantees"></a>

## Delivery guarantees

| Mode | Behavior |
|---|---|
| `off` | Default binding mode; no recorder process or ledger delivery |
| `best_effort` | Bounded, nonblocking queue; events can be dropped |
| `durable_local` | Start commit before upstream dispatch; recognized final response waits for local commit |

No extension lock means accounting is inactive. The mode in an explicit recorder
binding is required; examples below select `durable_local`.

A call has separate producer, request, attempt, event and revision identities.
Events preserve upstream outcome, gateway outcome, usage finality and observation
completeness independently. No usage is `unobserved`, not a zero-token success.
UTC timestamps use Unix milliseconds. A terminal snapshot commits before a
recognized JSON/SSE final response is exposed. This is not proof the client received
it. A failed local start commit prevents the provider call. Failed final commits
produce a local JSON error or interrupt an already-started stream. No model request
is retried because recording failed.

Native SSE observation is bounded and preserves the original bytes. Malformed or
oversized observation frames do not stop otherwise-supported native forwarding.
They mark collection incomplete. If completion cannot be recognized, the gateway
waits for a termination-record ACK at normal EOF, but cannot guarantee a commit
before the client's final event. Transport limits and protocol conversion errors
still apply. Cancellation does not drain the upstream to obtain usage. A forced
process exit can lose uncommitted observations; started calls lacking a terminal
record remain visibly unknown after restart.

Local commit means SQLite transaction completion, not remote delivery. SQLite uses
WAL, `synchronous=FULL`, a single ledger writer and a busy timeout. Filesystem and
storage hardware still determine power-loss behavior. Committed outbox events are
retried with receiver deduplication; complete exactly-once provider billing is not
promised. Network waits run outside the local IPC writer.

<a id="recorder-installation"></a>

## Recorder installation

Native recording supports Linux and macOS. The ordinary gateway remains available
without native extensions. Build the separate executable explicitly:

```sh
cargo build --locked --release -p gateway-usage-recorder
python3 -B scripts/extension_manager.py package \
  --binary "$PWD/target/release/gateway-usage-recorder" \
  --license-file "$PWD/LICENSE" --output "$PWD/.local/recorder-package" \
  --id usage-recorder --version 0.1.0 --role usage_recorder
```

Follow the [extension installation guide](extensions.md) to install the exact
package digest. Prepare a private, user-owned ledger directory at
`usage/<store_id>` inside the extension store. Initialize it with the recorder's
`init --store` command. The extension manager does not execute package code.

Create private `recorder.json` in that ledger directory:

```json
{"schema":"gateway-usage-recorder-config/v1","destinations":[]}
```

Create a private recorder binding file with the actual configuration file SHA-256:

```json
{"store_id":"primary","mode":"durable_local","queue_capacity":256,"ack_timeout_ms":5000,"config_sha256":"<configuration-file-sha256>"}
```

Activate using `enable --recorder-binding` and all three explicit grants:
`export_usage`, `observe_usage`, `write_usage_store`. The package protocol is
`gateway-usage-recorder/v1`; its role is selected with `--role usage_recorder`.
Exactly one recorder is supported. Queues allow 2–4096 slots and ACK deadlines
1–60000 milliseconds. One terminal slot is reserved per admitted recorded call.
Event frames are limited to 65536 bytes excluding the newline; ACKs to 4096 bytes.

Recorder activation uses `gateway-extension-lock/v2`,
`gateway-extended-manifest/v2` and `gateway-extended-ready/v2`. Consumers must verify
the matching execution digest and reject unsupported versions. Observer-only
activation retains its existing contract and permissions. The ledger identity is
independent of package version, allowing upgrades without abandoning stored usage.
Older executables reject unsupported database schemas; migration is explicit.

Native executables are trusted code, not an OS sandbox. Do not include prompts,
response text, tool data, credentials or arbitrary headers in events. The gateway
does not trust client-supplied tenant or billing identities. A consuming host joins
verified execution context to request identifiers independently.

<a id="external-export"></a>

## External export

Configure up to eight destinations. PostgreSQL uses a dedicated `gateway_usage`
schema initialized explicitly with `initialize-postgres --destination`. It does
not write another application's internal tables. Destination configuration uses
`kind`, `id`, and either `url` plus `bearer_file`, or `connection_file` plus optional
`tls_ca_file`. Secret files are private regular files read directly by the recorder.
The inherited environment is empty. PostgreSQL requires verified TLS; an optional
private CA file supports explicitly trusted certificates.

HTTP requires HTTPS, except numeric loopback test endpoints. Redirects, inherited
proxies, URL credentials, query strings and fragments are rejected. The configured
URL is the full collector endpoint; no private host-specific route is assumed.

Requests use `gateway-usage-batch/v1`, Bearer authentication, and at most 100 events
or 1 MiB per batch. A 200 response must contain `gateway-usage-batch-receipt/v1`
and exactly one receipt per event with `producer_id`, `event_id`, `sha256` and
`status`. Status is `committed`, `duplicate`, `conflict`, or `rejected`. A receipt
acknowledges durable storage, not merely acceptance into a memory queue. A 202
response does not acknowledge a commit. See the [batch schema](../schemas/gateway-usage-batch-v1.schema.json).

Event digests cover UTF-8 JSON with sorted object keys, compact separators and no
terminal newline. Preserve full-width integers. Duplicate identity and identical
content is harmless; changed content is a conflict. Late older revisions cannot
replace newer state. PostgreSQL enforces the same rules transactionally.

Temporary failures and uncertain ACKs retry the same event with bounded exponential
backoff. Most HTTP client errors block delivery; 408 and 429 remain retryable.
Conflicts remain visible. Use `retry-blocked --destination` after correcting a
blocked destination. A destination ID binds its configuration; changing it is
rejected, and removing a destination with pending events is rejected. A new ID
receives future events only. Raw remote diagnostics and secrets are not logged.

<a id="queries-and-maintenance"></a>

## Queries and maintenance

The recorder CLI provides `status`, `query`, `export`, `aggregate`, `backup`,
`migrate`, `prune`, `flush`, and `retry-blocked`. Commands take `--store` except
`serve`, which runs in the activated ledger directory. Read-only queries may run
while serving; maintenance and manual export require the writer to be stopped.

`query --attempt` returns the event and derived `non_read_input_tokens`;
`--limit` is capped at 1000. `export` returns
immutable events with a cursor for `--after-rowid`. `aggregate --from-ms --to-ms
--timezone` uses a half-open interval attributed to attempt start time, defaulting
to UTC and allowing IANA zones. It returns field observation counts, final/partial/
unobserved calls, unfinished calls and cache-ratio coverage alongside exact sums.
The cache-read ratio uses only calls whose input and cache read are both known.
Unknown or zero denominators return null. Cost calculation is not implemented.

`backup --output` refuses overwrite. `migrate --backup` verifies the current schema
and writes a backup; the first schema has no prior migration. `prune --before-ms`
removes only finished calls with no pending external deliveries. Automatic expiry
is disabled. Keep backups before maintenance and package rollback.

Synthetic checks use `scripts/usage_smoke.py` and `scripts/usage_postgres_test.py`.
The latter owns a disposable TLS PostgreSQL instance and stops it afterward.
Neither synthetic conformance nor a passing local check proves actual provider
billing accuracy or operational acceptance by an external host.
