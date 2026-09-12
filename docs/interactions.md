<a id="gemini-interactions-호스트-연동"></a>
<a id="설정"></a>
<a id="지원-범위와-사용량"></a>
<a id="호스트-제어와-재개"></a>
<a id="백업과-실패-복구"></a>

# Gemini Interactions host integration

[English](interactions.md) | [한국어](ko/interactions.md)

The supported test combination is host-managed Codex 0.154.0 with synthetic Gemini
Interactions. Requests enter through Responses, verified continuation and the common
IR. Actual Gemini model compatibility and quality require separate qualification;
ordinary CLI/Desktop direct connections are not covered. The gateway never runs tools.

## Configuration

Use an explicit model/profile and Google API-key authentication. The API prefix is
https://generativelanguage.googleapis.com/v1; requests append /interactions. The pinned
[v1 contract](../tests/interactions/wire-lock.json) must not be mixed with v1beta.
The example requires a host-selected model and store identity before it can run.

```toml
listen = "127.0.0.1:0"
[providers.google]
base_url = "https://generativelanguage.googleapis.com/v1"
api_key_env = "ARG_GOOGLE_KEY"
[models."example/gemini"]
provider = "google"
upstream_model = "CHOOSE_MODEL"
api = "gemini_interactions"
auth = "google_api_key"
capability_profile = "gemini"
[capability_profiles.gemini]
version = "1"
provider = "google"
upstream_model = "CHOOSE_MODEL"
api = "gemini_interactions"
context_window = 32768
max_output_tokens = 1024
tested_codex_version = "0.154.0"
[capability_profiles.gemini.support]
instructions = "native"
instruction_hierarchy = "bridged_gemini_instruction_envelope"
function_tools = "native"
custom_tools = "bridged_custom_tool_json"
custom_grammar = "bridged_codex_patch_grammar"
namespaced_tools = "bridged_tool_namespace"
tool_choice = "native"
parallel_tool_control = "native"
max_output_tokens = "native"
reasoning_effort = "native"
structured_output = "native"
strict_structured_output = "native"
[continuation]
directory = "/ABSOLUTE/PRIVATE/DIRECTORY"
store_id = "COPY_INITIALIZED_STORE_ID"
realm = "host-realm"
generation = "1"
key_id = "stable-key-1"
key_env = "ARG_CONTINUATION_KEY"
control_token_env = "ARG_CONTROL_TOKEN"
max_store_bytes = 1073741824
```

The host creates a private directory, using mode 0700 on Unix or an owner-only ACL
on Windows. Use a native absolute path without symlink components. Initialize once:

```sh
agent-response-gateway init-continuation --directory /ABSOLUTE/PRIVATE/DIRECTORY
```

Copy the returned store_id into the configuration. The host supplies an independently
generated stable 256-bit key as 64 lowercase hex characters through key_env. Keep its
key_id and value in protected host storage across restarts. Do not regenerate the key
on every launch. Supply a separate control token of 32–4096 printable ASCII characters,
a separate local Codex token, and the Google key. Secret values must all differ.
The existing api_key authentication still means x-api-key; google_api_key means
x-goog-api-key. Only the local token and session header go to Codex.

The enabled manifest/readiness schemas are gateway-embedded-manifest/v3 and
gateway-ready/v3. Compare the offline configuration digest before launching Codex.
Old hosts must reject unfamiliar contracts. The manifest includes configuration and
store identity, wire digest, profile and adapter version; it does not prove a live
provider call. Combined observer/continuation manifest mode is currently rejected.

## Support and usage

| Feature | Contract |
|---|---|
| Text JSON/SSE | Validated projection; raw provider steps retained separately |
| Functions and parallel calls | Names, call IDs, counts, schemas and result links checked |
| Custom text, namespaces, patch grammar | Existing JSON/name bridge and registered grammar |
| Instructions | Leading system/developer roles and order in an explicit instruction envelope; late instructions rejected |
| Output schema | Conservative object/array/scalar, enum, required, properties, items, anyOf and additionalProperties subset; output checked |
| Thinking | minimal, low, medium, high; unknown efforts rejected |
| Parallel disable | Rejected when callable tools exist; accepted when no tools can be called |
| Strict function arguments | strict:true is not represented by the pinned function contract and is rejected |
| Hosted tools and multimodal input/output | Rejected, including images, audio, video, search and code execution |
| Provider storage/background | Explicit store:false and background:false |
| Requires action | Completed Responses tool items; session retains pending tool state |
| Incomplete or interrupted output | No successful finalization or automatic retry; host reconciliation required |

Terminal usage takes precedence. If it is absent, use the last cumulative usage
from step delta metadata or step stop; never sum cumulative and per-step counters.
For usage, input_tokens equals total_input_tokens. output_tokens is the sum of
available total_output_tokens and total_thought_tokens; it remains unknown if either
counter is absent. total_tokens is preserved and checked against the sum when all
counters exist. total_cached_tokens is an input subset, never added again. Reasoning
tokens are also reported separately. Missing counters remain null. Nonzero hosted-tool
usage is rejected. Matching token or effort labels does not establish equal model cost.

## Host control and resume

Control routes use the control token as Bearer authentication and are separate from
the model API. A missing session is an error, never an instruction to create one.

| Method and path | Body or result |
|---|---|
| POST /__continuation/sessions | origin containing route, realm and generation; returns id, epoch, revision and status |
| GET /__continuation/sessions/{id} | Current metadata and pending_tools; no provider payload |
| POST /__continuation/sessions/{id}/transitions | revision, kind, portable_sha256, decision_reference, pending_tools:false, pending_approvals:false |

Construct origin.route from the selected manifest route, excluding api_key_env.
A credential realm/generation is a host identity, not an environment variable name or
key value. Bind the session ID into the dedicated Codex provider's fixed http_headers
using x-gateway-session. Keep Responses wire_api and disable transport retries.
The control token must never be present in the Codex environment or configuration.

Normal restart requires the same session, protected key, store, route and Codex history.
Each response adds only its new encrypted provider steps to reasoning.encrypted_content.
The next request authenticates the original input prefix and corresponding public output.
Public text/tool items never substitute for missing provider signatures.

Before local compaction, the host verifies no pending tools/approvals and sends a
compact_begin transition. It then calls thread/compact/start and verifies completion,
summary and completed tool results. Preserve the original thread. Put the checked
portable context into one user message in a fresh Codex thread, then compact_commit
with its digest to activate a new epoch in the same session. This explicit transfer
avoids Codex's mid-history developer reinsertion; it does not preserve removed signatures.
Unobserved automatic compaction or unexplained history changes fail verification.

portable_sha256 hashes a one-element JSON array containing that user message: sort
object keys, use compact UTF-8 JSON, normalize omitted message type to message, remove
item id/status, null phase/internal_chat_message_metadata_passthrough, and empty output
text annotations. No text or tool content is normalized. The new context may contain
Codex's leading instructions and environment messages; the bound portable user message
must appear exactly once and no prior assistant/tool/provider items may be imported.
See the [executable synthetic host](../tests/codex/interactions_continuity.py).

## Backup and failure recovery

SQLite uses WAL, synchronous=FULL, foreign keys, revision checks and exclusive store
ownership. A blocking worker performs database and cryptographic operations. Initialization
is explicit; serve never replaces a missing/corrupt database with an empty one. There is
no automatic migration, expiry, eviction, fallback or inference retry. Storage admission
reserves max_response_bytes for each attempt; max_store_bytes bounds these reservations
and the database page count. WAL and host backups require additional filesystem space.
Encrypted payload plaintext is capped at 2 MiB, and incoming/outgoing byte limits also
apply to envelopes and converted SSE. A limit error never silently truncates state.

For a consistent backup, stop Codex activity and the gateway, then preserve the database,
its WAL/SHM sidecars if present, store metadata, configuration, stable key/key ID and Codex
history together. Keep all copies private. Do not copy only a live main database file.
Restore a mutually compatible set while the server is stopped. The lock file is not
execution evidence. The host must enforce Windows directory ACLs; Unix privacy checks
are enforced locally. Keys, bodies, signatures and envelopes must stay out of logs.

A finalized execution record with only its encrypted payload missing can be repaired
from the matching Codex envelope without a model call. A missing record or pending/unknown
attempt blocks automatic use. After investigating the earlier execution and confirming
no pending tool/approval work, the host records a recover decision with a new portable
message digest and starts a new epoch/thread. This does not invent completion of the
previous attempt. Lost/changed keys or incompatible schemas fail explicitly. Rollback
requires a compatible binary, database, key and Codex history together.

The ContinuationStore contract is backend-neutral, but only SQLite is implemented.
PostgreSQL, Redis, public Responses storage/lookup/deletion and previous_response_id are
outside this support boundary. Gateway/provider shutdown does not prove a provider-side
job was cancelled or that the provider did not charge for it.

The common managed executor writes gateway-continuation/v2 records with a typed
native payload and completion outcome. It authenticates gateway-continuation/v1
records and checks their original finalized digests before conversion. Existing
Gemini route bindings, database tables, keys and history remain valid; no automatic
migration or ciphertext rewrite occurs. The manifest declares replay_versions as
read [1, 2] and write 2. Old hosts reject the new manifest; old binaries cannot be
assumed to read v2 records. Rollback requires a compatible binary, DB, key and
Codex history together. Public reasoning summaries, when present, participate in
the authenticated history digest separately from native provider state. Managed Messages uses an explicit Claude reasoning contract. Chat reasoning remains
disabled until its explicit contracts are implemented.
