<a id="관리형-reasoning과-재개"></a>
<a id="실행과-표시"></a>
<a id="호스트-설정과-호환성"></a>
<a id="복구와-압축"></a>
<a id="사용량-기록과-실패"></a>
<a id="검증과-한계"></a>

# Managed reasoning and continuation

[English](managed-continuation.md) | [한국어](ko/managed-continuation.md)

The managed execution layer supports Gemini Interactions, Claude Messages
Adaptive/Manual thinking, and explicit DeepSeek/OpenRouter Chat contracts.
Messages and Chat remain stateless by default. Database configuration alone does
not enable reasoning: select continuation_mode="managed" and a reasoning_contract
on the capability profile. Gemini retains its existing managed default.

## Execution and display

The request follows Responses input validation, session/history verification,
Request IR, the provider adapter, native response assembly, encryption and durable
finalization, then Responses completion. The common executor owns attempts,
revision conflicts, capacity reservations, recovery and the publication barrier.
Adapters own wire conversion, native replay, public output and termination.

Public thinking text becomes Codex reasoning summaries. Original signed content,
redacted data and encrypted details remain provider state. Public summaries never
reconstruct that state. One authenticated envelope contains only the new native
state from one response. Public summaries and ordinary output are authenticated
together against Codex history; native replay replaces those spans exactly once.

Text and reasoning deltas may stream before storage completes. Executable tool
completion, the recovery envelope and a successful terminal wait for finalized
storage. A provider asking for tools has completed its model execution; the host's
tool work is still pending. The gateway neither executes tools nor approves them.

## Host configuration and compatibility

Use the [store/key/session setup](interactions.md#configuration) and the
[host control API](interactions.md#host-control-and-resume). Provider configuration
is documented for [Messages](messages.md#managed-thinking) and
[Chat](chat-completions.md#managed-reasoning). A dedicated Codex provider sends the
host-created x-gateway-session as a fixed HTTP header. Unknown sessions are rejected.
Only the local gateway token and session ID reach Codex. The control token,
provider credential and independent stable 256-bit encryption key stay with the host.

The standalone managed manifest is gateway-embedded-manifest/v3 and readiness is
gateway-ready/v3. They declare replay_versions read [1, 2], write 2. Enabling an
extension with managed configuration produces gateway-extended-manifest/v3 and
gateway-extended-ready/v3. Validate the nested gateway manifest, configuration
and execution digests, supported replay versions, and declared usage profiles.
An older host must reject an unfamiliar version. A manifest is a configuration
contract, not evidence that a provider was qualified.

Existing Gemini v1 records are authenticated in their original serialized form
and checked against their original finalized digest before internal conversion.
The gateway writes ReplayV2 for new responses, preserving old ciphertext and
Codex history. Gemini route binding and SQLite tables are unchanged. No automatic
migration is performed. Messages/Chat contracts are part of new route bindings;
changing provider, model or contract requires a new session.

## Recovery and compaction

Restart with the same database, stable key/key ID, Codex history and host binding.
The database is authoritative. Only a finalized record with missing payload can
be repaired automatically from an envelope with the same authenticated binding
and digest. Missing execution records and pending/unknown attempts block model
calls. The host must reconcile tool outcomes and explicitly activate a new epoch;
a recovery transition does not manufacture completion of an earlier attempt.

The host registers compaction before thread/compact/start. Preserve the original
Codex task, verify the resulting portable summary and completed tools, then move
portable context to a fresh task and activate the new epoch of the same session.
Pending tools/approvals block the transition. Dropped provider signatures are not
claimed to survive. Unobserved automatic compaction or unexplained history changes
cannot be accepted as ordinary continuation.

Stop the gateway before copying the continuation database and its SQLite sidecars.
Back up the matching key/key ID, host binding and Codex history as one compatible
recovery set. Preserve directory permissions. serve must not replace missing or
corrupt storage with an empty database. Key loss, changed origin and unsupported
schemas fail explicitly. Rollback needs a compatible binary and recovery set;
old binaries are not promised to read v2 records. There is no automatic TTL deletion;
capacity exhaustion rejects new work before provider dispatch.

## Usage recording and failures

The optional [Usage Recorder](usage-accounting.md) stores numeric metadata in its
own ledger. It receives no provider steps, reasoning text, signatures or envelopes.
Continuation does not own accounting persistence, exports or tool accounting.
DeepSeek uses deepseek/v1; OpenRouter uses chat/v1; Messages uses messages/v1 and
Gemini uses gemini_interactions/v1. Unreported counters stay unknown. Thinking is
not added twice to counters that already include it.

With durable_local, recorder admission is confirmed before continuation attempt
creation and provider dispatch. After provider validation, continuation finalizes
first, followed by the recorder's final local acknowledgement, before executable
completion is published. Recorder admission failure makes zero provider calls.
A final recorder failure can leave a finalized continuation record without a client
completion; preserve both stores and reconcile the task before continuing. This
is not permission to repeat inference. A recorder IPC failure requires a supervised
restart; the gateway does not silently reconnect or replay its request.

Upstream outcome, gateway outcome and usage finality remain separate. Recorded
completion does not prove all bytes reached Codex, tools ran, or a provider charged
nothing. Cancellation closes the upstream without draining it for usage. Partial
observations remain partial. Keep recorder backup/retention/export procedures
separate from the continuation recovery set.

## Validation and limits

The supported claim is host-managed Codex 0.154.0 with synthetic
Claude/DeepSeek/OpenRouter: reasoning display, original-state preservation and
resume. Required checks retain the original 49 scenarios and add 60 reasoning
scenarios, signed/opaque-only continuity, v1/v2 repair, compaction, recorder failure
barriers and four-platform package reasoning/restart smoke. Both output and
heartbeat-only cancellation use the existing five-second upstream-close bound.

Actual model quality, compatibility and cost qualification need separate acceptance.
Direct generic CLI/Desktop setup, Native Responses managed mode, previous_response_id,
public response storage APIs and PostgreSQL/Redis continuation backends are outside
this contract. PostgreSQL usage export is an independent recorder feature.
